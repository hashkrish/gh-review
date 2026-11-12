//! App-side half of the two-way sync between local draft comments and the
//! viewer's pending review on GitHub. The worker and merge rules live in
//! [`crate::sync`]; this module applies local edits, applies sync results to
//! the draft list, and drives the conflict prompt.

use crossterm::event::KeyCode;

use crate::sync::{Conflict, ConflictKind, SyncEvent, SyncOp, reconcile};
use crate::types::{ReviewComment, ReviewEvent};

use super::App;

impl App {
    pub(super) fn enqueue(&self, op: SyncOp) {
        let _ = self.sync_tx.send(op);
    }

    /// Drafts for `pr`: the live list for the current PR, or the cached copy
    /// for another PR in the stack. `None` if that PR isn't tracked.
    fn drafts_mut(&mut self, pr: u64) -> Option<&mut Vec<ReviewComment>> {
        if pr == self.pr_number {
            Some(&mut self.pending_comments)
        } else {
            self.pr_cache.get_mut(pr).map(|s| &mut s.pending_comments)
        }
    }

    pub(crate) fn request_pull(&self, pr: u64) {
        self.enqueue(SyncOp::Pull { pr });
    }

    /// Add a new draft locally and push it to GitHub's pending review.
    pub(crate) fn add_draft(&mut self, mut comment: ReviewComment) {
        comment.local_id = self.next_local_id;
        self.next_local_id += 1;
        self.enqueue(SyncOp::Create {
            pr: self.pr_number,
            comment: comment.clone(),
        });
        self.pending_comments.push(comment);
    }

    /// Change a draft's text and push the edit to GitHub.
    pub(crate) fn edit_draft(&mut self, idx: usize, body: String) {
        let Some(draft) = self.pending_comments.get_mut(idx) else {
            return;
        };
        draft.body = body.clone();
        let local_id = draft.local_id;
        // Unlinked drafts have no remote copy yet; the create op (or a later
        // sync) carries the latest text.
        if let Some(node_id) = draft.node_id.clone() {
            self.enqueue(SyncOp::Update {
                pr: self.pr_number,
                local_id,
                node_id,
                body,
            });
        }
    }

    /// Remove a draft locally and delete it from GitHub's pending review.
    pub(crate) fn remove_draft(&mut self, idx: usize) {
        if idx >= self.pending_comments.len() {
            return;
        }
        let draft = self.pending_comments.remove(idx);
        // Unlinked: if its create is still in flight, `Created` notices the
        // draft is gone and deletes the remote copy.
        if let Some(node_id) = draft.node_id {
            self.tombstones.insert(node_id.clone());
            self.enqueue(SyncOp::Delete { node_id });
        }
    }

    /// Submit the pending review. GitHub already holds the linked drafts, so
    /// only drafts that never reached it are sent along.
    pub(crate) fn submit_review(&mut self, event: ReviewEvent, body: String) {
        if self.sync_conflicts.iter().any(|c| c.pr == self.pr_number) {
            self.status
                .error("Resolve the pending-review sync conflicts before submitting");
            return;
        }
        let unlinked = self
            .pending_comments
            .iter()
            .filter(|c| c.node_id.is_none())
            .cloned()
            .collect();
        self.status.info(format!("Submitting {}...", event.label()));
        self.enqueue(SyncOp::Submit {
            pr: self.pr_number,
            event,
            body,
            unlinked,
        });
    }

    pub(crate) fn handle_sync_event(&mut self, event: SyncEvent) {
        match event {
            SyncEvent::Created {
                pr,
                local_id,
                node_id,
                body,
            } => self.on_created(pr, local_id, node_id, body),
            SyncEvent::Updated { pr, local_id, body } => {
                if let Some(d) = self
                    .drafts_mut(pr)
                    .and_then(|ds| ds.iter_mut().find(|d| d.local_id == local_id))
                {
                    d.synced_body = Some(body);
                }
            }
            SyncEvent::Pulled { pr, remote } => self.on_pulled(pr, remote),
            SyncEvent::CreateFailed { error } => self.status.error(format!(
                "Couldn't save comment to GitHub: {error} (retried on next sync or submit)"
            )),
            SyncEvent::UpdateFailed { error } => self.status.error(format!(
                "Couldn't update comment on GitHub: {error} (retried on next sync)"
            )),
            SyncEvent::DeleteFailed { error } => self.status.error(format!(
                "Couldn't delete comment on GitHub: {error} (it may reappear on next sync)"
            )),
            SyncEvent::PullFailed { error } => self
                .status
                .error(format!("Couldn't sync pending review: {error}")),
            SyncEvent::ViewedSet { pr, path } => self.on_viewed_set(pr, path),
            SyncEvent::ViewedSetFailed {
                pr,
                path,
                viewed,
                error,
            } => self.on_viewed_set_failed(pr, path, viewed, error),
            SyncEvent::ViewedPulled { pr, states } => self.on_viewed_pulled(pr, states),
            SyncEvent::ViewedPullFailed { error } => self
                .status
                .error(format!("Couldn't sync viewed files: {error}")),
        }
    }

    fn on_created(&mut self, pr: u64, local_id: u64, node_id: String, body: String) {
        let Some(drafts) = self.drafts_mut(pr) else {
            return;
        };
        let Some(draft) = drafts.iter_mut().find(|d| d.local_id == local_id) else {
            // Deleted while the create was in flight: remove the remote copy.
            self.tombstones.insert(node_id.clone());
            self.enqueue(SyncOp::Delete { node_id });
            return;
        };
        draft.node_id = Some(node_id.clone());
        draft.synced_body = Some(body.clone());
        // Edited while the create was in flight: push the newer text.
        let newer = (draft.body != body).then(|| draft.body.clone());
        if let Some(body) = newer {
            self.enqueue(SyncOp::Update {
                pr,
                local_id,
                node_id,
                body,
            });
        }
    }

