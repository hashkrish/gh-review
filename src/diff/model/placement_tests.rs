//! Where comments land in the diff: on their line when they have one,
//! otherwise grouped with their file, and only comments that belong to no file
//! in the diff go in the trailing section.

use std::collections::HashSet;

use super::*;
use crate::types::{FileStatus, Hunk, LineKind, PrUser};

fn file(path: &str, lines: usize) -> DiffFile {
    DiffFile {
        path: path.into(),
        status: FileStatus::Modified,
        additions: lines,
        deletions: 0,
        hunks: vec![Hunk {
            header: format!("@@ -0,0 +1,{lines} @@"),
            old_start: 0,
            old_count: 0,
            new_start: 1,
            new_count: lines,
            lines: (1..=lines)
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

fn comment(id: u64, path: &str, line: Option<usize>, body: &str) -> ExistingComment {
    ExistingComment {
        id,
        path: path.into(),
        line,
        side: Some("RIGHT".into()),
        start_line: None,
        body: body.into(),
        user: PrUser {
            login: format!("user{id}"),
        },
        created_at: "2026-11-09T07:00:00Z".into(),
        in_reply_to_id: None,
        original_line: None,
        subject_type: Some("line".into()),
    }
}

fn outdated(id: u64, path: &str, was: Option<usize>, body: &str) -> ExistingComment {
    ExistingComment {
        original_line: was,
        ..comment(id, path, None, body)
    }
}

fn file_level(id: u64, path: &str, body: &str) -> ExistingComment {
    ExistingComment {
        subject_type: Some("file".into()),
        ..comment(id, path, None, body)
    }
}

fn reply(id: u64, to: u64, base: &ExistingComment) -> ExistingComment {
    ExistingComment {
        id,
        in_reply_to_id: Some(to),
        ..comment(id, &base.path, base.line, "a reply")
    }
}

fn pr_level(id: u64, body: &str) -> ExistingComment {
    ExistingComment {
        subject_type: None,
        ..comment(id, "", None, body)
    }
}

fn rows_for(
    files: &[DiffFile],
    comments: &[ExistingComment],
    review_bodies: &[ExistingComment],
    collapsed: &[usize],
) -> Vec<DisplayRow> {
    build_display_rows(
        files,
        comments,
        review_bodies,
        &[],
        &HashSet::new(),
        &HashSet::new(),
        &HashMap::new(),
        100,
        &collapsed.iter().copied().collect(),
    )
}

/// One short label per structural row (bodies and footers are skipped).
fn outline(rows: &[DisplayRow]) -> Vec<String> {
    rows.iter()
        .filter_map(|r| match r {
            DisplayRow::FileHeader { path, .. } => Some(format!("file {path}")),
            DisplayRow::HunkHeader { .. } => Some("hunk".to_string()),
            DisplayRow::DiffLine { line, .. } => {
                Some(format!("line {}", line.new_lineno.unwrap_or(0)))
            }
            DisplayRow::CommentHeader {
                author,
                note,
                is_reply: false,
                ..
            } => Some(match note {
                Some(n) => format!("comment {author} [{n}]"),
                None => format!("comment {author}"),
            }),
            DisplayRow::SectionHeader { text } => Some(format!("section {text}")),
            _ => None,
        })
        .collect()
}

const TRAILING: &str = "section Conversation and review comments";

// ── Inline comments are unchanged ──────────────────────────────────────

#[test]
fn a_comment_on_a_diff_line_stays_right_after_that_line() {
    let rows = rows_for(
        &[file("a.rs", 3)],
        &[comment(1, "a.rs", Some(2), "hi")],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "hunk",
            "line 1",
            "line 2",
            "comment user1",
            "line 3"
        ]
    );
}

// ── Outdated comments go with their file ───────────────────────────────

#[test]
fn an_outdated_comment_sits_at_the_end_of_its_own_file_not_the_whole_diff() {
    let rows = rows_for(
        &[file("a.rs", 2), file("b.rs", 2)],
        &[outdated(1, "a.rs", Some(7), "stale")],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "hunk",
            "line 1",
            "line 2",
            "comment user1 [outdated · was line 7]",
            "file b.rs",
            "hunk",
            "line 1",
            "line 2",
        ],
        "after a.rs's last line, before b.rs"
    );
}

#[test]
fn an_outdated_comment_without_an_original_line_is_still_labelled() {
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[outdated(1, "a.rs", None, "stale")],
        &[],
        &[],
    );
    assert!(outline(&rows).contains(&"comment user1 [outdated]".to_string()));
}

#[test]
fn an_outdated_thread_keeps_its_replies_together() {
    let root = outdated(1, "a.rs", Some(4), "root");
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[
            outdated(1, "a.rs", Some(4), "root"),
            reply(2, 1, &root),
            reply(3, 1, &root),
        ],
        &[],
        &[],
    );
    let headers: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            DisplayRow::CommentHeader {
                is_reply: false,
                reply_count,
                ..
            } => Some(*reply_count),
            _ => None,
        })
        .collect();
    assert_eq!(
        headers,
        [2],
        "one thread with two replies, not three threads"
    );
}

#[test]
fn outdated_comments_for_several_files_each_follow_their_own_file() {
    let rows = rows_for(
        &[file("a.rs", 1), file("b.rs", 1)],
        &[
            outdated(1, "b.rs", Some(1), "on b"),
            outdated(2, "a.rs", Some(1), "on a"),
        ],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "hunk",
            "line 1",
            "comment user2 [outdated · was line 1]",
            "file b.rs",
            "hunk",
            "line 1",
            "comment user1 [outdated · was line 1]",
        ]
    );
}

