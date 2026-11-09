use tokio::sync::mpsc;

use super::*;
use crate::diff::renderer::DisplayRow;
use crate::types::{DiffLine, FileStatus, Hunk, LineKind, ReviewComment, Side};

const SHA_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const SHA_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const SHA_C: &str = "cccccccccccccccccccccccccccccccccccccccc";

fn commit(sha: &str, headline: &str) -> Commit {
    Commit {
        sha: sha.to_string(),
        headline: headline.to_string(),
        author: "alice".to_string(),
        date: "2026-11-09T07:00:00Z".to_string(),
    }
}

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

/// A file with one added line (new line 1), so a comment on it renders.
fn file_with_line(path: &str) -> DiffFile {
    DiffFile {
        hunks: vec![Hunk {
            header: "@@ -0,0 +1 @@".into(),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: 1,
            lines: vec![DiffLine {
                kind: LineKind::Added,
                old_lineno: None,
                new_lineno: Some(1),
                content: "let x = 1;".into(),
                highlighted_content: None,
            }],
        }],
        ..file(path, false)
    }
}

/// An app showing the full PR diff of two files, with three PR commits loaded.
fn app() -> App {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 7, tx);
    app.files = vec![file("a.rs", false), file("b.rs", false)];
    app.file_picker.set_files(&app.files);
    app.rebuild_display();
    app.commits_panel.set_commits(vec![
        commit(SHA_A, "first"),
        commit(SHA_B, "second"),
        commit(SHA_C, "third"),
    ]);
    app
}

/// Select a commit by its position in the PR (0 = oldest).
fn select(app: &mut App, index: usize) {
    app.commits_panel.select_row(index + 1);
}

/// Pretend the network answered for the pending view.
fn load_pending(app: &mut App, files: Vec<DiffFile>) {
    let source = app.pending_source.clone().expect("a view was requested");
    app.on_alt_diff_loaded(app.pr_number, source, files);
}

// ── Toggling and the drawer ────────────────────────────────────────────

#[test]
fn toggle_opens_the_panel_and_focuses_it() {
    let mut app = app();
    app.toggle_commits_panel();
    assert!(app.commits_panel.visible);
    assert_eq!(app.focus, Focus::Commits);
    assert_eq!(app.right_drawer(), Some(Focus::Commits));
}

#[test]
fn toggle_again_closes_it_and_returns_focus_to_the_diff() {
    let mut app = app();
    app.toggle_commits_panel();
    app.toggle_commits_panel();
    assert!(!app.commits_panel.visible);
    assert_eq!(app.focus, Focus::DiffView);
    assert_eq!(app.right_drawer(), None);
}

#[test]
fn opening_commits_closes_the_description_drawer() {
    let mut app = app();
    app.description_panel.visible = true;
    app.toggle_commits_panel();
    assert!(!app.description_panel.visible);
    assert_eq!(app.right_drawer(), Some(Focus::Commits));
}

#[test]
fn opening_the_description_closes_the_commits_drawer() {
    let mut app = app();
    app.toggle_commits_panel();
    crate::app::command_handlers::description(&mut app);
    assert!(!app.commits_panel.visible);
    assert!(app.description_panel.visible);
    assert_eq!(app.focus, Focus::Description);
}

#[test]
fn commits_load_into_the_panel_and_stale_loads_are_ignored() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 7, tx);
    app.on_commits_loaded(99, Ok(vec![commit(SHA_A, "other PR")]));
    assert!(!app.commits_panel.is_loaded(), "another PR's commits");
    app.on_commits_loaded(7, Ok(vec![commit(SHA_A, "mine")]));
    assert!(app.commits_panel.is_loaded());
}

#[test]
fn a_failed_load_is_reported() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 7, tx);
    app.on_commits_loaded(7, Err("boom".into()));
    assert!(!app.commits_panel.is_loaded());
}

// ── Viewing a commit ───────────────────────────────────────────────────

#[test]
fn viewing_a_commit_requests_it_without_changing_the_diff_yet() {
    let mut app = app();
    select(&mut app, 1);
    app.view_selected_commit();
    assert_eq!(app.pending_source, Some(DiffSource::Commit(SHA_B.into())));
    assert!(app.diff_source.is_full());
    assert_eq!(app.files.len(), 2);
}

#[test]
fn a_loaded_commit_replaces_the_diff_and_sets_the_full_files_aside() {
    let mut app = app();
    select(&mut app, 1);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("only.rs", false)]);

    assert_eq!(app.diff_source, DiffSource::Commit(SHA_B.into()));
    assert_eq!(app.files.len(), 1);
    assert_eq!(app.files[0].path, "only.rs");
    assert_eq!(app.full_files.len(), 2, "the PR's files are kept");
    assert_eq!(app.file_picker.files.len(), 1);
    assert_eq!(app.pending_source, None);
}