    fn on_pulled(&mut self, pr: u64, remote: Vec<crate::types::RemoteDraft>) {
        let tombstones = self.tombstones.clone();
        let mut next_id = self.next_local_id;
        let Some(drafts) = self.drafts_mut(pr) else {
            return;
        };
        let rec = reconcile(pr, drafts, &remote, &tombstones);
        let mut ops = Vec::new();

        let changed = rec.import.len() + rec.adopt_remote.len() + rec.drop_local.len();
        let (imported, adopted, dropped) = (
            rec.import.len(),
            rec.adopt_remote.len(),
            rec.drop_local.len(),
        );

        for r in rec.import {
            drafts.push(ReviewComment {
                path: r.path,
                line: r.line,
                side: r.side,
                synced_body: Some(r.body.clone()),
                body: r.body,
                start_line: r.start_line,
                start_side: r.start_side,
                local_id: next_id,
                node_id: Some(r.node_id),
            });
            next_id += 1;
        }
        for (id, body) in rec.adopt_remote {
            if let Some(d) = drafts.iter_mut().find(|d| d.local_id == id) {
                d.synced_body = Some(body.clone());
                d.body = body;
            }
        }
        for (id, body) in rec.converged {
            if let Some(d) = drafts.iter_mut().find(|d| d.local_id == id) {
                d.synced_body = Some(body);
            }
        }
        drafts.retain(|d| !rec.drop_local.contains(&d.local_id));

        for id in rec.push_update {
            if let Some(d) = drafts.iter().find(|d| d.local_id == id)
                && let Some(node_id) = d.node_id.clone()
            {
                ops.push(SyncOp::Update {
                    pr,
                    local_id: id,
                    node_id,
                    body: d.body.clone(),
                });
            }
        }
        // Drafts whose create failed earlier: try again (the worker skips any
        // that have since succeeded).
        for d in drafts.iter().filter(|d| d.node_id.is_none()) {
            ops.push(SyncOp::Create {
                pr,
                comment: d.clone(),
            });
        }

        self.next_local_id = next_id;
        for op in ops {
            self.enqueue(op);
        }

        // Replace any earlier conflicts for this PR with the current set.
        self.sync_conflicts.retain(|c| c.pr != pr);
        let conflicts = rec.conflicts.len();
        self.sync_conflicts.extend(rec.conflicts);

        if pr == self.pr_number {
            if changed > 0 {
                self.rebuild_display();
            }
            if conflicts > 0 {
                self.status.error(format!(
                    "{conflicts} pending-review sync conflict{}",
                    if conflicts == 1 { "" } else { "s" }
                ));
            } else if changed > 0 {
                self.status.success(format!(
                    "Synced pending review: {imported} new, {adopted} updated, {dropped} removed"
                ));
            }
        }
    }

    // --- Conflict prompt ---

    /// The prompt shows only when no other modal is up, so it can't swallow
    /// keystrokes meant for a comment being typed.
    pub(crate) fn conflict_prompt_active(&self) -> bool {
        !self.sync_conflicts.is_empty()
            && !self.comment_input.visible
            && !self.review_confirm.visible
            && !self.command_bar.active
            && !self.search_bar.active
            && !self.file_picker.is_filter_active()
            && !self.show_help
    }

    pub(super) fn handle_conflict_key(&mut self, key: crossterm::event::KeyEvent) {
        match key.code {
            KeyCode::Char('l') => self.resolve_conflict(true),
            KeyCode::Char('g') => self.resolve_conflict(false),
            // Decide later: the conflicts are detected again on the next sync.
            KeyCode::Esc => self.sync_conflicts.clear(),
            _ => {}
        }
    }

    fn resolve_conflict(&mut self, keep_local: bool) {
        let Some(Conflict {
            pr,
            local_id,
            kind,
            remote_body,
            ..
        }) = self.sync_conflicts.pop_front()
        else {
            return;
        };

        let mut ops = Vec::new();
        if let Some(drafts) = self.drafts_mut(pr) {
            match (kind, keep_local) {
                (ConflictKind::BothEdited, true) => {
                    if let Some(d) = drafts.iter().find(|d| d.local_id == local_id)
                        && let Some(node_id) = d.node_id.clone()
                    {
                        ops.push(SyncOp::Update {
                            pr,
                            local_id,
                            node_id,
                            body: d.body.clone(),
                        });
                    }
                }
                (ConflictKind::BothEdited, false) => {
                    if let (Some(d), Some(body)) = (
                        drafts.iter_mut().find(|d| d.local_id == local_id),
                        remote_body,
                    ) {
                        d.synced_body = Some(body.clone());
                        d.body = body;
                    }
                }
                (ConflictKind::DeletedOnGithub, true) => {
                    if let Some(d) = drafts.iter_mut().find(|d| d.local_id == local_id) {
                        // Re-create it on GitHub as a fresh comment.
                        d.node_id = None;
                        d.synced_body = None;
                        ops.push(SyncOp::Create {
                            pr,
                            comment: d.clone(),
                        });
                    }
                }
                (ConflictKind::DeletedOnGithub, false) => {
                    drafts.retain(|d| d.local_id != local_id);
                }
            }
        }
        for op in ops {
            self.enqueue(op);
        }
        if pr == self.pr_number {
            self.rebuild_display();
        }
    }
}

#[cfg(test)]
mod tests;
