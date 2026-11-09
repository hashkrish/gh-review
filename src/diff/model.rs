use std::collections::HashMap;

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::layout;
use super::wrap::wrap_spans;
use crate::stack::graphite;
use crate::types::{
    DiffFile, DiffLine, ExistingComment, LineKind, ReviewComment, Side, ThreadInfo,
};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum DisplayRow {
    FileHeader {
        path: String,
        file_idx: usize,
        collapsed: bool,
        viewed: bool,
    },
    HunkHeader {
        text: String,
        file_idx: usize,
    },
    DiffLine {
        line: DiffLine,
        file_idx: usize,
        hunk_idx: usize,
        line_idx: usize,
    },
    ExpandHint {
        file_idx: usize,
        hunk_idx: usize,
        direction: ExpandDirection,
        available_lines: usize,
    },
    CommentHeader {
        author: String,
        is_pending: bool,
        github_id: Option<u64>,
        pending_idx: Option<usize>,
        thread_root_id: Option<u64>,
        thread_node_id: Option<String>,
        is_resolved: bool,
        expanded: bool,
        reply_count: usize,
        body_preview: String,
        is_reply: bool,
        /// Why the thread isn't on a diff line ("outdated · was line 12"),
        /// for threads shown at the end of a file instead of inline.
        note: Option<String>,
    },
    /// A heading for comments that belong to no file in the diff.
    SectionHeader {
        text: String,
    },
    CommentBodyLine {
        line: Line<'static>,
        is_reply: bool,
        is_resolved: bool,
        is_pending: bool,
        is_suggestion: bool,
    },
    CommentFooter {
        is_reply: bool,
        is_resolved: bool,
        is_pending: bool,
    },
}

#[derive(Debug, Clone, Copy)]
#[allow(dead_code)]
pub enum ExpandDirection {
    Up,
    Down,
}

use super::suggestion;
use crate::types::DiffLine as HunkLine;

/// Context needed to render an inline suggestion diff (original lines from the hunk).
struct SuggestionContext<'a> {
    hunk_lines: &'a [HunkLine],
    current_line: &'a HunkLine,
    lineno: usize,
    start_line: Option<usize>,
    file_path: &'a str,
}

/// Emit the expanded body of a comment thread: top padding, markdown body,
/// optional suggestion diff, replies, bottom padding, and footer.
///
/// This is the shared rendering path used by inline threads, pending comments,
/// and orphan threads.
#[allow(clippy::too_many_arguments)]
fn emit_expanded_thread(
    rows: &mut Vec<DisplayRow>,
    root_body: &str,
    replies: &[&ExistingComment],
    is_resolved: bool,
    is_pending: bool,
    root_id: u64,
    body_max_width: usize,
    sug_ctx: Option<SuggestionContext>,
) {
    rows.push(blank_body_row(is_resolved, is_pending));

    let sug_text = suggestion::extract(root_body);
    let body_text = if sug_text.is_some() {
        suggestion::strip_block(root_body)
    } else {
        root_body.to_string()
    };

    if !body_text.trim().is_empty() {
        let md_lines = render_markdown_to_lines(&body_text);
        rows.extend(wrap_body_lines(
            md_lines,
            body_max_width,
            false,
            is_resolved,
            is_pending,
        ));
    }

    if let Some(ref suggested) = sug_text
        && let Some(ctx) = &sug_ctx
    {
        let original_lines = suggestion::collect_original_lines(
            ctx.hunk_lines,
            ctx.current_line,
            ctx.lineno,
            ctx.start_line,
        );
        rows.extend(suggestion::build_rows(
            ctx.file_path,
            &original_lines,
            suggested,
            is_resolved,
        ));
    }

    for reply in replies {
        rows.push(blank_body_row(is_resolved, is_pending));
        rows.push(DisplayRow::CommentHeader {
            author: reply.user.login.clone(),
            is_pending: false,
            github_id: Some(reply.id),
            pending_idx: None,
            thread_root_id: Some(root_id),
            thread_node_id: None,
            is_resolved,
            expanded: true,
            reply_count: 0,
            body_preview: String::new(),
            is_reply: true,
            note: None,
        });
        let reply_lines = render_markdown_to_lines(&reply.body);
        rows.extend(wrap_body_lines(
            reply_lines,
            body_max_width,
            true,
            is_resolved,
            is_pending,
        ));
    }

    rows.push(blank_body_row(is_resolved, is_pending));
    rows.push(DisplayRow::CommentFooter {
        is_reply: false,
        is_resolved,
        is_pending,
    });
}

