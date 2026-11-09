use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc;

use super::*;
use crate::components::comments_panel::Filter;
use crate::types::{
    DiffFile, DiffLine, DiffSource, ExistingComment, FileStatus, Hunk, LineKind, PrUser,
    ReviewComment, Side, ThreadInfo,
};

fn comment(id: u64, reply_to: Option<u64>, path: &str, line: usize, body: &str) -> ExistingComment {
    ExistingComment {
        id,
        path: path.to_string(),
        line: Some(line),
        side: Some("RIGHT".to_string()),
        start_line: None,
        body: body.to_string(),
        user: PrUser {
            login: format!("user{id}"),
        },
        created_at: "2026-11-09T07:00:00Z".to_string(),
        in_reply_to_id: reply_to,
        original_line: None,
        subject_type: None,
    }
}

/// A file whose diff has `n` added lines, numbered 1..=n.
fn file(path: &str, n: usize) -> DiffFile {
    DiffFile {
        path: path.into(),
        status: FileStatus::Modified,
        additions: n,
        deletions: 0,
        hunks: vec![Hunk {
            header: format!("@@ -0,0 +1,{n} @@"),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: n,
            lines: (1..=n)
                .map(|i| DiffLine {
                    kind: LineKind::Added,
                    old_lineno: None,
                    new_lineno: Some(i),
                    content: format!("line {i}"),
                    highlighted_content: None,
                })
                .collect(),
        }],
        viewed: false,
    }
}

/// An app with a.rs (3 lines) and b.rs (3 lines), an open thread on a.rs:2
/// (root 5, one reply), a resolved thread on b.rs:1 (root 8), and a draft on
/// a.rs:3.
fn app() -> App {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 7, tx);
    app.files = vec![file("a.rs", 3), file("b.rs", 3)];
    app.file_picker.set_files(&app.files);
    app.existing_comments = vec![
        comment(5, None, "a.rs", 2, "open question"),
        comment(6, Some(5), "a.rs", 2, "a reply"),
        comment(8, None, "b.rs", 1, "done"),
    ];
    app.thread_map.insert(
        5,
        ThreadInfo {
            thread_node_id: "PRRT_5".into(),
            is_resolved: false,
        },
    );
    app.thread_map.insert(
        8,
        ThreadInfo {
            thread_node_id: "PRRT_8".into(),
            is_resolved: true,
        },
    );
    app.pending_comments.push(ReviewComment::new_local(
        "a.rs".into(),
        3,
        Side::Right,
        "my draft".into(),
        None,
        None,
    ));
    app.rebuild_display();
    app
}

/// Select the entry matching `f` and return it.
fn select(app: &mut App, f: impl Fn(&CommentEntry) -> bool) {
    app.comments_panel.goto_first();
    let mut guard = 0;
    while app.comments_panel.selected_entry().map(&f) != Some(true) {
        app.comments_panel.next();
        guard += 1;
        assert!(guard < 50, "no such entry");
    }
}

fn thread(root: u64) -> impl Fn(&CommentEntry) -> bool {
    move |e| e.root_id == Some(root)
}

fn draft(e: &CommentEntry) -> bool {
    e.kind == EntryKind::Draft
}

fn press(app: &mut App, code: KeyCode) {
    app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
}

// ── The list follows the data ──────────────────────────────────────────

#[test]
fn the_panel_lists_threads_and_drafts_in_diff_order() {
    let app = app();
    assert_eq!(app.comments_panel.len(), 3);
}

#[test]
fn rebuilding_the_display_refreshes_the_list() {
    let mut app = app();
    app.existing_comments
        .push(comment(20, None, "b.rs", 3, "new on the server"));
    app.rebuild_display();
    assert_eq!(app.comments_panel.len(), 4);

    app.pending_comments.clear();
    app.rebuild_display();
    assert_eq!(app.comments_panel.len(), 3);
}

