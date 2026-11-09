use super::*;
use crate::types::{DiffLine, LineKind};

fn diff_row(kind: LineKind) -> DisplayRow {
    DisplayRow::DiffLine {
        line: DiffLine {
            kind,
            old_lineno: Some(3),
            new_lineno: Some(3),
            content: "let x = 1;".to_string(),
            highlighted_content: None,
        },
        file_idx: 0,
        hunk_idx: 0,
        line_idx: 0,
    }
}

fn text(line: &Line<'_>) -> String {
    line.spans.iter().map(|s| s.content.as_ref()).collect()
}

#[test]
fn selecting_a_line_does_not_shift_its_text() {
    for kind in [LineKind::Added, LineKind::Removed, LineKind::Context] {
        let row = diff_row(kind.clone());
        let plain = render_unified_row(&row, &[], 80, false);
        let selected = render_unified_row(&row, &[], 80, true);
        assert_eq!(text(&plain), text(&selected), "{kind:?}");
    }
}

#[test]
fn selected_line_is_still_highlighted() {
    let row = diff_row(LineKind::Added);
    let selected = render_unified_row(&row, &[], 80, true);
    let bg = Theme::selected_line().bg;
    assert!(selected.spans.iter().all(|s| s.style.bg == bg));
}

#[test]
fn selected_file_header_does_not_gain_a_leading_marker() {
    let row = DisplayRow::FileHeader {
        path: "a.rs".into(),
        file_idx: 0,
        collapsed: false,
        viewed: false,
    };
    let selected = render_unified_row(&row, &[], 80, true);
    assert!(text(&selected).starts_with('▼'));
}