fn render_markdown_to_lines(body: &str) -> Vec<Line<'static>> {
    let text = tui_markdown::from_str(body);
    text.lines
        .into_iter()
        .map(|line| {
            Line::from(
                line.spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.to_string(), span.style))
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn blank_body_row(is_resolved: bool, is_pending: bool) -> DisplayRow {
    DisplayRow::CommentBodyLine {
        line: Line::default(),
        is_reply: false,
        is_resolved,
        is_pending,
        is_suggestion: false,
    }
}

fn wrap_body_lines(
    md_lines: Vec<Line<'static>>,
    max_width: usize,
    is_reply: bool,
    is_resolved: bool,
    is_pending: bool,
) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    for ml in md_lines {
        let total_width: usize = ml.spans.iter().map(|s| s.content.width()).sum();
        if total_width <= max_width {
            rows.push(DisplayRow::CommentBodyLine {
                line: ml,
                is_reply,
                is_resolved,
                is_pending,
                is_suggestion: false,
            });
        } else {
            for wrapped_line in wrap_spans(&ml.spans, max_width) {
                rows.push(DisplayRow::CommentBodyLine {
                    line: wrapped_line,
                    is_reply,
                    is_resolved,
                    is_pending,
                    is_suggestion: false,
                });
            }
        }
    }
    rows
}

type Threads<'a> = std::collections::BTreeMap<u64, Vec<&'a ExistingComment>>;

/// Group comments into threads by root comment id (oldest first).
fn group_threads<'a>(comments: impl IntoIterator<Item = &'a ExistingComment>) -> Threads<'a> {
    let mut threads = Threads::new();
    for c in comments {
        threads
            .entry(c.in_reply_to_id.unwrap_or(c.id))
            .or_default()
            .push(c);
    }
    threads
}

/// What to say about a comment shown away from a diff line. `None` for PR
/// conversation comments and review summaries, which have no place in a file.
fn anchor_note(c: &ExistingComment) -> Option<String> {
    if c.is_file_level() {
        return Some("file comment".to_string());
    }
    if c.is_pr_level() {
        return None;
    }
    Some(match (c.line, c.original_line) {
        (None, Some(n)) => format!("outdated · was line {n}"),
        (None, None) => "outdated".to_string(),
        (Some(n), _) => format!("line {n} isn't in the shown diff"),
    })
}

