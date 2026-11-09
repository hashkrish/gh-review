//! Two-way sync between local draft comments and the viewer's pending review
//! on GitHub, plus the per-file "Viewed" checkbox.
//!
//! Local changes are pushed through a single ordered worker task, so a draft's
//! create, edit and delete always reach GitHub in the order they were made.
//! Pulling is a worker op too; [`reconcile`] then compares the fetched drafts
//! with the local ones and decides what to import, push, adopt or flag as a
//! conflict. It is pure so the merge rules can be unit tested.

use std::collections::{HashMap, HashSet};

use tokio::sync::mpsc;

use crate::event::AppEvent;
use crate::types::{RemoteDraft, ReviewComment, ReviewEvent};

/// Work for the sync worker. Every op names the PR it applies to because the
/// user can switch PRs in a stack while earlier ops are still in flight.
#[derive(Debug)]
pub enum SyncOp {
    Create {
        pr: u64,
        comment: ReviewComment,
    },
    Update {
        pr: u64,
        local_id: u64,
        node_id: String,
        body: String,
    },
    Delete {
        node_id: String,
    },
    Pull {
        pr: u64,
    },
    /// Set or clear GitHub's per-file "Viewed" checkbox.
    SetViewed {
        pr: u64,
        path: String,
        viewed: bool,
    },
    PullViewed {
        pr: u64,
    },
    /// Submit the pending review. `unlinked` are drafts that never reached
    /// GitHub; they are added to the review first.
    Submit {
        pr: u64,
        event: ReviewEvent,
        body: String,
        unlinked: Vec<ReviewComment>,
    },
}

/// Results the worker reports back to the app.
#[derive(Debug)]
pub enum SyncEvent {
    Created {
        pr: u64,
        local_id: u64,
        node_id: String,
        /// The body that was sent, which may be stale if the user kept editing.
        body: String,
    },
    CreateFailed {
        error: String,
    },
    Updated {
        pr: u64,
        local_id: u64,
        body: String,
    },
    UpdateFailed {
        error: String,
    },
    DeleteFailed {
        error: String,
    },
    Pulled {
        pr: u64,
        remote: Vec<RemoteDraft>,
    },
    PullFailed {
        error: String,
    },
    ViewedSet {
        pr: u64,
        path: String,
    },
    ViewedSetFailed {
        pr: u64,
        path: String,
        /// The state that was being pushed, so the app can revert it.
        viewed: bool,
        error: String,
    },
    ViewedPulled {
        pr: u64,
        states: HashMap<String, bool>,
    },
    ViewedPullFailed {
        error: String,
    },
}

