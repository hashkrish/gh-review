//! A right-hand drawer listing every review thread in the PR plus the user's
//! pending drafts, so comments can be found and acted on without scrolling
//! the diff: jump to one, reply, resolve it, edit or discard a draft.
//!
//! Like the commits panel it holds display state only. The list is rebuilt
//! from the app's comment data whenever that changes ([`build_entries`]),
//! and handlers in `app/comments.rs` act on the selected entry.

use std::collections::{BTreeMap, HashMap};

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::stack::graphite::is_graphite_stack_comment;
use crate::theme::Theme;
use crate::types::{ExistingComment, ReviewComment, Side, ThreadInfo};

/// Each entry takes two lines: where it is, then who said what.
const ROW_HEIGHT: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A published review thread (a root comment and its replies).
    Thread,
    /// A comment in the user's own pending review, not yet submitted.
    Draft,
}

/// Where a comment sits, which decides how it's labelled and which actions
/// apply to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor {
    /// On a line of the diff (or a draft's line).
    Line(usize),
    /// Written on code that has since changed; the line it was on, if known.
    Outdated(Option<usize>),
    /// On a whole file rather than a line.
    File,
    /// A PR conversation comment: not attached to any file, and not a review
    /// thread, so it can't be resolved or replied to from here.
    Conversation,
}

/// One row of the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct CommentEntry {
    pub anchor: Anchor,
    pub kind: EntryKind,
    /// GitHub id of the thread's root comment (threads only); the id a reply
    /// is posted to and the anchor of its `#discussion_r…` link.
    pub root_id: Option<u64>,
    /// GraphQL id of the thread, needed to resolve it (threads only).
    pub thread_node_id: Option<String>,
    /// Index into the app's pending comments (drafts only).
    pub draft_idx: Option<usize>,
    pub path: String,
    /// `None` for file-level and outdated comments, which have no line.
    pub line: Option<usize>,
    pub side: Side,
    pub author: String,
    /// The root comment's full text.
    pub body: String,
    pub reply_count: usize,
    pub is_resolved: bool,
}

impl CommentEntry {
    fn key(&self) -> (EntryKind, u64) {
        match self.kind {
            EntryKind::Thread => (self.kind, self.root_id.unwrap_or(0)),
            EntryKind::Draft => (self.kind, self.draft_idx.unwrap_or(0) as u64),
        }
    }

    fn preview(&self) -> String {
        let first = self
            .body
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("");
        if first.starts_with("```suggestion") {
            "suggested change".to_string()
        } else {
            first.to_string()
        }
    }
}

/// Which entries the panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Filter {
    #[default]
    All,
    /// Open threads and your drafts: what still needs attention.
    Unresolved,
    Resolved,
    Drafts,
}

impl Filter {
    pub fn next(self) -> Self {
        match self {
            Self::All => Self::Unresolved,
            Self::Unresolved => Self::Resolved,
            Self::Resolved => Self::Drafts,
            Self::Drafts => Self::All,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Unresolved => "unresolved",
            Self::Resolved => "resolved",
            Self::Drafts => "drafts",
        }
    }

    fn accepts(self, e: &CommentEntry) -> bool {
        // Conversation comments aren't review threads, so "resolved" and
        // "unresolved" don't apply to them; they only appear under "all".
        let conversation = e.anchor == Anchor::Conversation;
        match self {
            Self::All => true,
            Self::Unresolved => !e.is_resolved && !conversation,
            Self::Resolved => e.is_resolved,
            Self::Drafts => e.kind == EntryKind::Draft,
        }
    }
}