/// Emit threads that aren't attached to a diff line: each as a header (with
/// a note saying why) followed by its body when open.
fn emit_threads(
    rows: &mut Vec<DisplayRow>,
    threads: &Threads,
    thread_map: &HashMap<u64, ThreadInfo>,
    expanded_threads: &std::collections::HashSet<u64>,
    body_max_width: usize,
) {
    for (root_id, thread_comments) in threads {
        let root = thread_comments[0];
        let reply_count = thread_comments.len().saturating_sub(1);
        let thread_info = thread_map.get(root_id);
        let thread_node_id = thread_info.map(|t| t.thread_node_id.clone());
        let is_resolved = thread_info.map(|t| t.is_resolved).unwrap_or(false);
        let default_open = !is_resolved;
        let is_expanded = if expanded_threads.contains(root_id) {
            !default_open
        } else {
            default_open
        };
        let preview = root.body.lines().next().unwrap_or("").to_string();

        rows.push(DisplayRow::CommentHeader {
            author: root.user.login.clone(),
            is_pending: false,
            github_id: Some(root.id),
            pending_idx: None,
            thread_root_id: Some(*root_id),
            thread_node_id,
            is_resolved,
            expanded: is_expanded,
            reply_count,
            body_preview: preview,
            is_reply: false,
            note: anchor_note(root),
        });

        if is_expanded {
            let replies: Vec<&ExistingComment> = thread_comments.iter().skip(1).copied().collect();
            emit_expanded_thread(
                rows,
                &root.body,
                &replies,
                is_resolved,
                false,
                *root_id,
                body_max_width,
                None,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build_display_rows(
    files: &[DiffFile],
    existing_comments: &[ExistingComment],
    review_body_comments: &[ExistingComment],
    pending_comments: &[ReviewComment],
    expanded_threads: &std::collections::HashSet<u64>,
    expanded_pending: &std::collections::HashSet<usize>,
    thread_map: &HashMap<u64, ThreadInfo>,
    wrap_width: usize,
    collapsed_files: &std::collections::HashSet<usize>,
) -> Vec<DisplayRow> {
    let mut rows = Vec::new();
    let body_max_width = layout::comment_body_width(wrap_width);
    // Thread roots already shown on a diff line.
    let mut placed_roots: std::collections::HashSet<u64> = std::collections::HashSet::new();
    let real_comments = |c: &&ExistingComment| !graphite::is_graphite_stack_comment(&c.body);

    for (file_idx, file) in files.iter().enumerate() {
        let collapsed = collapsed_files.contains(&file_idx);
        rows.push(DisplayRow::FileHeader {
            path: file.path.clone(),
            file_idx,
            collapsed,
            viewed: file.viewed,
        });

        if collapsed {
            continue;
        }

        // Comments on the whole file sit at the top of its section.
        let file_level = group_threads(
            existing_comments
                .iter()
                .filter(real_comments)
                .filter(|c| c.path == file.path && c.is_file_level()),
        );
        emit_threads(
            &mut rows,
            &file_level,
            thread_map,
            expanded_threads,
            body_max_width,
        );

        for (hunk_idx, hunk) in file.hunks.iter().enumerate() {
            rows.push(DisplayRow::HunkHeader {
                text: hunk.header.clone(),
                file_idx,
            });

            for (line_idx, line) in hunk.lines.iter().enumerate() {
                rows.push(DisplayRow::DiffLine {
                    line: line.clone(),
                    file_idx,
                    hunk_idx,
                    line_idx,
                });

                let (target_line, target_side) = match line.kind {
                    LineKind::Added | LineKind::Context => (line.new_lineno, Side::Right),
                    LineKind::Removed => (line.old_lineno, Side::Left),
                };

                if let Some(lineno) = target_line {
                    let line_comments: Vec<&ExistingComment> = existing_comments
                        .iter()
                        .filter(|c| {
                            c.path == file.path
                                && c.line == Some(lineno)
                                && !graphite::is_graphite_stack_comment(&c.body)
                                && matches!(
                                    (c.side.as_deref(), &target_side),
                                    (Some("LEFT"), Side::Left)
                                        | (Some("RIGHT"), Side::Right)
                                        | (None, _)
                                )
                        })
                        .collect();

                    let mut threads: std::collections::BTreeMap<u64, Vec<&ExistingComment>> =
                        std::collections::BTreeMap::new();
                    for ec in &line_comments {
                        let root_id = ec.in_reply_to_id.unwrap_or(ec.id);
                        threads.entry(root_id).or_default().push(ec);
                    }

                    for (root_id, thread_comments) in &threads {
                        // Remember it landed on a line, so it isn't repeated
                        // among the file's unplaced comments below.
                        placed_roots.insert(*root_id);
                        let root = thread_comments[0];
                        let reply_count = thread_comments.len().saturating_sub(1);
                        let thread_info = thread_map.get(root_id);
                        let thread_node_id = thread_info.map(|t| t.thread_node_id.clone());
                        let is_resolved = thread_info.map(|t| t.is_resolved).unwrap_or(false);
                        let default_open = !is_resolved;
                        let is_expanded = if expanded_threads.contains(root_id) {
                            !default_open
                        } else {
                            default_open
                        };
                        let preview = root.body.lines().next().unwrap_or("").to_string();

                        rows.push(DisplayRow::CommentHeader {
                            author: root.user.login.clone(),
                            is_pending: false,
                            github_id: Some(root.id),
                            pending_idx: None,
                            thread_root_id: Some(*root_id),
                            thread_node_id,
                            is_resolved,
                            expanded: is_expanded,
                            reply_count,
                            body_preview: preview,
                            is_reply: false,
                            note: None,
                        });

                        if is_expanded {
                            let replies: Vec<&ExistingComment> =
                                thread_comments.iter().skip(1).copied().collect();
                            emit_expanded_thread(
                                &mut rows,
                                &root.body,
                                &replies,
                                is_resolved,
                                false,
                                *root_id,
                                body_max_width,
                                Some(SuggestionContext {
                                    hunk_lines: &hunk.lines,
                                    current_line: line,
                                    lineno,
                                    start_line: root.start_line,
                                    file_path: &file.path,
                                }),
                            );
                        }
                    }

                    for (pc_idx, pc) in pending_comments.iter().enumerate().filter(|(_, c)| {
                        c.path == file.path && c.line == lineno && c.side == target_side
                    }) {
                        let is_expanded = expanded_pending.contains(&pc_idx);
                        let preview = pc.body.lines().next().unwrap_or("").to_string();

                        rows.push(DisplayRow::CommentHeader {
                            author: String::new(),
                            is_pending: true,
                            github_id: None,
                            pending_idx: Some(pc_idx),
                            thread_root_id: None,
                            thread_node_id: None,
                            is_resolved: false,
                            expanded: is_expanded,
                            reply_count: 0,
                            body_preview: preview,
                            is_reply: false,
                            note: None,
                        });

                        if is_expanded {
                            emit_expanded_thread(
                                &mut rows,
                                &pc.body,
                                &[],
                                false,
                                true,
                                0,
                                body_max_width,
                                Some(SuggestionContext {
                                    hunk_lines: &hunk.lines,
                                    current_line: line,
                                    lineno,
                                    start_line: None,
                                    file_path: &file.path,
                                }),
                            );
                        }
                    }
                }
            }
        }

        // Comments on this file that didn't land on a diff line (outdated, or
        // on a line the diff doesn't show) go at the end of its section
        // instead of being lost.
        let unplaced = group_threads(existing_comments.iter().filter(real_comments).filter(|c| {
            c.path == file.path
                && !c.is_file_level()
                && !placed_roots.contains(&c.in_reply_to_id.unwrap_or(c.id))
        }));
        emit_threads(
            &mut rows,
            &unplaced,
            thread_map,
            expanded_threads,
            body_max_width,
        );
    }

    // Comments that belong to no file in the diff: PR conversation comments,
    // review summaries, and comments on files the PR no longer changes.
    let in_diff: std::collections::HashSet<&str> = files.iter().map(|f| f.path.as_str()).collect();
    let trailing = group_threads(
        existing_comments
            .iter()
            .filter(real_comments)
            .filter(|c| !in_diff.contains(c.path.as_str()))
            .chain(review_body_comments.iter()),
    );
    if !trailing.is_empty() {
        rows.push(DisplayRow::SectionHeader {
            text: "Conversation and review comments".to_string(),
        });
        emit_threads(
            &mut rows,
            &trailing,
            thread_map,
            expanded_threads,
            body_max_width,
        );
    }

    rows
}

#[cfg(test)]
mod placement_tests;