pub fn spawn_worker(
    repo: String,
    mut rx: mpsc::UnboundedReceiver<SyncOp>,
    tx: mpsc::UnboundedSender<AppEvent>,
) {
    tokio::spawn(async move {
        let send = |ev: SyncEvent| {
            let _ = tx.send(AppEvent::Sync(ev));
        };
        // Drafts already created on GitHub by this worker. A retry queued for
        // a draft whose first attempt succeeded in the meantime is skipped.
        let mut created: HashSet<u64> = HashSet::new();
        // Pending review id per PR, so each op doesn't re-query it.
        let mut review_ids: HashMap<u64, String> = HashMap::new();
        // PR node ids, needed by the viewed-file mutations.
        let mut pr_node_ids: HashMap<u64, String> = HashMap::new();

        while let Some(op) = rx.recv().await {
            match op {
                SyncOp::SetViewed { pr, path, viewed } => {
                    match set_viewed(&repo, pr, &path, viewed, &mut pr_node_ids).await {
                        Ok(()) => send(SyncEvent::ViewedSet { pr, path }),
                        Err(e) => send(SyncEvent::ViewedSetFailed {
                            pr,
                            path,
                            viewed,
                            error: format!("{e:#}"),
                        }),
                    }
                }
                SyncOp::PullViewed { pr } => match crate::gh::fetch_viewed_files(&repo, pr).await {
                    Ok(states) => send(SyncEvent::ViewedPulled { pr, states }),
                    Err(e) => send(SyncEvent::ViewedPullFailed {
                        error: format!("{e:#}"),
                    }),
                },
                SyncOp::Create { pr, comment } => {
                    if created.contains(&comment.local_id) {
                        continue;
                    }
                    match create_draft(&repo, pr, &comment, &mut review_ids).await {
                        Ok(node_id) => {
                            created.insert(comment.local_id);
                            send(SyncEvent::Created {
                                pr,
                                local_id: comment.local_id,
                                node_id,
                                body: comment.body,
                            });
                        }
                        Err(e) => send(SyncEvent::CreateFailed {
                            error: format!("{e:#}"),
                        }),
                    }
                }
                SyncOp::Update {
                    pr,
                    local_id,
                    node_id,
                    body,
                } => match crate::gh::update_draft_comment(&node_id, &body).await {
                    Ok(()) => send(SyncEvent::Updated { pr, local_id, body }),
                    Err(e) => send(SyncEvent::UpdateFailed {
                        error: format!("{e:#}"),
                    }),
                },
                SyncOp::Delete { node_id } => {
                    if let Err(e) = crate::gh::delete_draft_comment(&node_id).await {
                        send(SyncEvent::DeleteFailed {
                            error: format!("{e:#}"),
                        });
                    }
                }
                SyncOp::Pull { pr } => match crate::gh::fetch_pending_drafts(&repo, pr).await {
                    Ok(remote) => send(SyncEvent::Pulled { pr, remote }),
                    Err(e) => send(SyncEvent::PullFailed {
                        error: format!("{e:#}"),
                    }),
                },
                SyncOp::Submit {
                    pr,
                    event,
                    body,
                    unlinked,
                } => {
                    let result = submit(
                        &repo,
                        pr,
                        event,
                        &body,
                        unlinked,
                        &mut created,
                        &mut review_ids,
                    )
                    .await;
                    // The pending review is gone once submitted (and may be
                    // stale if the submit failed), so never reuse the id.
                    review_ids.remove(&pr);
                    match result {
                        Ok(linked) => {
                            for ev in linked {
                                send(ev);
                            }
                            let _ = tx.send(AppEvent::ReviewSubmitted);
                        }
                        Err((linked, e)) => {
                            // Keep what did reach GitHub linked, so the next
                            // pull doesn't import it as a duplicate.
                            for ev in linked {
                                send(ev);
                            }
                            let _ = tx.send(AppEvent::Error(format!("Submit failed: {e:#}")));
                        }
                    }
                }
            }
        }
    });
}

async fn set_viewed(
    repo: &str,
    pr: u64,
    path: &str,
    viewed: bool,
    pr_node_ids: &mut HashMap<u64, String>,
) -> anyhow::Result<()> {
    let pr_node_id = match pr_node_ids.get(&pr) {
        Some(id) => id.clone(),
        None => {
            let id = crate::gh::fetch_pr_node_id(repo, pr).await?;
            pr_node_ids.insert(pr, id.clone());
            id
        }
    };
    crate::gh::set_file_viewed(&pr_node_id, path, viewed).await
}

async fn review_id_for(
    repo: &str,
    pr: u64,
    review_ids: &mut HashMap<u64, String>,
) -> anyhow::Result<String> {
    if let Some(id) = review_ids.get(&pr) {
        return Ok(id.clone());
    }
    let id = crate::gh::ensure_pending_review(repo, pr).await?;
    review_ids.insert(pr, id.clone());
    Ok(id)
}

async fn create_draft(
    repo: &str,
    pr: u64,
    comment: &ReviewComment,
    review_ids: &mut HashMap<u64, String>,
) -> anyhow::Result<String> {
    let review_id = review_id_for(repo, pr, review_ids).await?;
    let result = crate::gh::create_draft_thread(&review_id, comment).await;
    if result.is_err() {
        // The cached review may have been submitted or deleted on github.com.
        review_ids.remove(&pr);
    }
    result
}

type SubmitFailure = (Vec<SyncEvent>, anyhow::Error);

