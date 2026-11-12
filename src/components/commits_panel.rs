//! A right-hand drawer listing the PR's commits. The first row stands for the
//! full PR diff; each commit below it can be viewed on its own, or "since"
//! (everything after it up to the PR head).
//!
//! Like the other panels it holds only display state; every key goes through
//! the keymap and the handlers in `app/commits.rs` act on the selection.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    widgets::{Block, Borders, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::theme::Theme;
use crate::types::{Commit, DiffSource};

/// Each row (the full-diff row and every commit) takes two lines.
const ROW_HEIGHT: u16 = 2;

pub struct CommitsPanel {
    pub visible: bool,
    commits: Vec<Commit>,
    /// Whether `commits` reflects a finished fetch (an empty list is valid).
    loaded: bool,
    error: Option<String>,
    /// Row 0 is the full PR diff; row `n` is `commits[n - 1]`.
    selected: usize,
    scroll: usize,
    /// Rows that fit on screen, as of the last draw; sizes page jumps.
    page_rows: usize,
}

impl CommitsPanel {
    pub fn new() -> Self {
        Self {
            visible: false,
            commits: Vec::new(),
            loaded: false,
            error: None,
            selected: 0,
            scroll: 0,
            page_rows: 5,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    #[cfg(test)]
    pub fn commits(&self) -> &[Commit] {
        &self.commits
    }

    pub fn set_commits(&mut self, commits: Vec<Commit>) {
        self.commits = commits;
        self.loaded = true;
        self.error = None;
        self.selected = self.selected.min(self.row_count() - 1);
    }

    pub fn set_error(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Forget everything (a different PR is being shown).
    pub fn reset(&mut self) {
        self.commits.clear();
        self.loaded = false;
        self.error = None;
        self.selected = 0;
        self.scroll = 0;
    }

    fn row_count(&self) -> usize {
        self.commits.len() + 1
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1).min(self.row_count() - 1);
    }

    pub fn prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
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
        self.selected = (self.selected + self.page_step(half)).min(self.row_count() - 1);
    }

    pub fn page_up(&mut self, half: bool) {
        self.selected = self.selected.saturating_sub(self.page_step(half));
    }

    pub fn goto_first(&mut self) {
        self.selected = 0;
    }

    pub fn goto_last(&mut self) {
        self.selected = self.row_count() - 1;
    }

    /// The selected commit, or `None` when the full-diff row is selected.
    pub fn selected_commit(&self) -> Option<&Commit> {
        self.selected
            .checked_sub(1)
            .and_then(|i| self.commits.get(i))
    }

    /// The newest commit: the PR head, which "since" comparisons run up to.
    pub fn head_commit(&self) -> Option<&Commit> {
        self.commits.last()
    }

    #[cfg(test)]
    pub fn selected_row(&self) -> usize {
        self.selected
    }

    #[cfg(test)]
    pub fn select_row(&mut self, row: usize) {
        self.selected = row.min(self.row_count() - 1);
    }

    pub fn draw(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        focused: bool,
        active: &DiffSource,
        now_secs: i64,
    ) {
        let border_style = if focused {
            Theme::border_focused()
        } else {
            Theme::border()
        };
        let title = match active {
            DiffSource::Full => format!(" Commits ({}) ", self.commits.len()),
            DiffSource::Commit(sha) => {
                format!(
                    " Commits ({}) · viewing {} ",
                    self.commits.len(),
                    short(sha)
                )
            }
            DiffSource::Since(sha) => {
                format!(" Commits ({}) · since {} ", self.commits.len(), short(sha))
            }
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
        if let Some(error) = &self.error {
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                &format!(" {error}"),
                Theme::status_deleted(),
            );
            return;
        }
        if !self.loaded {
            put(
                buf,
                inner.x,
                inner.y,
                inner.width,
                " Loading commits…",
                meta_style(),
            );
            return;
        }

        let visible_rows = (inner.height / ROW_HEIGHT) as usize;
        self.page_rows = visible_rows.max(1);
        if self.selected < self.scroll {
            self.scroll = self.selected;
        } else if self.selected >= self.scroll + visible_rows {
            self.scroll = self.selected + 1 - visible_rows;
        }

        let end = (self.scroll + visible_rows).min(self.row_count());
        for (slot, row) in (self.scroll..end).enumerate() {
            let y = inner.y + slot as u16 * ROW_HEIGHT;
            let is_selected = row == self.selected;
            let (headline, detail) = self.row_text(row, active, now_secs);
            let (main_style, meta) = if is_selected {
                let sel = Theme::file_list_selected();
                buf.set_style(Rect::new(inner.x, y, inner.width, ROW_HEIGHT), sel);
                (sel, sel.fg(Color::Gray))
            } else {
                (Theme::file_list_normal(), meta_style())
            };
            put(buf, inner.x, y, inner.width, &headline, main_style);
            put(buf, inner.x, y + 1, inner.width, &detail, meta);
        }
    }

    /// The two display lines for a row.
    fn row_text(&self, row: usize, active: &DiffSource, now_secs: i64) -> (String, String) {
        let Some(commit) = row.checked_sub(1).and_then(|i| self.commits.get(i)) else {
            let marker = if active.is_full() { "▸" } else { " " };
            return (
                format!("{marker} All changes"),
                format!("  full PR diff · {} commits", self.commits.len()),
            );
        };
        let marker = match active {
            DiffSource::Commit(sha) if *sha == commit.sha => "●",
            DiffSource::Since(sha) if *sha == commit.sha => "↧",
            _ => " ",
        };
        (
            format!("{marker} {} {}", commit.short_sha(), commit.headline),
            format!(
                "  {} · {}",
                commit.author,
                age_label(&commit.date, now_secs)
            ),
        )
    }
}

fn short(sha: &str) -> &str {
    &sha[..sha.len().min(7)]
}

fn meta_style() -> Style {
    Style::default().fg(Color::DarkGray)
}

/// Write `text` at (x, y), truncated with an ellipsis to `width` cells.
fn put(buf: &mut Buffer, x: u16, y: u16, width: u16, text: &str, style: Style) {
    buf.set_string(x, y, truncate(text, width as usize), style);
}

fn truncate(text: &str, max: usize) -> String {
    if text.width() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    let mut used = 0;
    for c in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if used + w + 1 > max {
            break;
        }
        out.push(c);
        used += w;
    }
    out.push('…');
    out
}