#[test]
fn the_list_still_shows_comments_while_a_commit_view_hides_them_in_the_diff() {
    let mut app = app();
    app.diff_source = DiffSource::Commit("abc".into());
    app.rebuild_display();
    assert_eq!(app.comments_panel.len(), 3);
}

// ── Toggling and the drawer ────────────────────────────────────────────

#[test]
fn i_toggles_the_panel() {
    let mut app = app();
    app.focus = Focus::DiffView;
    press(&mut app, KeyCode::Char('i'));
    assert!(app.comments_panel.visible);
    assert_eq!(app.focus, Focus::Comments);
    assert_eq!(app.right_drawer(), Some(Focus::Comments));
    press(&mut app, KeyCode::Char('i'));
    assert!(!app.comments_panel.visible);
    assert_eq!(app.focus, Focus::DiffView);
}

#[test]
fn q_and_esc_close_the_panel_instead_of_quitting() {
    let mut app = app();
    app.toggle_comments_panel();
    press(&mut app, KeyCode::Char('q'));
    assert!(!app.comments_panel.visible && !app.should_quit());
    app.toggle_comments_panel();
    press(&mut app, KeyCode::Esc);
    assert!(!app.comments_panel.visible && !app.should_quit());
}

#[test]
fn the_three_drawers_are_mutually_exclusive() {
    let mut app = app();
    app.toggle_comments_panel();

    press(&mut app, KeyCode::Char('p')); // commits
    assert!(app.commits_panel.visible && !app.comments_panel.visible);

    press(&mut app, KeyCode::Char('i')); // back to comments
    assert!(app.comments_panel.visible && !app.commits_panel.visible);

    press(&mut app, KeyCode::Char('d')); // description
    assert!(app.description_panel.visible && !app.comments_panel.visible);

    press(&mut app, KeyCode::Char('i'));
    assert!(app.comments_panel.visible && !app.description_panel.visible);
}

#[test]
fn tab_cycles_through_the_comments_drawer() {
    let mut app = app();
    app.focus = Focus::DiffView;
    app.comments_panel.visible = true;
    press(&mut app, KeyCode::Tab);
    assert_eq!(app.focus, Focus::Comments);
}

// ── Jumping to a comment ───────────────────────────────────────────────

#[test]
fn enter_jumps_to_a_thread_in_the_diff() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, thread(5));
    press(&mut app, KeyCode::Enter);

    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(
        matches!(
            row,
            DisplayRow::CommentHeader {
                thread_root_id: Some(5),
                is_reply: false,
                ..
            }
        ),
        "cursor should be on thread 5's header, got {row:?}"
    );
    assert!(app.comments_panel.visible, "the panel stays open");
}

#[test]
fn enter_jumps_to_a_draft_in_the_diff() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, draft);
    press(&mut app, KeyCode::Enter);

    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(
        matches!(
            row,
            DisplayRow::CommentHeader {
                is_pending: true,
                pending_idx: Some(0),
                ..
            }
        ),
        "cursor should be on the draft's header, got {row:?}"
    );
}

#[test]
fn jumping_into_a_folded_file_unfolds_it() {
    let mut app = app();
    app.diff_view.collapsed_files.insert(1); // b.rs
    app.rebuild_display();
    app.toggle_comments_panel();
    select(&mut app, thread(8));

    press(&mut app, KeyCode::Enter);

    assert!(!app.diff_view.collapsed_files.contains(&1));
    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(matches!(
        row,
        DisplayRow::CommentHeader {
            thread_root_id: Some(8),
            ..
        }
    ));
}

#[test]
fn jumping_from_a_commit_view_returns_to_the_full_diff_first() {
    let mut app = app();
    app.full_files = std::mem::take(&mut app.files);
    app.files = vec![file("only.rs", 1)];
    app.diff_source = DiffSource::Commit("abc".into());
    app.rebuild_display();
    app.toggle_comments_panel();
    select(&mut app, thread(5));

    press(&mut app, KeyCode::Enter);

    assert!(app.diff_source.is_full(), "left the commit view");
    assert_eq!(app.files.len(), 2);
    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(matches!(
        row,
        DisplayRow::CommentHeader {
            thread_root_id: Some(5),
            ..
        }
    ));
}