// ── File-level comments go at the top of the file ──────────────────────

#[test]
fn a_file_level_comment_comes_before_the_hunks() {
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[file_level(1, "a.rs", "about the file")],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "comment user1 [file comment]",
            "hunk",
            "line 1"
        ]
    );
}

// ── Comments the diff can't show a line for are not lost ───────────────

#[test]
fn a_line_comment_outside_the_shown_hunks_goes_to_the_end_of_its_file() {
    // Line 99 isn't in the diff. This used to vanish without a trace.
    let rows = rows_for(
        &[file("a.rs", 2)],
        &[comment(1, "a.rs", Some(99), "far away")],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "hunk",
            "line 1",
            "line 2",
            "comment user1 [line 99 isn't in the shown diff]"
        ]
    );
}

#[test]
fn a_comment_on_a_file_without_a_patch_is_still_shown() {
    let mut binary = file("img.png", 0);
    binary.hunks.clear();
    let rows = rows_for(
        &[binary],
        &[comment(1, "img.png", Some(3), "nice")],
        &[],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file img.png",
            "comment user1 [line 3 isn't in the shown diff]"
        ]
    );
}

#[test]
fn no_comment_is_shown_twice() {
    let rows = rows_for(
        &[file("a.rs", 3)],
        &[
            comment(1, "a.rs", Some(2), "inline"),
            outdated(2, "a.rs", Some(9), "old"),
            file_level(3, "a.rs", "file"),
            comment(4, "a.rs", Some(50), "away"),
        ],
        &[],
        &[],
    );
    let comment_rows = outline(&rows)
        .iter()
        .filter(|l| l.starts_with("comment"))
        .count();
    assert_eq!(comment_rows, 4);
}

// ── The trailing section is only for what belongs to no file ───────────

#[test]
fn conversation_comments_and_review_summaries_get_a_titled_trailing_section() {
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[pr_level(1, "general chat")],
        &[pr_level(2, "[Approved] looks good")],
        &[],
    );
    assert_eq!(
        outline(&rows),
        [
            "file a.rs",
            "hunk",
            "line 1",
            TRAILING,
            "comment user1",
            "comment user2"
        ],
        "no explanatory note: these were never on a line"
    );
}

#[test]
fn a_comment_on_a_file_the_pr_no_longer_changes_goes_to_the_trailing_section() {
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[comment(1, "gone.rs", Some(3), "orphaned")],
        &[],
        &[],
    );
    let o = outline(&rows);
    let section = o
        .iter()
        .position(|l| l == TRAILING)
        .expect("a trailing section");
    assert!(o[section + 1].starts_with("comment user1"));
}

#[test]
fn there_is_no_trailing_section_when_every_comment_has_a_file() {
    let rows = rows_for(
        &[file("a.rs", 2)],
        &[
            comment(1, "a.rs", Some(1), "x"),
            outdated(2, "a.rs", Some(5), "y"),
        ],
        &[],
        &[],
    );
    assert!(!outline(&rows).contains(&TRAILING.to_string()));
}

#[test]
fn graphite_stack_comments_are_never_shown() {
    let graphite = pr_level(1, "This PR is managed by graphite.dev");
    let on_file = outdated(2, "a.rs", Some(1), "stack info managed by graphite.dev");
    let rows = rows_for(&[file("a.rs", 1)], &[graphite, on_file], &[], &[]);
    assert!(!outline(&rows).iter().any(|l| l.starts_with("comment")));
    assert!(!outline(&rows).contains(&TRAILING.to_string()));
}

// ── Folded files ───────────────────────────────────────────────────────

#[test]
fn a_folded_file_hides_its_outdated_comments_without_moving_them_to_the_end() {
    let rows = rows_for(
        &[file("a.rs", 2), file("b.rs", 1)],
        &[
            outdated(1, "a.rs", Some(7), "stale"),
            file_level(2, "a.rs", "file note"),
        ],
        &[],
        &[0],
    );
    assert_eq!(outline(&rows), ["file a.rs", "file b.rs", "hunk", "line 1"]);
}

// ── Bodies ─────────────────────────────────────────────────────────────

#[test]
fn an_open_outdated_comment_shows_its_body_and_a_footer() {
    let rows = rows_for(
        &[file("a.rs", 1)],
        &[outdated(1, "a.rs", Some(7), "stale")],
        &[],
        &[],
    );
    let header = rows
        .iter()
        .position(|r| matches!(r, DisplayRow::CommentHeader { .. }))
        .unwrap();
    assert!(
        rows[header..]
            .iter()
            .any(|r| matches!(r, DisplayRow::CommentFooter { .. }))
    );
}

#[test]
fn a_resolved_outdated_thread_is_collapsed_by_default() {
    let comments = [outdated(1, "a.rs", Some(7), "done")];
    let threads = HashMap::from([(
        1,
        ThreadInfo {
            thread_node_id: "T".into(),
            is_resolved: true,
        },
    )]);
    let rows = build_display_rows(
        &[file("a.rs", 1)],
        &comments,
        &[],
        &[],
        &HashSet::new(),
        &HashSet::new(),
        &threads,
        100,
        &HashSet::new(),
    );
    assert!(matches!(
        rows.iter()
            .find(|r| matches!(r, DisplayRow::CommentHeader { .. })),
        Some(DisplayRow::CommentHeader {
            expanded: false,
            is_resolved: true,
            ..
        })
    ));
}
