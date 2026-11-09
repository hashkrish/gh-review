//! Modal shown when a draft comment differs between gh-review and GitHub's
//! pending review and the two edits can't be merged automatically.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap},
};

use crate::sync::{Conflict, ConflictKind};
use crate::theme::Theme;

/// Show at most this many lines of each version.
const MAX_BODY_LINES: usize = 4;

fn body_lines(label: &str, body: &str) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(Span::styled(
        format!("  {label}"),
        Theme::review_bar_key(),
    ))];
    let total = body.lines().count();
    for l in body.lines().take(MAX_BODY_LINES) {
        lines.push(Line::from(Span::styled(
            format!("    {l}"),
            Theme::review_bar_label(),
        )));
    }
    if total > MAX_BODY_LINES {
        lines.push(Line::from(Span::styled(
            format!("    … {} more lines", total - MAX_BODY_LINES),
            Theme::review_bar_label(),
        )));
    }
    lines
}

pub fn draw(area: Rect, buf: &mut Buffer, conflict: &Conflict, total: usize) {
    let width = 70u16.min(area.width.saturating_sub(4));
    let height = 20u16.min(area.height.saturating_sub(4));
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    let popup = Rect::new(x, y, width, height);

    Widget::render(Clear, popup, buf);
    let block = Block::default()
        .title(format!(" Pending review conflict (1 of {total}) "))
        .borders(Borders::ALL)
        .border_style(Theme::border_focused());
    let inner = block.inner(popup);
    Widget::render(block, popup, buf);

    let (summary, github_label) = match conflict.kind {
        ConflictKind::BothEdited => ("This comment was edited here and on GitHub.", "On GitHub:"),
        ConflictKind::DeletedOnGithub => (
            "This comment was deleted on GitHub but edited here.",
            "On GitHub:  (deleted)",
        ),
    };

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {}:{}", conflict.path, conflict.line),
            Theme::review_bar_label(),
        )),
        Line::from(Span::styled(
            format!("  {summary}"),
            Theme::review_bar_label(),
        )),
        Line::default(),
    ];
    lines.extend(body_lines("Here:", &conflict.local_body));
    lines.push(Line::default());
    match &conflict.remote_body {
        Some(body) => lines.extend(body_lines(github_label, body)),
        None => lines.push(Line::from(Span::styled(
            format!("  {github_label}"),
            Theme::review_bar_key(),
        ))),
    }
    lines.push(Line::default());

    let (keep_local, keep_github) = match conflict.kind {
        ConflictKind::BothEdited => (" keep mine  ", " keep GitHub's  "),
        ConflictKind::DeletedOnGithub => (" re-create on GitHub  ", " accept deletion  "),
    };
    lines.push(Line::from(vec![
        Span::styled("  l", Theme::review_bar_key()),
        Span::styled(keep_local, Theme::review_bar_label()),
        Span::styled("g", Theme::review_bar_key()),
        Span::styled(keep_github, Theme::review_bar_label()),
        Span::styled("Esc", Theme::review_bar_key()),
        Span::styled(" decide later", Theme::review_bar_label()),
    ]));

    Widget::render(Paragraph::new(lines).wrap(Wrap { trim: false }), inner, buf);
}