#[test]
fn switching_between_commits_keeps_the_original_full_files() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("one.rs", false)]);
    select(&mut app, 2);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("two.rs", false)]);

    assert_eq!(app.files[0].path, "two.rs");
    assert_eq!(app.full_files.len(), 2);
    assert_eq!(
        app.full_files[0].path, "a.rs",
        "not the first commit's files"
    );
}

#[test]
fn a_response_for_a_view_that_was_replaced_is_ignored() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    select(&mut app, 1);
    app.view_selected_commit(); // replaces the pending view
    app.on_alt_diff_loaded(
        7,
        DiffSource::Commit(SHA_A.into()),
        vec![file("late.rs", false)],
    );
    assert!(app.diff_source.is_full(), "the late response was dropped");
    assert_eq!(app.pending_source, Some(DiffSource::Commit(SHA_B.into())));
}

#[test]
fn a_response_for_another_pr_is_ignored() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    app.on_alt_diff_loaded(
        99,
        DiffSource::Commit(SHA_A.into()),
        vec![file("x.rs", false)],
    );
    assert!(app.diff_source.is_full());
}

#[test]
fn viewing_the_commit_already_shown_does_nothing() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("one.rs", false)]);
    app.view_selected_commit();
    assert_eq!(app.pending_source, None);
}

// ── Since a commit ─────────────────────────────────────────────────────

#[test]
fn since_requests_everything_after_the_selected_commit() {
    let mut app = app();
    select(&mut app, 0);
    app.view_since_selected();
    assert_eq!(app.pending_source, Some(DiffSource::Since(SHA_A.into())));
}

#[test]
fn since_the_newest_commit_has_nothing_to_show() {
    let mut app = app();
    select(&mut app, 2);
    app.view_since_selected();
    assert_eq!(app.pending_source, None);
}

#[test]
fn since_needs_a_commit_not_the_full_diff_row() {
    let mut app = app();
    app.commits_panel.select_row(0);
    app.view_since_selected();
    assert_eq!(app.pending_source, None);
}

// ── Returning to the full diff ─────────────────────────────────────────

#[test]
fn the_full_diff_row_restores_the_pr_files() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("one.rs", false)]);

    app.commits_panel.select_row(0);
    app.view_selected_commit();

    assert!(app.diff_source.is_full());
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.files[0].path, "a.rs");
    assert!(app.full_files.is_empty());
    assert_eq!(app.file_picker.files.len(), 2);
}

#[test]
fn returning_refolds_files_marked_viewed() {
    let mut app = app();
    app.files[1].viewed = true;
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("one.rs", false)]);
    assert!(app.diff_view.collapsed_files.is_empty());

    app.commits_panel.select_row(0);
    app.view_selected_commit();

    assert!(app.diff_view.collapsed_files.contains(&1));
    assert!(!app.diff_view.collapsed_files.contains(&0));
}

#[test]
fn the_full_diff_row_cancels_a_view_that_is_still_loading() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    app.commits_panel.select_row(0);
    app.view_selected_commit();
    assert_eq!(app.pending_source, None);
    // The response arriving afterwards must not take effect.
    app.on_alt_diff_loaded(
        7,
        DiffSource::Commit(SHA_A.into()),
        vec![file("late.rs", false)],
    );
    assert!(app.diff_source.is_full());
}

#[test]
fn leave_commit_view_is_a_no_op_on_the_full_diff() {
    let mut app = app();
    app.leave_commit_view();
    assert_eq!(app.files.len(), 2);
}

// ── Read-only view ─────────────────────────────────────────────────────

#[test]
fn the_guard_allows_everything_on_the_full_diff() {
    let mut app = app();
    assert!(!app.read_only_guard());
}

#[test]
fn the_guard_blocks_while_viewing_a_commit() {
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("one.rs", false)]);
    assert!(app.read_only_guard());
}

#[test]
fn commenting_commands_refuse_in_a_commit_view() {
    use crate::app::command_handlers as h;
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file_with_line("one.rs")]);
    app.diff_view.cursor = 1; // the added line

    h::comment_on_line(&mut app);
    h::suggest(&mut app);
    h::visual(&mut app);
    h::toggle_viewed(&mut app);

    assert!(!app.comment_input.visible, "no comment box opened");
    assert!(!app.diff_view.is_visual_mode());
    assert!(!app.files[0].viewed, "viewed mark untouched");
    assert!(app.pending_comments.is_empty());
}

