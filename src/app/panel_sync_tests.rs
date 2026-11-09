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

fn press(app: &mut App, code: crossterm::event::KeyCode) {
    app.handle_key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    ));
}

fn command_for(
    app: &App,
    code: crossterm::event::KeyCode,
    mods: crossterm::event::KeyModifiers,
) -> Option<&'static str> {
    use super::keymap::LookupResult;
    let key = crossterm::event::KeyEvent::new(code, mods);
    match app
        .keymap
        .lookup(&key, Focus::DiffView, crate::types::RowContext::Code)
    {
        LookupResult::Command(cmd) => Some(cmd.name),
        _ => None,
    }
}

#[test]
fn space_pages_down_and_shift_space_pages_up() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let app = app_with_three_files();
    assert_eq!(
        command_for(&app, KeyCode::Char(' '), KeyModifiers::NONE),
        Some("full_page_down")
    );
    assert_eq!(
        command_for(&app, KeyCode::Char(' '), KeyModifiers::SHIFT),
        Some("full_page_up")
    );
}

#[test]
fn ctrl_f_and_ctrl_b_still_page() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let app = app_with_three_files();
    assert_eq!(
        command_for(&app, KeyCode::Char('f'), KeyModifiers::CONTROL),
        Some("full_page_down")
    );
    assert_eq!(
        command_for(&app, KeyCode::Char('b'), KeyModifiers::CONTROL),
        Some("full_page_up")
    );
}

#[test]
fn shift_is_still_ignored_for_shifted_characters() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let app = app_with_three_files();
    // `G` arrives as Char('G') with SHIFT on some terminals, none on others.
    assert_eq!(
        command_for(&app, KeyCode::Char('G'), KeyModifiers::SHIFT),
        command_for(&app, KeyCode::Char('G'), KeyModifiers::NONE)
    );
    assert_eq!(
        command_for(&app, KeyCode::Char('G'), KeyModifiers::NONE),
        Some("goto_last")
    );
}

#[test]
fn enter_in_the_file_list_moves_focus_to_the_diff() {
    let mut app = app_with_three_files();
    app.focus = Focus::FilePicker;
    app.file_picker.selected = 1;

    press(&mut app, crossterm::event::KeyCode::Enter);

    assert_eq!(app.focus, Focus::DiffView);
    assert_eq!(app.file_picker.selected, 1);
}

#[test]
fn enter_in_the_diff_does_not_change_focus() {
    let mut app = app_with_three_files();
    app.focus = Focus::DiffView;

    press(&mut app, crossterm::event::KeyCode::Enter);

    assert_eq!(app.focus, Focus::DiffView);
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
