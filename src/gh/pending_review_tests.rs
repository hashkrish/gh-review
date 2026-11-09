use super::*;
use serde_json::json;

fn thread(state: &str) -> serde_json::Value {
    json!({
        "path": "src/a.rs",
        "line": 12,
        "startLine": 10,
        "diffSide": "RIGHT",
        "startDiffSide": "RIGHT",
        "comments": { "nodes": [{
            "id": "PRRC_1",
            "body": "nit",
            "pullRequestReview": { "state": state }
        }]}
    })
}

#[test]
fn parses_pending_thread_into_draft() {
    let d = parse_pending_draft(&thread("PENDING")).unwrap();
    assert_eq!(d.node_id, "PRRC_1");
    assert_eq!(d.path, "src/a.rs");
    assert_eq!((d.line, d.start_line), (12, Some(10)));
    assert_eq!((d.side, d.start_side), (Side::Right, Some(Side::Right)));
    assert_eq!(d.body, "nit");
}

#[test]
fn ignores_published_threads() {
    assert!(parse_pending_draft(&thread("COMMENTED")).is_none());
}

#[test]
fn ignores_threads_without_a_current_line() {
    let mut t = thread("PENDING");
    t["line"] = serde_json::Value::Null;
    assert!(parse_pending_draft(&t).is_none());
}

#[test]
fn thread_input_includes_range_only_when_multiline() {
    let mut c = ReviewComment::new_local("a.rs".into(), 5, Side::Left, "b".into(), None, None);
    let single = draft_thread_input("R1", &c);
    assert_eq!(single["side"], "LEFT");
    assert!(single.get("startLine").is_none());

    c.start_line = Some(3);
    c.start_side = Some(Side::Left);
    let multi = draft_thread_input("R1", &c);
    assert_eq!(multi["startLine"], 3);
    assert_eq!(multi["startSide"], "LEFT");
    assert_eq!(multi["pullRequestReviewId"], "R1");
}