async fn submit(
    repo: &str,
    pr: u64,
    event: ReviewEvent,
    body: &str,
    unlinked: Vec<ReviewComment>,
    created: &mut HashSet<u64>,
    review_ids: &mut HashMap<u64, String>,
) -> Result<Vec<SyncEvent>, SubmitFailure> {
    let mut linked = Vec::new();
    let review_id = match review_id_for(repo, pr, review_ids).await {
        Ok(id) => id,
        Err(e) => return Err((linked, e)),
    };
    for comment in unlinked {
        if created.contains(&comment.local_id) {
            continue;
        }
        match crate::gh::create_draft_thread(&review_id, &comment).await {
            Ok(node_id) => {
                created.insert(comment.local_id);
                linked.push(SyncEvent::Created {
                    pr,
                    local_id: comment.local_id,
                    node_id,
                    body: comment.body,
                });
            }
            Err(e) => return Err((linked, e)),
        }
    }
    match crate::gh::submit_pending_review(&review_id, event, body).await {
        Ok(()) => Ok(linked),
        Err(e) => Err((linked, e)),
    }
}

// ── Reconciliation ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConflictKind {
    /// Both sides changed the comment's text since the last sync.
    BothEdited,
    /// The comment was deleted on GitHub but changed locally.
    DeletedOnGithub,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Conflict {
    pub pr: u64,
    pub local_id: u64,
    pub kind: ConflictKind,
    pub path: String,
    pub line: usize,
    pub local_body: String,
    /// GitHub's text; `None` when it was deleted there.
    pub remote_body: Option<String>,
}

#[derive(Debug, Default, PartialEq)]
pub struct Reconciliation {
    /// On GitHub but unknown locally: add as new local drafts.
    pub import: Vec<RemoteDraft>,
    /// Changed on GitHub only: take its text. `(local_id, body)`.
    pub adopt_remote: Vec<(u64, String)>,
    /// Gone from GitHub and unchanged locally: drop the local draft.
    pub drop_local: Vec<u64>,
    /// Changed locally only (e.g. an earlier push failed): push again.
    pub push_update: Vec<u64>,
    /// Both changed to the same text: just record it as synced.
    pub converged: Vec<(u64, String)>,
    pub conflicts: Vec<Conflict>,
}

