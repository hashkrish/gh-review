use tokio::sync::mpsc;

use super::*;
use crate::sync::SyncEvent;
use crate::types::FileStatus;

fn file(path: &str, viewed: bool) -> DiffFile {
    DiffFile {
        path: path.into(),
        status: FileStatus::Modified,
        additions: 1,
        deletions: 0,
        hunks: Vec::new(),
        viewed,
    }
}

fn states(pairs: &[(&str, bool)]) -> HashMap<String, bool> {
    pairs.iter().map(|(p, v)| (p.to_string(), *v)).collect()
}

/// An app with two files whose sync ops land in the returned receiver.
fn test_app() -> (App, mpsc::UnboundedReceiver<SyncOp>) {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 1, tx);
    let (sync_tx, sync_rx) = mpsc::unbounded_channel();
    app.sync_tx = sync_tx;
    app.files = vec![file("a.rs", false), file("b.rs", false)];
    app.file_picker.set_files(&app.files);
    app.focus = Focus::FilePicker;
    (app, sync_rx)
}

fn drain(rx: &mut mpsc::UnboundedReceiver<SyncOp>) -> Vec<SyncOp> {
    std::iter::from_fn(|| rx.try_recv().ok()).collect()
}

#[test]
fn apply_reports_only_changed_files() {
    let mut files = vec![file("a.rs", false), file("b.rs", true), file("c.rs", true)];
    let changed = apply_viewed_states(
        &mut files,
        &states(&[("a.rs", true), ("b.rs", true)]),
        &HashSet::new(),
    );
    // a.rs became viewed; b.rs unchanged; c.rs is absent so counts as unviewed.
    assert_eq!(changed, vec![(0, true), (2, false)]);
    assert!(files[0].viewed && files[1].viewed && !files[2].viewed);
}

#[test]
fn apply_skips_files_with_a_toggle_in_flight() {
    let mut files = vec![file("a.rs", true)];
    let in_flight: HashSet<String> = ["a.rs".to_string()].into();
    let changed = apply_viewed_states(&mut files, &states(&[("a.rs", false)]), &in_flight);
    assert!(changed.is_empty());
    assert!(files[0].viewed);
}

#[test]
fn toggle_marks_folds_and_pushes() {
    let (mut app, mut rx) = test_app();
    app.file_picker.selected = 1;

    app.toggle_file_viewed();

    assert!(app.files[1].viewed && !app.files[0].viewed);
    assert!(app.diff_view.collapsed_files.contains(&1));
    assert!(matches!(
        &drain(&mut rx)[..],
        [SyncOp::SetViewed { pr: 1, path, viewed: true }] if path == "b.rs"
    ));
}

#[test]
fn toggling_twice_unmarks_and_unfolds() {
    let (mut app, mut rx) = test_app();
    app.toggle_file_viewed();
    app.toggle_file_viewed();

    assert!(!app.files[0].viewed);
    assert!(!app.diff_view.collapsed_files.contains(&0));
    assert!(matches!(
        &drain(&mut rx)[..],
        [
            SyncOp::SetViewed { viewed: true, .. },
            SyncOp::SetViewed { viewed: false, .. }
        ]
    ));
}

#[test]
fn pull_adopts_github_state_and_folds() {
    let (mut app, _rx) = test_app();
    app.handle_sync_event(SyncEvent::ViewedPulled {
        pr: 1,
        states: states(&[("a.rs", true)]),
    });
    assert!(app.files[0].viewed);
    assert!(app.diff_view.collapsed_files.contains(&0));
    assert!(!app.diff_view.collapsed_files.contains(&1));
}

#[test]
fn pull_unfolds_files_unviewed_on_github() {
    let (mut app, _rx) = test_app();
    app.files[0].viewed = true;
    app.diff_view.collapsed_files.insert(0);
    app.handle_sync_event(SyncEvent::ViewedPulled {
        pr: 1,
        states: states(&[]),
    });
    assert!(!app.files[0].viewed);
    assert!(!app.diff_view.collapsed_files.contains(&0));
}

#[test]
fn pull_does_not_override_a_toggle_still_in_flight() {
    let (mut app, _rx) = test_app();
    app.toggle_file_viewed(); // a.rs viewed locally, push pending
    app.handle_sync_event(SyncEvent::ViewedPulled {
        pr: 1,
        states: states(&[("a.rs", false)]), // stale: fetched before the push
    });
    assert!(app.files[0].viewed);
}

#[test]
fn failed_push_reverts_the_toggle() {
    let (mut app, _rx) = test_app();
    app.toggle_file_viewed();
    app.handle_sync_event(SyncEvent::ViewedSetFailed {
        pr: 1,
        path: "a.rs".into(),
        viewed: true,
        error: "boom".into(),
    });
    assert!(!app.files[0].viewed);
    assert!(!app.diff_view.collapsed_files.contains(&0));
    assert!(app.viewed_inflight.is_empty());
}

#[test]
fn successful_push_clears_in_flight_tracking() {
    let (mut app, _rx) = test_app();
    app.toggle_file_viewed();
    assert_eq!(app.viewed_inflight.len(), 1);
    app.handle_sync_event(SyncEvent::ViewedSet {
        pr: 1,
        path: "a.rs".into(),
    });
    assert!(app.viewed_inflight.is_empty());
    assert!(app.files[0].viewed);
}