#[test]
fn a_comment_on_a_line_the_diff_does_not_show_is_still_reachable() {
    let mut app = app();
    // Line 99 isn't in a.rs's diff. Such a comment is shown at the end of
    // its file rather than dropped, so it can be jumped to.
    app.existing_comments
        .push(comment(30, None, "a.rs", 99, "outside the hunks"));
    app.rebuild_display();
    app.toggle_comments_panel();
    select(&mut app, thread(30));

    press(&mut app, KeyCode::Enter);

    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(
        matches!(
            row,
            DisplayRow::CommentHeader {
                thread_root_id: Some(30),
                ..
            }
        ),
        "got {row:?}"
    );
}

#[test]
fn an_outdated_comment_can_be_jumped_to_too() {
    let mut app = app();
    let mut outdated = comment(31, None, "b.rs", 1, "the code has moved on");
    outdated.line = None;
    outdated.original_line = Some(12);
    app.existing_comments.push(outdated);
    app.rebuild_display();
    app.toggle_comments_panel();
    select(&mut app, thread(31));

    press(&mut app, KeyCode::Enter);

    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(
        matches!(
            row,
            DisplayRow::CommentHeader {
                thread_root_id: Some(31),
                ..
            }
        ),
        "got {row:?}"
    );
}

#[test]
fn jumping_with_nothing_selected_does_nothing() {
    let (tx, _rx) = mpsc::unbounded_channel();
    let mut app = App::new("o/r".into(), 7, tx);
    app.toggle_comments_panel();
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::Comments);
}

// ── Replying ───────────────────────────────────────────────────────────

#[test]
fn r_opens_a_reply_to_the_selected_thread() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, thread(5));
    press(&mut app, KeyCode::Char('r'));
    assert!(app.comment_input.visible);
    assert_eq!(app.comment_input.reply_to_id, Some(5));
    assert_eq!(app.comment_input.reply_author, "user5");
}

#[test]
fn drafts_cannot_be_replied_to() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, draft);
    press(&mut app, KeyCode::Char('r'));
    assert!(!app.comment_input.visible);
}

// ── Resolving ──────────────────────────────────────────────────────────

fn entry(kind: EntryKind, node: Option<&str>, resolved: bool) -> CommentEntry {
    CommentEntry {
        anchor: Anchor::Line(1),
        kind,
        root_id: (kind == EntryKind::Thread).then_some(5),
        thread_node_id: node.map(String::from),
        draft_idx: (kind == EntryKind::Draft).then_some(0),
        path: "a.rs".into(),
        line: Some(1),
        side: Side::Right,
        author: "alice".into(),
        body: "x".into(),
        reply_count: 0,
        is_resolved: resolved,
    }
}

#[test]
fn resolve_targets_the_thread_and_carries_its_state() {
    let open = entry(EntryKind::Thread, Some("PRRT_1"), false);
    let done = entry(EntryKind::Thread, Some("PRRT_1"), true);
    assert_eq!(resolve_target(&open), Ok(("PRRT_1".to_string(), false)));
    assert_eq!(resolve_target(&done), Ok(("PRRT_1".to_string(), true)));
}

#[test]
fn resolve_refuses_drafts_and_threads_without_an_id() {
    assert!(resolve_target(&entry(EntryKind::Draft, None, false)).is_err());
    assert!(resolve_target(&entry(EntryKind::Thread, None, false)).is_err());
}

#[test]
fn resolving_a_draft_does_nothing() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, draft);
    press(&mut app, KeyCode::Char('R')); // must not try to reach GitHub
    assert_eq!(app.pending_comments.len(), 1);
}