#[test]
fn review_comments_are_hidden_in_a_commit_view() {
    let mut app = app();
    app.files = vec![file_with_line("a.rs")];
    app.pending_comments.push(ReviewComment::new_local(
        "a.rs".into(),
        1,
        Side::Right,
        "needs work".into(),
        None,
        None,
    ));
    app.rebuild_display();
    let full_rows = app.diff_view.display_rows.len();

    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file_with_line("a.rs")]);
    let commit_rows = app.diff_view.display_rows.len();

    assert!(commit_rows < full_rows, "{commit_rows} vs {full_rows}");
    assert!(
        !app.diff_view
            .display_rows
            .iter()
            .any(|r| matches!(r, DisplayRow::CommentHeader { .. })),
        "no comment rows"
    );
    // Back on the full diff they return.
    app.commits_panel.select_row(0);
    app.view_selected_commit();
    assert_eq!(app.diff_view.display_rows.len(), full_rows);
}

#[test]
fn viewed_state_pulled_during_a_commit_view_updates_the_pr_files() {
    use crate::sync::SyncEvent;
    let mut app = app();
    select(&mut app, 0);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("a.rs", false)]); // same path as a PR file

    app.handle_sync_event(SyncEvent::ViewedPulled {
        pr: 7,
        states: [("a.rs".to_string(), true)].into(),
    });

    assert!(!app.files[0].viewed, "the commit's file list is untouched");
    assert!(app.diff_view.collapsed_files.is_empty(), "no stray fold");
    assert!(app.full_files[0].viewed, "the PR's own file got the state");
}

// ── Opening and copying ────────────────────────────────────────────────

#[test]
fn commit_urls_point_at_the_commit_page() {
    let app = app();
    assert_eq!(
        app.commit_url(SHA_A),
        format!("https://github.com/o/r/commit/{SHA_A}")
    );
}

#[test]
fn open_and_copy_need_a_selected_commit() {
    let mut app = app();
    app.commits_panel.select_row(0);
    // Neither may act (or panic) on the full-diff row.
    app.open_selected_commit();
    app.copy_selected_sha();
    assert!(app.diff_source.is_full());
}

// ── Keys ───────────────────────────────────────────────────────────────

fn press(app: &mut App, code: crossterm::event::KeyCode) {
    app.handle_key(crossterm::event::KeyEvent::new(
        code,
        crossterm::event::KeyModifiers::NONE,
    ));
}

#[test]
fn p_toggles_the_panel_from_the_diff() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.focus = Focus::DiffView;
    press(&mut app, KeyCode::Char('p'));
    assert!(app.commits_panel.visible);
    press(&mut app, KeyCode::Char('p'));
    assert!(!app.commits_panel.visible);
}

#[test]
fn j_k_move_the_selection_and_enter_views_the_commit() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_B);
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_A);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.pending_source, Some(DiffSource::Commit(SHA_A.into())));
}

#[test]
fn s_in_the_panel_requests_changes_since() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.pending_source, Some(DiffSource::Since(SHA_A.into())));
}

#[test]
fn q_and_esc_close_the_panel_instead_of_quitting() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.commits_panel.visible);
    assert!(!app.should_quit());

    app.toggle_commits_panel();
    press(&mut app, KeyCode::Esc);
    assert!(!app.commits_panel.visible);
    assert!(!app.should_quit());
}

#[test]
fn keys_scoped_to_the_diff_do_not_leak_into_the_panel() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    // `c` comments in the diff; in the commits panel it must do nothing.
    press(&mut app, KeyCode::Char('c'));
    assert!(!app.comment_input.visible);
}

#[test]
fn d_switches_from_commits_to_the_description() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('d'));
    assert!(app.description_panel.visible);
    assert!(!app.commits_panel.visible);
}

#[test]
fn tab_cycles_through_the_commits_drawer() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.focus = Focus::DiffView;
    app.commits_panel.visible = true;
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Commits);
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::FilePicker);
}

fn press_with(
    app: &mut App,
    code: crossterm::event::KeyCode,
    mods: crossterm::event::KeyModifiers,
) {
    app.handle_key(crossterm::event::KeyEvent::new(code, mods));
}

/// An app with the panel open on 60 commits, drawn once so the panel knows
/// how many rows fit on screen.
fn app_with_many_commits() -> App {
    let mut app = app();
    app.commits_panel.set_commits(
        (0..60)
            .map(|i| commit(&format!("{i:040x}"), &format!("change {i}")))
            .collect(),
    );
    app.toggle_commits_panel();
    screen(&mut app, 130, 24);
    app
}

