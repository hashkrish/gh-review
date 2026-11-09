use ratatui::{buffer::Buffer, layout::Rect};

use super::*;

fn commit(sha: &str, headline: &str) -> Commit {
    Commit {
        sha: sha.to_string(),
        headline: headline.to_string(),
        author: "alice".to_string(),
        date: "2026-11-09T07:00:00Z".to_string(),
    }
}

fn panel_with(n: usize) -> CommitsPanel {
    let mut p = CommitsPanel::new();
    p.set_commits(
        (0..n)
            .map(|i| commit(&format!("{i:040x}"), &format!("change {i}")))
            .collect(),
    );
    p
}

/// Text of every row of the buffer, trimmed, joined by newlines.
fn screen(buf: &Buffer) -> String {
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

fn render(panel: &mut CommitsPanel, active: &DiffSource, height: u16) -> String {
    // 2026-11-09T09:00:00Z, two hours after the sample commits.
    let now = parse_iso_utc("2026-11-09T09:00:00Z").unwrap();
    let area = Rect::new(0, 0, 40, height);
    let mut buf = Buffer::empty(area);
    panel.draw(area, &mut buf, true, active, now);
    screen(&buf)
}

#[test]
fn parses_iso_timestamps() {
    assert_eq!(parse_iso_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(parse_iso_utc("1970-01-02T00:00:01Z"), Some(86_401));
    assert_eq!(parse_iso_utc("2000-03-01T00:00:00Z"), Some(951_868_800));
    assert_eq!(parse_iso_utc("not a date"), None);
}

#[test]
fn age_labels_pick_the_largest_unit() {
    let then = "2026-01-01T00:00:00Z";
    let at = |secs: i64| age_label(then, parse_iso_utc(then).unwrap() + secs);
    assert_eq!(at(10), "now");
    assert_eq!(at(5 * 60), "5m");
    assert_eq!(at(3 * 3_600), "3h");
    assert_eq!(at(2 * 86_400), "2d");
    assert_eq!(at(60 * 86_400), "2mo");
    assert_eq!(at(400 * 86_400), "1y");
    assert_eq!(at(-100), "now", "future timestamps clamp to now");
    assert_eq!(age_label("garbage", 0), "");
}

#[test]
fn row_zero_is_the_full_diff_and_has_no_commit() {
    let mut p = panel_with(3);
    assert!(p.selected_commit().is_none());
    p.next();
    assert_eq!(p.selected_commit().unwrap().headline, "change 0");
    p.goto_last();
    assert_eq!(p.selected_commit().unwrap().headline, "change 2");
    p.goto_first();
    assert!(p.selected_commit().is_none());
}

#[test]
fn navigation_stops_at_both_ends() {
    let mut p = panel_with(2);
    p.prev();
    assert!(p.selected_commit().is_none());
    for _ in 0..10 {
        p.next();
    }
    assert_eq!(p.selected_commit().unwrap().headline, "change 1");
}

#[test]
fn head_commit_is_the_newest() {
    assert_eq!(panel_with(3).head_commit().unwrap().headline, "change 2");
    assert!(CommitsPanel::new().head_commit().is_none());
}

#[test]
fn lists_commits_with_sha_headline_author_and_age() {
    let mut p = panel_with(2);
    let out = render(&mut p, &DiffSource::Full, 10);
    assert!(out.contains("Commits (2)"), "{out}");
    assert!(out.contains("▸ All changes"), "{out}");
    assert!(out.contains("0000000 change 0"), "{out}");
    assert!(out.contains("alice · 2h"), "{out}");
}

#[test]
fn marks_the_commit_being_viewed() {
    let mut p = panel_with(2);
    let sha = p.commits()[1].sha.clone();
    let out = render(&mut p, &DiffSource::Commit(sha), 10);
    assert!(out.contains("viewing 0000000"), "{out}");
    assert!(out.contains("● 0000000 change 1"), "{out}");
    assert!(
        !out.contains("▸ All changes"),
        "full row must not be marked"
    );
}

#[test]
fn marks_the_base_of_a_since_view() {
    let mut p = panel_with(3);
    let sha = p.commits()[0].sha.clone();
    let out = render(&mut p, &DiffSource::Since(sha), 12);
    assert!(out.contains("since 0000000"), "{out}");
    assert!(out.contains("↧ 0000000 change 0"), "{out}");
}

#[test]
fn shows_loading_and_error_states() {
    let mut p = CommitsPanel::new();
    assert!(render(&mut p, &DiffSource::Full, 6).contains("Loading commits"));
    p.set_error("boom".into());
    assert!(render(&mut p, &DiffSource::Full, 6).contains("boom"));
}

#[test]
fn scrolls_to_keep_the_selection_visible() {
    let mut p = panel_with(10);
    p.select_row(10);
    // 8 lines tall = 6 inner = 3 rows.
    let out = render(&mut p, &DiffSource::Full, 8);
    assert!(out.contains("change 9"), "{out}");
    assert!(
        !out.contains("All changes"),
        "top rows scrolled away: {out}"
    );
    p.select_row(0);
    assert!(render(&mut p, &DiffSource::Full, 8).contains("All changes"));
}

#[test]
fn truncates_long_headlines_with_an_ellipsis() {
    let mut p = CommitsPanel::new();
    p.set_commits(vec![commit(
        &"a".repeat(40),
        "a very long commit headline that cannot possibly fit in the drawer",
    )]);
    let out = render(&mut p, &DiffSource::Full, 10);
    assert!(out.contains('…'), "{out}");
    assert!(out.lines().all(|l| l.chars().count() <= 40), "{out}");
}

#[test]
fn paging_moves_by_the_rows_that_fit_and_clamps() {
    let mut p = panel_with(40);
    // 8 lines tall = 6 inner lines = 3 rows per page.
    render(&mut p, &DiffSource::Full, 8);

    p.page_down(false);
    assert_eq!(p.selected_row(), 3, "a full page is every visible row");
    p.page_down(true);
    assert_eq!(p.selected_row(), 4, "a half page is half of them");
    p.page_up(true);
    p.page_up(false);
    assert_eq!(p.selected_row(), 0);
    p.page_up(false);
    assert_eq!(p.selected_row(), 0, "clamps at the top");

    for _ in 0..30 {
        p.page_down(false);
    }
    assert_eq!(p.selected_row(), 40, "clamps at the last commit");
}

#[test]
fn a_half_page_always_moves_at_least_one_row() {
    let mut p = panel_with(10);
    // Only one row fits: half of that rounds to zero without the floor.
    render(&mut p, &DiffSource::Full, 4);
    p.page_down(true);
    assert_eq!(p.selected_row(), 1);
}

#[test]
fn reset_forgets_commits() {
    let mut p = panel_with(3);
    p.reset();
    assert!(!p.is_loaded());
    assert!(p.commits().is_empty());
}