// ── Editing and discarding drafts ──────────────────────────────────────

#[test]
fn e_edits_the_selected_draft() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, draft);
    press(&mut app, KeyCode::Char('e'));
    assert!(app.comment_input.visible);
    assert_eq!(app.comment_input.editing_pending_idx, Some(0));
    assert_eq!(app.comment_input.file_path, "a.rs");
    assert_eq!(app.comment_input.line, 3);
}

#[test]
fn threads_cannot_be_edited() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, thread(5));
    press(&mut app, KeyCode::Char('e'));
    assert!(!app.comment_input.visible);
}

#[test]
fn x_discards_the_selected_draft_and_the_list_follows() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, draft);
    press(&mut app, KeyCode::Char('x'));

    assert!(app.pending_comments.is_empty());
    assert_eq!(app.comments_panel.len(), 2, "only the threads remain");
    assert!(app.comments_panel.selected_entry().is_some());
}

#[test]
fn x_leaves_published_threads_alone() {
    let mut app = app();
    app.toggle_comments_panel();
    select(&mut app, thread(5));
    press(&mut app, KeyCode::Char('x'));
    assert_eq!(app.comments_panel.len(), 3);
    assert_eq!(app.pending_comments.len(), 1);
}

// ── Conversation comments (not attached to any file) ───────────────────

fn conversation_entry() -> CommentEntry {
    CommentEntry {
        anchor: Anchor::Conversation,
        thread_node_id: None,
        ..entry(EntryKind::Thread, None, false)
    }
}

#[test]
fn conversation_comments_cannot_be_resolved_or_replied_to_from_the_panel() {
    let e = conversation_entry();
    assert!(resolve_target(&e).is_err());
    assert!(reply_target(&e).is_err());
}

#[test]
fn conversation_links_use_the_issue_comment_anchor() {
    assert_eq!(
        comment_url("o/r", 7, &conversation_entry()).as_deref(),
        Some("https://github.com/o/r/pull/7#issuecomment-5")
    );
}

#[test]
fn a_conversation_comment_is_listed_last_and_can_be_jumped_to() {
    let mut app = app();
    let mut chat = comment(40, None, "", 1, "general discussion");
    chat.line = None;
    chat.subject_type = None;
    app.existing_comments.push(chat);
    app.rebuild_display();
    app.toggle_comments_panel();

    app.comments_panel.goto_last();
    assert_eq!(
        app.comments_panel.selected_entry().unwrap().anchor,
        Anchor::Conversation,
        "listed after everything else, though its empty path sorts first"
    );

    press(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, Focus::DiffView);
    let row = &app.diff_view.display_rows[app.diff_view.cursor];
    assert!(
        matches!(
            row,
            DisplayRow::CommentHeader {
                thread_root_id: Some(40),
                ..
            }
        ),
        "got {row:?}"
    );
}

#[test]
fn replying_to_a_conversation_comment_does_not_open_the_reply_box() {
    let mut app = app();
    let mut chat = comment(40, None, "", 1, "general discussion");
    chat.line = None;
    app.existing_comments.push(chat);
    app.rebuild_display();
    app.toggle_comments_panel();
    app.comments_panel.goto_last();
    press(&mut app, KeyCode::Char('r'));
    assert!(!app.comment_input.visible);
}

#[test]
fn conversation_comments_only_appear_under_the_all_filter() {
    let mut app = app();
    let mut chat = comment(40, None, "", 1, "general discussion");
    chat.line = None;
    app.existing_comments.push(chat);
    app.rebuild_display();
    app.toggle_comments_panel();
    assert_eq!(app.comments_panel.len(), 4);
    press(&mut app, KeyCode::Char('f')); // unresolved
    assert_eq!(app.comments_panel.len(), 2, "thread 5 and the draft only");
}

// ── Opening and copying ────────────────────────────────────────────────

