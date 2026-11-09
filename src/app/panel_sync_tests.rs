//! The file list's selection follows the diff cursor.

use tokio::sync::mpsc;

use super::{App, Focus};
use crate::diff::renderer::DisplayRow;
use crate::types::{DiffFile, FileStatus};

fn file(path: &str) -> DiffFile {
    DiffFile {
        path: path.into(),
        status: FileStatus::Modified,
        additions: 1,
        deletions: 0,
        hunks: Vec::new(),
        viewed: false,
    }
}

/// An app with three files whose diff view has one header row per file.
fn app_with_three_files() -> App {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 1, tx);
    app.files = vec![file("a.rs"), file("b.rs"), file("c.rs")];
    app.file_picker.set_files(&app.files);
    app.diff_view.display_rows = (0..3)
        .map(|i| DisplayRow::FileHeader {
            path: format!("{i}.rs"),
            file_idx: i,
            collapsed: false,
            viewed: false,
        })
        .collect();
    app
}

#[test]
fn selection_follows_the_diff_cursor() {
    let mut app = app_with_three_files();
    app.focus = Focus::DiffView;

    app.diff_view.cursor = 2;
    app.sync_file_picker_to_cursor();
    assert_eq!(app.file_picker.selected, 2);

    app.diff_view.cursor = 0;
    app.sync_file_picker_to_cursor();
    assert_eq!(app.file_picker.selected, 0);
}

#[test]
fn selection_is_left_alone_while_the_list_has_focus() {
    let mut app = app_with_three_files();
    app.focus = Focus::FilePicker;
    app.file_picker.selected = 1;
    app.diff_view.cursor = 2;

    app.sync_file_picker_to_cursor();

    assert_eq!(app.file_picker.selected, 1);
}

#[test]
fn selection_is_left_alone_while_filtering() {
    let mut app = app_with_three_files();
    app.focus = Focus::DiffView;
    app.file_picker.start_filter(); // resets the selection
    app.file_picker.selected = 1;
    app.diff_view.cursor = 2;

    app.sync_file_picker_to_cursor();

    assert_eq!(app.file_picker.selected, 1);
}