/// Seconds since the Unix epoch for an ISO 8601 UTC timestamp like
/// `2026-11-09T07:21:52Z`. Returns `None` for anything else.
fn parse_iso_utc(s: &str) -> Option<i64> {
    let (date, time) = s.trim_end_matches('Z').split_once('T')?;
    let mut d = date.split('-').map(|p| p.parse::<i64>());
    let (y, m, day) = (d.next()?.ok()?, d.next()?.ok()?, d.next()?.ok()?);
    let mut t = time
        .split(':')
        .map(|p| p.get(..2).unwrap_or(p).parse::<i64>());
    let (h, min, sec) = (t.next()?.ok()?, t.next()?.ok()?, t.next()?.ok()?);

    // Days since 1970-01-01 (Howard Hinnant's civil-date algorithm).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3_600 + min * 60 + sec)
}

/// A short relative age such as `5m`, `3h`, `2d`, `4mo`, `1y`.
pub fn age_label(iso: &str, now_secs: i64) -> String {
    let Some(then) = parse_iso_utc(iso) else {
        return String::new();
    };
    let secs = (now_secs - then).max(0);
    match secs {
        0..=59 => "now".to_string(),
        60..=3_599 => format!("{}m", secs / 60),
        3_600..=86_399 => format!("{}h", secs / 3_600),
        86_400..=2_591_999 => format!("{}d", secs / 86_400),
        2_592_000..=31_535_999 => format!("{}mo", secs / 2_592_000),
        _ => format!("{}y", secs / 31_536_000),
    }
}

#[cfg(test)]
mod tests;
