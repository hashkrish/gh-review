use ratatui::{buffer::Buffer, layout::Rect};

use super::*;

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

fn render(files: &[DiffFile]) -> Buffer {
    let mut picker = FilePicker::new();
    picker.set_files(files);
    let area = Rect::new(0, 0, 40, 6);
    let mut buf = Buffer::empty(area);
    picker.draw(area, &mut buf, true);
    buf
}

/// Column of the first cell on `row` showing `symbol`.
fn column_of(buf: &Buffer, row: u16, symbol: &str) -> Option<u16> {
    (0..buf.area.width).find(|&x| buf[(x, row)].symbol() == symbol)
}

#[test]
fn paths_align_whether_or_not_a_file_is_viewed() {
    let buf = render(&[file("alpha.rs", true), file("beta.rs", false)]);
    // Rows 1 and 2 (row 0 is the border).
    assert_eq!(column_of(&buf, 1, "a"), column_of(&buf, 2, "b"));
}

#[test]
fn viewed_marker_is_drawn_only_on_viewed_rows() {
    let buf = render(&[file("alpha.rs", true), file("beta.rs", false)]);
    assert!(column_of(&buf, 1, "✓").is_some());
    assert!(column_of(&buf, 2, "✓").is_none());
}

#[test]
fn viewed_marker_follows_the_status_symbol() {
    let buf = render(&[file("alpha.rs", true)]);
    let status = column_of(&buf, 1, "M").unwrap();
    // "M", a space, then the tick.
    assert_eq!(column_of(&buf, 1, "✓"), Some(status + 2));
}

#[test]
fn path_starts_after_the_marker_slot() {
    let buf = render(&[file("alpha.rs", true)]);
    let tick = column_of(&buf, 1, "✓").unwrap();
    // The slot is two cells wide: the tick and one space before the path.
    assert_eq!(column_of(&buf, 1, "a"), Some(tick + 2));
}