/// Compare local drafts with GitHub's pending review.
///
/// `tombstones` holds node ids the user deleted locally; GitHub may still list
/// them until the delete lands, and they must not be re-imported.
pub fn reconcile(
    pr: u64,
    local: &[ReviewComment],
    remote: &[RemoteDraft],
    tombstones: &HashSet<String>,
) -> Reconciliation {
    let remote_by_id: HashMap<&str, &RemoteDraft> =
        remote.iter().map(|r| (r.node_id.as_str(), r)).collect();
    let mut out = Reconciliation::default();
    let mut linked: HashSet<&str> = HashSet::new();

    for draft in local {
        // Never pushed: the create op (or the submit) owns it.
        let Some(node_id) = draft.node_id.as_deref() else {
            continue;
        };
        linked.insert(node_id);
        let local_changed = draft.synced_body.as_deref() != Some(draft.body.as_str());

        match remote_by_id.get(node_id) {
            Some(r) => {
                // Without a recorded sync point, assume GitHub's copy is it.
                let synced = draft.synced_body.as_deref().unwrap_or(r.body.as_str());
                let local_changed = draft.body != synced;
                let remote_changed = r.body != synced;
                match (local_changed, remote_changed) {
                    (false, false) => {}
                    (true, false) => out.push_update.push(draft.local_id),
                    (false, true) => out.adopt_remote.push((draft.local_id, r.body.clone())),
                    (true, true) if draft.body == r.body => {
                        out.converged.push((draft.local_id, r.body.clone()))
                    }
                    (true, true) => out.conflicts.push(Conflict {
                        pr,
                        local_id: draft.local_id,
                        kind: ConflictKind::BothEdited,
                        path: draft.path.clone(),
                        line: draft.line,
                        local_body: draft.body.clone(),
                        remote_body: Some(r.body.clone()),
                    }),
                }
            }
            None if local_changed => out.conflicts.push(Conflict {
                pr,
                local_id: draft.local_id,
                kind: ConflictKind::DeletedOnGithub,
                path: draft.path.clone(),
                line: draft.line,
                local_body: draft.body.clone(),
                remote_body: None,
            }),
            None => out.drop_local.push(draft.local_id),
        }
    }

    for r in remote {
        if !linked.contains(r.node_id.as_str()) && !tombstones.contains(&r.node_id) {
            out.import.push(r.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    fn local(id: u64, node: Option<&str>, synced: Option<&str>, body: &str) -> ReviewComment {
        ReviewComment {
            path: "a.rs".into(),
            line: 3,
            side: Side::Right,
            body: body.into(),
            start_line: None,
            start_side: None,
            local_id: id,
            node_id: node.map(String::from),
            synced_body: synced.map(String::from),
        }
    }

    fn remote(node: &str, body: &str) -> RemoteDraft {
        RemoteDraft {
            node_id: node.into(),
            path: "a.rs".into(),
            line: 3,
            side: Side::Right,
            body: body.into(),
            start_line: None,
            start_side: None,
        }
    }

    fn run(l: &[ReviewComment], r: &[RemoteDraft]) -> Reconciliation {
        reconcile(1, l, r, &HashSet::new())
    }

    #[test]
    fn identical_drafts_need_nothing() {
        let out = run(
            &[local(1, Some("n1"), Some("x"), "x")],
            &[remote("n1", "x")],
        );
        assert_eq!(out, Reconciliation::default());
    }

    #[test]
    fn imports_drafts_created_on_github() {
        let out = run(&[], &[remote("n9", "hi")]);
        assert_eq!(out.import, vec![remote("n9", "hi")]);
    }

    #[test]
    fn does_not_reimport_locally_deleted_drafts() {
        let tombstones: HashSet<String> = ["n9".to_string()].into();
        let out = reconcile(1, &[], &[remote("n9", "hi")], &tombstones);
        assert!(out.import.is_empty());
    }

    #[test]
    fn unlinked_local_drafts_are_left_alone() {
        let out = run(&[local(1, None, None, "draft")], &[remote("n1", "other")]);
        assert_eq!(out.import, vec![remote("n1", "other")]);
        assert!(out.conflicts.is_empty() && out.drop_local.is_empty());
    }

    #[test]
    fn adopts_remote_edit_when_local_unchanged() {
        let out = run(
            &[local(1, Some("n1"), Some("a"), "a")],
            &[remote("n1", "b")],
        );
        assert_eq!(out.adopt_remote, vec![(1, "b".to_string())]);
    }

    #[test]
    fn retries_push_when_only_local_changed() {
        let out = run(
            &[local(1, Some("n1"), Some("a"), "b")],
            &[remote("n1", "a")],
        );
        assert_eq!(out.push_update, vec![1]);
    }

    #[test]
    fn flags_conflict_when_both_changed() {
        let out = run(
            &[local(1, Some("n1"), Some("a"), "b")],
            &[remote("n1", "c")],
        );
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(out.conflicts[0].kind, ConflictKind::BothEdited);
        assert_eq!(out.conflicts[0].remote_body.as_deref(), Some("c"));
    }

    #[test]
    fn same_edit_on_both_sides_converges() {
        let out = run(
            &[local(1, Some("n1"), Some("a"), "b")],
            &[remote("n1", "b")],
        );
        assert_eq!(out.converged, vec![(1, "b".to_string())]);
        assert!(out.conflicts.is_empty());
    }

    #[test]
    fn drops_local_when_deleted_on_github_and_unchanged() {
        let out = run(&[local(1, Some("n1"), Some("a"), "a")], &[]);
        assert_eq!(out.drop_local, vec![1]);
    }

    #[test]
    fn flags_conflict_when_deleted_on_github_but_edited_locally() {
        let out = run(&[local(1, Some("n1"), Some("a"), "b")], &[]);
        assert_eq!(out.conflicts.len(), 1);
        assert_eq!(out.conflicts[0].kind, ConflictKind::DeletedOnGithub);
        assert_eq!(out.conflicts[0].remote_body, None);
    }
}