/// Build the entries from the app's comment data: one per thread (grouped by
/// root comment) and one per pending draft, in diff order (file, then line).
/// Graphite's auto-generated stack comments are left out, as in the diff.
pub fn build_entries(
    existing: &[ExistingComment],
    pending: &[ReviewComment],
    threads: &HashMap<u64, ThreadInfo>,
) -> Vec<CommentEntry> {
    let mut by_root: BTreeMap<u64, Vec<&ExistingComment>> = BTreeMap::new();
    for c in existing
        .iter()
        .filter(|c| !is_graphite_stack_comment(&c.body))
    {
        by_root
            .entry(c.in_reply_to_id.unwrap_or(c.id))
            .or_default()
            .push(c);
    }

    let mut entries: Vec<CommentEntry> = by_root
        .into_iter()
        .map(|(root_id, comments)| {
            // The root is normally the comment whose id is the thread id.
            let root = comments
                .iter()
                .find(|c| c.id == root_id)
                .unwrap_or(&comments[0]);
            let info = threads.get(&root_id);
            CommentEntry {
                anchor: if root.is_pr_level() {
                    Anchor::Conversation
                } else if root.is_file_level() {
                    Anchor::File
                } else if let Some(line) = root.line {
                    Anchor::Line(line)
                } else {
                    Anchor::Outdated(root.original_line)
                },
                kind: EntryKind::Thread,
                root_id: Some(root_id),
                thread_node_id: info.map(|t| t.thread_node_id.clone()),
                draft_idx: None,
                path: root.path.clone(),
                line: root.line,
                side: if root.side.as_deref() == Some("LEFT") {
                    Side::Left
                } else {
                    Side::Right
                },
                author: root.user.login.clone(),
                body: root.body.clone(),
                reply_count: comments.len() - 1,
                is_resolved: info.map(|t| t.is_resolved).unwrap_or(false),
            }
        })
        .collect();

    entries.extend(pending.iter().enumerate().map(|(idx, c)| CommentEntry {
        anchor: Anchor::Line(c.line),
        kind: EntryKind::Draft,
        root_id: None,
        thread_node_id: None,
        draft_idx: Some(idx),
        path: c.path.clone(),
        line: Some(c.line),
        side: c.side,
        author: "you".to_string(),
        body: c.body.clone(),
        reply_count: 0,
        is_resolved: false,
    }));

    // Diff order, with conversation comments last (as in the diff). Within a
    // line, published threads come before drafts.
    entries.sort_by(|a, b| {
        let kind_rank = |e: &CommentEntry| (e.kind == EntryKind::Draft) as u8;
        let conversation = |e: &CommentEntry| (e.anchor == Anchor::Conversation) as u8;
        (conversation(a), &a.path, a.line, kind_rank(a), a.key().1).cmp(&(
            conversation(b),
            &b.path,
            b.line,
            kind_rank(b),
            b.key().1,
        ))
    });
    entries
}

pub struct CommentsPanel {
    pub visible: bool,
    entries: Vec<CommentEntry>,
    filter: Filter,
    /// Index into the *filtered* list.
    selected: usize,
    scroll: usize,
    /// Rows that fit on screen, as of the last draw; sizes page jumps.
    page_rows: usize,
}