#[test]
fn thread_links_point_at_the_discussion() {
    let e = entry(EntryKind::Thread, None, false);
    assert_eq!(
        comment_url("o/r", 7, &e).as_deref(),
        Some("https://github.com/o/r/pull/7#discussion_r5")
    );
}

#[test]
fn drafts_have_no_link() {
    assert_eq!(
        comment_url("o/r", 7, &entry(EntryKind::Draft, None, false)),
        None
    );
}

// ── Keys inside the panel ──────────────────────────────────────────────

#[test]
fn f_cycles_the_filter() {
    let mut app = app();
    app.toggle_comments_panel();
    press(&mut app, KeyCode::Char('f'));
    assert_eq!(app.comments_panel.filter(), Filter::Unresolved);
    assert_eq!(app.comments_panel.len(), 2, "thread 5 and the draft");
    press(&mut app, KeyCode::Char('f'));
    assert_eq!(app.comments_panel.filter(), Filter::Resolved);
    assert_eq!(app.comments_panel.len(), 1);
}

#[test]
fn navigation_keys_move_the_selection() {
    let mut app = app();
    app.toggle_comments_panel();
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.comments_panel.selected_row(), 2);
    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.comments_panel.selected_row(), 1);
    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.comments_panel.selected_row(), 2);
    press(&mut app, KeyCode::Char('g'));
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.comments_panel.selected_row(), 0);
    app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL));
    assert!(app.comments_panel.selected_row() > 0, "Ctrl-D pages down");
}

#[test]
fn diff_only_keys_do_not_leak_into_the_panel() {
    let mut app = app();
    app.toggle_comments_panel();
    for key in ['c', 's', 'a', 'm', 'E', 'v'] {
        press(&mut app, KeyCode::Char(key));
    }
    assert!(!app.comment_input.visible);
    assert!(!app.review_confirm.visible, "no review submitted");
    assert!(!app.diff_view.is_visual_mode());
    assert_eq!(app.pending_comments.len(), 1);
}

// ── On screen ──────────────────────────────────────────────────────────

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

#[test]
fn outdated_and_conversation_comments_are_labelled_on_screen() {
    let mut app = app();
    let mut old = comment(50, None, "a.rs", 1, "written against older code");
    old.line = None;
    old.original_line = Some(12);
    app.existing_comments.push(old);
    let mut chat = comment(51, None, "", 1, "general discussion");
    chat.line = None;
    chat.subject_type = None;
    app.existing_comments.push(chat);
    app.rebuild_display();

    let out = screen(&mut app, 140, 40);
    assert!(
        out.contains("outdated · was line 12"),
        "note on the header:\n{out}"
    );
    assert!(
        out.contains("Conversation and review comments"),
        "heading for the trailing section:\n{out}"
    );
    // The outdated comment is inside a.rs's section, above b.rs.
    let outdated_at = out.find("outdated · was line 12").unwrap();
    let b_rs_at = out.find("─── b.rs ───").unwrap();
    let section_at = out.find("Conversation and review comments").unwrap();
    assert!(outdated_at < b_rs_at, "not pushed past the next file");
    assert!(b_rs_at < section_at, "the trailing section comes last");
}

#[test]
fn the_open_panel_is_drawn_beside_the_diff_with_its_hints() {
    let mut app = app();
    app.toggle_comments_panel();
    let out = screen(&mut app, 140, 24);
    assert!(out.contains("Comments (3)"), "{out}");
    assert!(out.contains("○ a.rs:2"), "{out}");
    assert!(out.contains("user5 (+1) · open question"), "{out}");
    assert!(out.contains("✓ b.rs:1"), "{out}");
    assert!(out.contains("✎ a.rs:3"), "{out}");
    assert!(out.contains("[Enter] jump"), "hint bar:\n{out}");
    assert!(out.contains("[r] reply"), "{out}");
    assert!(out.contains("Files ("), "file list still drawn:\n{out}");
}
