use tokio::sync::mpsc;

use super::*;
use crate::sync::SyncOp;
use crate::types::{RemoteDraft, Side};

/// An app whose sync ops land in the returned receiver instead of a worker.
fn test_app() -> (App, mpsc::UnboundedReceiver<SyncOp>) {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 1, tx);
    let (sync_tx, sync_rx) = mpsc::unbounded_channel();
    app.sync_tx = sync_tx;
    (app, sync_rx)
}

fn draft(body: &str) -> ReviewComment {
    ReviewComment::new_local("a.rs".into(), 3, Side::Right, body.into(), None, None)
}

/// A draft already linked to GitHub's copy, last synced with body "a".
fn linked_draft(body: &str) -> ReviewComment {
    ReviewComment {
        local_id: 1,
        node_id: Some("n1".into()),
        synced_body: Some("a".into()),
        ..draft(body)
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

fn drain(rx: &mut mpsc::UnboundedReceiver<SyncOp>) -> Vec<SyncOp> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

fn created(local_id: u64, node: &str, body: &str) -> SyncEvent {
    SyncEvent::Created {
        pr: 1,
        local_id,
        node_id: node.into(),
        body: body.into(),
    }
}

#[test]
fn add_draft_assigns_unique_ids_and_queues_create() {
    let (mut app, mut rx) = test_app();
    app.add_draft(draft("one"));
    app.add_draft(draft("two"));
    assert_ne!(
        app.pending_comments[0].local_id,
        app.pending_comments[1].local_id
    );
    let ops = drain(&mut rx);
    assert_eq!(ops.len(), 2);
    assert!(matches!(&ops[0], SyncOp::Create { pr: 1, comment } if comment.body == "one"));
}

#[test]
fn created_links_draft_and_pushes_edits_made_in_flight() {
    let (mut app, mut rx) = test_app();
    app.add_draft(draft("old"));
    let id = app.pending_comments[0].local_id;
    app.edit_draft(0, "new".into()); // not linked yet: nothing queued
    drain(&mut rx);

    app.handle_sync_event(created(id, "n1", "old"));

    let d = &app.pending_comments[0];
    assert_eq!(d.node_id.as_deref(), Some("n1"));
    assert_eq!(d.synced_body.as_deref(), Some("old"));
    let ops = drain(&mut rx);
    assert!(matches!(&ops[..], [SyncOp::Update { node_id, body, .. }]
            if node_id == "n1" && body == "new"));
}

#[test]
fn created_for_a_deleted_draft_removes_the_remote_copy() {
    let (mut app, mut rx) = test_app();
    app.add_draft(draft("x"));
    let id = app.pending_comments[0].local_id;
    app.remove_draft(0); // unlinked: nothing to delete yet
    drain(&mut rx);

    app.handle_sync_event(created(id, "n1", "x"));

    assert!(app.tombstones.contains("n1"));
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Delete { node_id }] if node_id == "n1"));
}

#[test]
fn removing_a_linked_draft_queues_delete_and_tombstones_it() {
    let (mut app, mut rx) = test_app();
    app.add_draft(draft("x"));
    let id = app.pending_comments[0].local_id;
    app.handle_sync_event(created(id, "n1", "x"));
    drain(&mut rx);

    app.remove_draft(0);

    assert!(app.pending_comments.is_empty());
    assert!(app.tombstones.contains("n1"));
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Delete { node_id }] if node_id == "n1"));
}

#[test]
fn editing_a_linked_draft_queues_update() {
    let (mut app, mut rx) = test_app();
    app.pending_comments.push(linked_draft("a"));
    app.edit_draft(0, "b".into());
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Update { body, .. }] if body == "b"));
}

#[test]
fn pull_imports_drafts_from_github() {
    let (mut app, _rx) = test_app();
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![remote("n7", "from web")],
    });
    assert_eq!(app.pending_comments.len(), 1);
    let d = &app.pending_comments[0];
    assert_eq!(d.body, "from web");
    assert_eq!(d.node_id.as_deref(), Some("n7"));
    assert_eq!(d.synced_body.as_deref(), Some("from web"));
}

#[test]
fn pull_retries_create_for_unlinked_drafts() {
    let (mut app, mut rx) = test_app();
    app.add_draft(draft("x"));
    drain(&mut rx);
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![],
    });
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Create { .. }]));
}

#[test]
fn conflicting_pull_raises_prompt_and_blocks_submit() {
    let (mut app, mut rx) = test_app();
    app.pending_comments.push(linked_draft("mine"));
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![remote("n1", "theirs")],
    });
    assert_eq!(app.sync_conflicts.len(), 1);

    app.submit_review(ReviewEvent::Comment, String::new());
    assert!(drain(&mut rx).is_empty(), "submit must wait for resolution");
}

#[test]
fn keeping_github_version_adopts_its_text() {
    let (mut app, _rx) = test_app();
    app.pending_comments.push(linked_draft("mine"));
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![remote("n1", "theirs")],
    });
    app.resolve_conflict(false);
    assert_eq!(app.pending_comments[0].body, "theirs");
    assert_eq!(
        app.pending_comments[0].synced_body.as_deref(),
        Some("theirs")
    );
    assert!(app.sync_conflicts.is_empty());
}

#[test]
fn keeping_local_version_pushes_it() {
    let (mut app, mut rx) = test_app();
    app.pending_comments.push(linked_draft("mine"));
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![remote("n1", "theirs")],
    });
    drain(&mut rx);
    app.resolve_conflict(true);
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Update { body, .. }] if body == "mine"));
}

#[test]
fn accepting_remote_deletion_drops_the_draft() {
    let (mut app, _rx) = test_app();
    app.pending_comments.push(linked_draft("edited"));
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![],
    });
    assert_eq!(app.sync_conflicts.len(), 1);
    app.resolve_conflict(false);
    assert!(app.pending_comments.is_empty());
}

#[test]
fn recreating_after_remote_deletion_queues_create() {
    let (mut app, mut rx) = test_app();
    app.pending_comments.push(linked_draft("edited"));
    app.handle_sync_event(SyncEvent::Pulled {
        pr: 1,
        remote: vec![],
    });
    drain(&mut rx);
    app.resolve_conflict(true);
    assert!(app.pending_comments[0].node_id.is_none());
    assert!(matches!(&drain(&mut rx)[..], [SyncOp::Create { .. }]));
}

#[test]
fn submit_sends_only_unlinked_drafts() {
    let (mut app, mut rx) = test_app();
    app.pending_comments.push(ReviewComment {
        synced_body: Some("linked".into()),
        ..linked_draft("linked")
    });
    app.pending_comments.push(ReviewComment {
        local_id: 2,
        ..draft("unlinked")
    });
    app.submit_review(ReviewEvent::Approve, "lgtm".into());
    let ops = drain(&mut rx);
    assert!(matches!(&ops[..], [SyncOp::Submit { unlinked, body, .. }]
            if unlinked.len() == 1 && unlinked[0].local_id == 2 && body == "lgtm"));
}