impl CommentsPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            entries: Vec::new(),
            filter: Filter::All,
            selected: 0,
            scroll: 0,
            page_rows: 5,
        }
    }

    #[cfg(test)]
    pub fn filter(&self) -> Filter {
        self.filter
    }

    fn shown(&self) -> Vec<&CommentEntry> {
        self.entries
            .iter()
            .filter(|e| self.filter.accepts(e))
            .collect()
    }

    /// Replace the entries, keeping the same comment selected if it still
    /// exists (otherwise the nearest row).
    pub fn set_entries(&mut self, entries: Vec<CommentEntry>) {
        let selected_key = self.selected_entry().map(CommentEntry::key);
        self.entries = entries;
        let shown = self.shown();
        self.selected = selected_key
            .and_then(|k| shown.iter().position(|e| e.key() == k))
            .unwrap_or(self.selected)
            .min(shown.len().saturating_sub(1));
    }

    pub fn selected_entry(&self) -> Option<&CommentEntry> {
        self.shown().get(self.selected).copied()
    }

    /// Switch to the next filter, keeping the selection on the same comment
    /// when it is still shown.
    pub fn cycle_filter(&mut self) {
        let key = self.selected_entry().map(CommentEntry::key);
        self.filter = self.filter.next();
        let shown = self.shown();
        self.selected = key
            .and_then(|k| shown.iter().position(|e| e.key() == k))
            .unwrap_or(0)
            .min(shown.len().saturating_sub(1));
        self.scroll = 0;
    }

    pub fn len(&self) -> usize {
        self.shown().len()
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1).min(self.len().saturating_sub(1));
    }

    pub fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn goto_first(&mut self) {
        self.selected = 0;
    }

    pub fn goto_last(&mut self) {
        self.selected = self.len().saturating_sub(1);
    }

    /// Rows to jump for a page: all that fit on screen, or half of them.
    fn page_step(&self, half: bool) -> usize {
        if half {
            (self.page_rows / 2).max(1)
        } else {
            self.page_rows.max(1)
        }
    }

    pub fn page_down(&mut self, half: bool) {
        self.selected = (self.selected + self.page_step(half)).min(self.len().saturating_sub(1));
    }

    pub fn page_up(&mut self, half: bool) {
        self.selected = self.selected.saturating_sub(self.page_step(half));
    }

    #[cfg(test)]
    pub fn selected_row(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub fn select_row(&mut self, row: usize) {
        self.selected = row.min(self.len().saturating_sub(1));
    }

    pub fn draw(&mut self, area: Rect, buf: &mut Buffer, focused: bool) {
        let border_style = if focused {
            Theme::border_focused()
        } else {
            Theme::border()
        };
        // Borrow just the entries (not all of `self`) so the scroll state can
        // be updated below while the list is in use.
        let filter = self.filter;
        let shown: Vec<&CommentEntry> = self.entries.iter().filter(|e| filter.accepts(e)).collect();
        let title = match self.filter {
            Filter::All => format!(" Comments ({}) ", self.entries.len()),
            f => format!(
                " Comments ({}/{}) · {} ",
                shown.len(),
                self.entries.len(),
                f.label()
            ),
        };
        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(border_style);
        let inner = block.inner(area);
        Widget::render(block, area, buf);

        if inner.height < ROW_HEIGHT || inner.width < 8 {
            return;
        }
        if shown.is_empty() {
            let text = if self.entries.is_empty() {
                " No comments yet"
            } else {
                " Nothing matches this filter"
            };
            put(buf, inner.x, inner.y, inner.width, text, meta_style());
            return;
        }

        let visible_rows = (inner.height / ROW_HEIGHT) as usize;
        self.page_rows = visible_rows.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected + 1 - visible_rows;
        }

        let end = (self.scroll + visible_rows).min(shown.len());
        for (slot, row) in (self.scroll..end).enumerate() {
            let entry = shown[row];
            let y = inner.y + slot as u16 * ROW_HEIGHT;
            let is_selected = row == self.selected;
            let (marker, marker_color) = match (entry.kind, entry.is_resolved) {
                (EntryKind::Draft, _) => ("✎", Color::Cyan),
                (EntryKind::Thread, true) => ("✓", Color::Green),
                (EntryKind::Thread, false) => ("○", Color::Yellow),
            };
            let (main_style, meta) = if is_selected {
                let sel = Theme::file_list_selected();
                buf.set_style(Rect::new(inner.x, y, inner.width, ROW_HEIGHT), sel);
                (sel, sel.fg(Color::Gray))
            } else {
                (Theme::file_list_normal(), meta_style())
            };

            let location = match entry.anchor {
                Anchor::Line(line) => format!("{}:{line}", entry.path),
                Anchor::Outdated(Some(was)) => {
                    format!("{} (outdated, was :{was})", entry.path)
                }
                Anchor::Outdated(None) => format!("{} (outdated)", entry.path),
                Anchor::File => format!("{} (file)", entry.path),
                Anchor::Conversation => "PR conversation".to_string(),
            };
            buf.set_string(inner.x, y, marker, main_style.fg(marker_color));
            let loc_width = inner.width.saturating_sub(2);
            put(
                buf,
                inner.x + 2,
                y,
                loc_width,
                &truncate_start(&location, loc_width as usize),
                main_style,
            );

            let replies = if entry.reply_count > 0 {
                format!(" (+{})", entry.reply_count)
            } else {
                String::new()
            };
            put(
                buf,
                inner.x + 2,
                y + 1,
                loc_width,
                &format!("{}{replies} · {}", entry.author, entry.preview()),
                meta,
            );
        }
    }
}

fn meta_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Write `text` at (x, y), truncated with an ellipsis to `width` cells.
fn put(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    buf.set_string(x, y, truncate_end(text, width as usize), style);
}

fn truncate_end(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Like [`truncate_end`] but keeps the tail, which is the informative part of
/// a path (`…/components/a.rs:12`).
fn truncate_start(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    let mut tail: Vec<char> = Vec::new();
    let mut used = 1; // the ellipsis
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > max {
            break;
        }
        tail.push(c);
        used += w;
    }
    tail.push('…');
    tail.into_iter().rev().collect()
}

#[cfg(test)]
mod tests;
