use ratatui::{buffer::Buffer, layout::Rect};

use super::*;
use crate::types::PrUser;

fn comment(
    id: u64,
    reply_to: Option<u64>,
    path: &str,
    line: Option<usize>,
    body: &str,
) -> ExistingComment {
    ExistingComment {
        id,
        path: path.to_string(),
        line,
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

fn draft(path: &str, line: usize, body: &str) -> ReviewComment {
    ReviewComment::new_local(path.into(), line, Side::Right, body.into(), None, None)
}

fn thread_info(node: &str, resolved: bool) -> ThreadInfo {
    ThreadInfo {
        thread_node_id: node.to_string(),
        is_resolved: resolved,
    }
}

fn entries(existing: &[ExistingComment], pending: &[ReviewComment]) -> Vec<CommentEntry> {
    build_entries(existing, pending, &HashMap::new())
}

fn panel(entries: Vec<CommentEntry>) -> CommentsPanel {
    let mut p = CommentsPanel::new();
    p.set_entries(entries);
    p
}

/// A panel with a thread, a resolved thread, and a draft, in that order.
fn sample_panel() -> CommentsPanel {
    let existing = [
        comment(1, None, "a.rs", Some(3), "open question"),
        comment(2, None, "b.rs", Some(9), "all sorted"),
    ];
    let threads = HashMap::from([(2, thread_info("T2", true)), (1, thread_info("T1", false))]);
    panel(build_entries(
        &existing,
        &[draft("c.rs", 5, "my draft")],
        &threads,
    ))
}

fn render(p: &mut CommentsPanel, height: u16) -> String {
    let area = Rect::new(0, 0, 44, height);
    let mut buf = Buffer::empty(area);
    p.draw(area, &mut buf, true);
    (0..area.height)
        .map(|y| {
            (0..area.width)
                .map(|x| buf[(x, y)].symbol().to_string())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ── build_entries ──────────────────────────────────────────────────────

#[test]
fn replies_are_grouped_under_their_root() {
    let e = entries(
        &[
            comment(10, None, "a.rs", Some(3), "root"),
            comment(11, Some(10), "a.rs", Some(3), "reply one"),
            comment(12, Some(10), "a.rs", Some(3), "reply two"),
        ],
        &[],
    );
    assert_eq!(e.len(), 1, "one thread, not three comments");
    assert_eq!(e[0].root_id, Some(10));
    assert_eq!(e[0].reply_count, 2);
    assert_eq!(e[0].author, "user10", "the root's author");
    assert_eq!(e[0].body, "root");
}

#[test]
fn thread_state_comes_from_the_thread_map() {
    let existing = [comment(1, None, "a.rs", Some(1), "x")];
    let threads = HashMap::from([(1, thread_info("PRRT_abc", true))]);
    let e = build_entries(&existing, &[], &threads);
    assert!(e[0].is_resolved);
    assert_eq!(e[0].thread_node_id.as_deref(), Some("PRRT_abc"));

    let e = build_entries(&existing, &[], &HashMap::new());
    assert!(!e[0].is_resolved, "unknown threads count as open");
    assert_eq!(e[0].thread_node_id, None);
}

#[test]
fn entries_follow_diff_order_with_drafts_after_threads_on_a_line() {
    let e = entries(
        &[
            comment(1, None, "b.rs", Some(2), "b2"),
            comment(2, None, "a.rs", Some(9), "a9"),
            comment(3, None, "a.rs", Some(4), "a4"),
        ],
        &[
            draft("a.rs", 4, "draft on a4"),
            draft("a.rs", 1, "draft on a1"),
        ],
    );
    let order: Vec<_> = e
        .iter()
        .map(|e| (e.path.as_str(), e.line.unwrap(), e.kind))
        .collect();
    assert_eq!(
        order,
        vec![
            ("a.rs", 1, EntryKind::Draft),
            ("a.rs", 4, EntryKind::Thread),
            ("a.rs", 4, EntryKind::Draft),
            ("a.rs", 9, EntryKind::Thread),
            ("b.rs", 2, EntryKind::Thread),
        ]
    );
}

#[test]
fn drafts_keep_their_index_into_the_pending_list() {
    let e = entries(
        &[],
        &[draft("a.rs", 9, "later"), draft("a.rs", 1, "earlier")],
    );
    // Sorted by line, but each still points at its slot in the pending list.
    assert_eq!(e[0].draft_idx, Some(1));
    assert_eq!(e[1].draft_idx, Some(0));
    assert!(
        e.iter()
            .all(|e| e.author == "you" && e.thread_node_id.is_none())
    );
}

#[test]
fn graphite_stack_comments_are_left_out() {
    let e = entries(
        &[
            comment(
                1,
                None,
                "a.rs",
                Some(1),
                "This PR is managed by graphite.dev",
            ),
            comment(2, None, "a.rs", Some(2), "a real comment"),
        ],
        &[],
    );
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].body, "a real comment");
}

#[test]
fn file_level_comments_have_no_line() {
    let e = entries(&[comment(1, None, "a.rs", None, "about the file")], &[]);
    assert_eq!(e[0].line, None);
}

fn kinds_of_anchor() -> Vec<ExistingComment> {
    vec![
        comment(1, None, "a.rs", Some(4), "on a line"),
        ExistingComment {
            original_line: Some(12),
            ..comment(2, None, "a.rs", None, "outdated")
        },
        ExistingComment {
            subject_type: Some("file".into()),
            ..comment(3, None, "a.rs", None, "whole file")
        },
        comment(4, None, "", None, "conversation"),
    ]
}

#[test]
fn comments_are_classified_by_where_they_sit() {
    let e = entries(&kinds_of_anchor(), &[draft("a.rs", 9, "d")]);
    let anchor_of = |root: u64| e.iter().find(|x| x.root_id == Some(root)).unwrap().anchor;
    assert_eq!(anchor_of(1), Anchor::Line(4));
    assert_eq!(anchor_of(2), Anchor::Outdated(Some(12)));
    assert_eq!(anchor_of(3), Anchor::File);
    assert_eq!(anchor_of(4), Anchor::Conversation);
    assert_eq!(
        e.iter()
            .find(|x| x.kind == EntryKind::Draft)
            .unwrap()
            .anchor,
        Anchor::Line(9)
    );
}

#[test]
fn an_outdated_comment_without_an_original_line_is_still_outdated() {
    let e = entries(&[comment(1, None, "a.rs", None, "x")], &[]);
    assert_eq!(e[0].anchor, Anchor::Outdated(None));
}

#[test]
fn conversation_comments_sort_after_everything_else() {
    // An empty path would otherwise sort first.
    let e = entries(&kinds_of_anchor(), &[draft("z.rs", 1, "last file")]);
    assert_eq!(e.last().unwrap().anchor, Anchor::Conversation);
    assert_ne!(e.first().unwrap().anchor, Anchor::Conversation);
}

#[test]
fn rows_label_each_kind_of_anchor() {
    let mut p = panel(entries(&kinds_of_anchor(), &[]));
    let out = render(&mut p, 14);
    assert!(out.contains("a.rs:4"), "{out}");
    assert!(out.contains("a.rs (outdated, was :12)"), "{out}");
    assert!(out.contains("a.rs (file)"), "{out}");
    assert!(out.contains("PR conversation"), "{out}");
}

#[test]
fn conversation_comments_are_not_counted_as_unresolved_threads() {
    let mut p = panel(entries(&kinds_of_anchor(), &[]));
    assert_eq!(p.len(), 4);
    p.cycle_filter(); // unresolved
    assert_eq!(
        p.len(),
        3,
        "the three review comments, not the conversation"
    );
}

#[test]
fn left_side_comments_are_recorded() {
    let mut c = comment(1, None, "a.rs", Some(1), "on a removed line");
    c.side = Some("LEFT".into());
    assert_eq!(entries(&[c], &[])[0].side, Side::Left);
}

#[test]
fn previews_skip_blank_lines_and_summarise_suggestions() {
    let e = entries(
        &[
            comment(1, None, "a.rs", Some(1), "\n\n  first real line  \nsecond"),
            comment(2, None, "a.rs", Some(2), "```suggestion\nlet x = 1;\n```"),
        ],
        &[],
    );
    assert_eq!(e[0].preview(), "first real line");
    assert_eq!(e[1].preview(), "suggested change");
}

// ── Filtering and selection ────────────────────────────────────────────

#[test]
fn the_filter_cycles_through_every_mode() {
    let mut p = sample_panel();
    assert_eq!(p.len(), 3);
    p.cycle_filter();
    assert_eq!(
        (p.filter(), p.len()),
        (Filter::Unresolved, 2),
        "thread + draft"
    );
    p.cycle_filter();
    assert_eq!((p.filter(), p.len()), (Filter::Resolved, 1));
    p.cycle_filter();
    assert_eq!((p.filter(), p.len()), (Filter::Drafts, 1));
    p.cycle_filter();
    assert_eq!((p.filter(), p.len()), (Filter::All, 3));
}

#[test]
fn changing_the_filter_keeps_the_selected_comment_if_it_is_still_shown() {
    let mut p = sample_panel();
    p.select_row(2); // the draft
    p.cycle_filter(); // unresolved: thread + draft
    assert_eq!(p.selected_entry().unwrap().kind, EntryKind::Draft);
    p.cycle_filter(); // resolved: the draft is gone
    assert_eq!(p.selected_entry().unwrap().root_id, Some(2));
}

#[test]
fn a_refresh_keeps_the_same_comment_selected_when_rows_shift() {
    let mut p = sample_panel();
    p.select_row(1); // b.rs, thread 2
    // A new comment sorts before it.
    let existing = [
        comment(0, None, "a.rs", Some(1), "new"),
        comment(1, None, "a.rs", Some(3), "open question"),
        comment(2, None, "b.rs", Some(9), "all sorted"),
    ];
    p.set_entries(build_entries(&existing, &[], &HashMap::new()));
    assert_eq!(
        p.selected_entry().unwrap().root_id,
        Some(2),
        "still thread 2"
    );
}

#[test]
fn a_refresh_that_drops_the_selection_clamps_instead_of_panicking() {
    let mut p = sample_panel();
    p.goto_last();
    p.set_entries(Vec::new());
    assert!(p.selected_entry().is_none());
    p.next();
    p.prev();
    p.page_down(false);
    p.goto_last();
}

#[test]
fn navigation_clamps_at_both_ends() {
    let mut p = sample_panel();
    p.prev();
    assert_eq!(p.selected_row(), 0);
    for _ in 0..10 {
        p.next();
    }
    assert_eq!(p.selected_row(), 2);
    p.goto_first();
    assert_eq!(p.selected_row(), 0);
    p.goto_last();
    assert_eq!(p.selected_row(), 2);
}

#[test]
fn paging_moves_by_the_rows_that_fit() {
    let existing: Vec<_> = (0..40u64)
        .map(|i| comment(i + 1, None, "a.rs", Some(i as usize + 1), "x"))
        .collect();
    let mut p = panel(entries(&existing, &[]));
    render(&mut p, 8); // 6 inner lines = 3 rows
    p.page_down(false);
    assert_eq!(p.selected_row(), 3);
    p.page_down(true);
    assert_eq!(p.selected_row(), 4);
    p.page_up(false);
    assert_eq!(p.selected_row(), 1);
}

// ── Drawing ────────────────────────────────────────────────────────────

#[test]
fn rows_show_status_location_author_and_preview() {
    let mut p = sample_panel();
    let out = render(&mut p, 12);
    assert!(out.contains("Comments (3)"), "{out}");
    assert!(out.contains("○ a.rs:3"), "unresolved marker:\n{out}");
    assert!(out.contains("user1 · open question"), "{out}");
    assert!(out.contains("✓ b.rs:9"), "resolved marker:\n{out}");
    assert!(out.contains("✎ c.rs:5"), "draft marker:\n{out}");
    assert!(out.contains("you · my draft"), "{out}");
}

#[test]
fn reply_counts_are_shown() {
    let mut p = panel(entries(
        &[
            comment(1, None, "a.rs", Some(1), "root"),
            comment(2, Some(1), "a.rs", Some(1), "r1"),
            comment(3, Some(1), "a.rs", Some(1), "r2"),
        ],
        &[],
    ));
    assert!(render(&mut p, 8).contains("user1 (+2) · root"));
}

#[test]
fn the_title_shows_an_active_filter() {
    let mut p = sample_panel();
    p.cycle_filter();
    let out = render(&mut p, 12);
    assert!(out.contains("Comments (2/3) · unresolved"), "{out}");
    assert!(!out.contains("b.rs:9"), "resolved thread hidden:\n{out}");
}

#[test]
fn empty_states_explain_themselves() {
    let mut p = CommentsPanel::new();
    assert!(render(&mut p, 6).contains("No comments yet"));
    let mut p = sample_panel();
    p.cycle_filter(); // unresolved
    p.cycle_filter(); // resolved
    p.cycle_filter(); // drafts
    p.set_entries(entries(&[comment(1, None, "a.rs", Some(1), "x")], &[]));
    assert!(render(&mut p, 6).contains("Nothing matches this filter"));
}

#[test]
fn long_paths_keep_their_tail() {
    assert_eq!(truncate_start("short.rs:1", 40), "short.rs:1");
    let cut = truncate_start("src/components/diff_view/mod.rs:120", 16);
    assert_eq!(cut.chars().count(), 16);
    assert!(cut.starts_with('…') && cut.ends_with("mod.rs:120"), "{cut}");
}

#[test]
fn long_previews_are_truncated_with_an_ellipsis() {
    let long = "a very long comment that cannot possibly fit inside the narrow drawer";
    let mut p = panel(entries(&[comment(1, None, "a.rs", Some(1), long)], &[]));
    let out = render(&mut p, 8);
    assert!(out.contains('…'), "{out}");
    assert!(out.lines().all(|l| l.chars().count() <= 44), "{out}");
}

#[test]
fn the_selected_row_scrolls_into_view() {
    let existing: Vec<_> = (0..20u64)
        .map(|i| comment(i + 1, None, "a.rs", Some(i as usize + 1), "x"))
        .collect();
    let mut p = panel(entries(&existing, &[]));
    p.goto_last();
    let out = render(&mut p, 8);
    assert!(out.contains("a.rs:20"), "{out}");
    assert!(!out.contains("a.rs:1\n"), "top scrolled away:\n{out}");
}