#[test]
fn ctrl_d_and_ctrl_u_page_by_half_a_screen() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = app_with_many_commits();

    press_with(&mut app, KeyCode::Char('f'), KeyModifiers::CONTROL);
    let full = app.commits_panel.selected_row();
    assert!(full > 1, "a full page moves several rows, got {full}");
    press_with(&mut app, KeyCode::Char('b'), KeyModifiers::CONTROL);
    assert_eq!(app.commits_panel.selected_row(), 0);

    press_with(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    let half = app.commits_panel.selected_row();
    assert!(half > 0, "Ctrl-D must move the selection down");
    assert_eq!(half, full / 2, "half a page");

    press_with(&mut app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(app.commits_panel.selected_row(), 0, "Ctrl-U moves back up");
}

#[test]
fn space_and_shift_space_page_the_commits_too() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = app_with_many_commits();

    press_with(&mut app, KeyCode::Char(' '), KeyModifiers::NONE);
    let down = app.commits_panel.selected_row();
    assert!(down > 1, "Space pages down, got {down}");
    press_with(&mut app, KeyCode::Char(' '), KeyModifiers::SHIFT);
    assert_eq!(app.commits_panel.selected_row(), 0, "Shift+Space pages up");
}

#[test]
fn the_page_keys_leave_the_diff_alone() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let mut app = app_with_many_commits();
    let cursor = app.diff_view.cursor;
    press_with(&mut app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(app.diff_view.cursor, cursor, "only the focused panel moves");
}

#[test]
fn gg_jumps_to_the_first_row_and_capital_g_to_the_last() {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_C);

    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('g'));
    assert!(
        app.commits_panel.selected_commit().is_none(),
        "back on the All changes row"
    );

    // Some terminals report capital G as Char('G') plus SHIFT.
    app.handle_key(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_C);
}

#[test]
fn a_lone_g_waits_for_the_second_key_instead_of_acting() {
    use crossterm::event::KeyCode;
    let mut app = app();
    app.toggle_commits_panel();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(
        app.commits_panel.selected_commit().unwrap().sha,
        SHA_B,
        "the first g alone moves nothing"
    );
    // A second key that isn't a commits sequence cancels it harmlessly.
    press(&mut app, KeyCode::Char('c'));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_B);
    assert!(!app.comment_input.visible);
    // ...and the next key works normally again.
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.commits_panel.selected_commit().unwrap().sha, SHA_A);
}

// ── Whole-screen rendering ─────────────────────────────────────────────

/// Render the full UI into an in-memory terminal and return it as text.
fn screen(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let buf = terminal.backend().buffer().clone();
    (0..buf.area.height)
        .map(|y| {
            (0..buf.area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn pr_meta() -> crate::types::PrMetadata {
    use crate::types::{PrMetadata, PrRef, PrUser};
    let git_ref = |name: &str| PrRef {
        sha: SHA_C.into(),
        ref_name: name.into(),
    };
    PrMetadata {
        number: 7,
        title: "Add things".into(),
        body: None,
        state: "open".into(),
        draft: false,
        review_decision: None,
        head: git_ref("feature"),
        base: git_ref("main"),
        user: PrUser {
            login: "alice".into(),
        },
        additions: Some(3),
        deletions: Some(1),
        changed_files: Some(2),
        reviewers: Vec::new(),
    }
}

#[test]
fn the_open_panel_shares_the_screen_with_the_diff_and_file_list() {
    let mut app = app();
    app.toggle_commits_panel();
    let out = screen(&mut app, 130, 24);
    assert!(out.contains("Files (1/2)"), "file list still drawn:\n{out}");
    assert!(out.contains("Commits (3)"), "commits drawer drawn:\n{out}");
    assert!(out.contains("▸ All changes"), "{out}");
    assert!(out.contains("aaaaaaa first"), "{out}");
    assert!(out.contains("ccccccc third"), "{out}");
    assert!(
        !out.contains(" Description "),
        "description drawer hidden:\n{out}"
    );
}

#[test]
fn a_closed_panel_leaves_no_drawer() {
    let mut app = app();
    let out = screen(&mut app, 130, 24);
    assert!(!out.contains("Commits ("), "{out}");
}

#[test]
fn the_title_bar_and_panel_show_which_view_is_active() {
    let mut app = app();
    app.pr_meta = Some(pr_meta());
    app.toggle_commits_panel();
    select(&mut app, 1);
    app.view_selected_commit();
    load_pending(&mut app, vec![file("only.rs", false)]);

    let out = screen(&mut app, 130, 24);
    assert!(
        out.contains("[commit bbbbbbb · read-only]"),
        "title bar:\n{out}"
    );
    assert!(out.contains("viewing bbbbbbb"), "panel title:\n{out}");
    assert!(out.contains("● bbbbbbb second"), "{out}");
    assert!(
        out.contains("Files (1/1)"),
        "file list shows the commit's files:\n{out}"
    );
}
