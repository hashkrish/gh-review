//! Local persistence for unsubmitted (pending) review comments, so drafts
//! survive quitting and reopening a PR.
//!
//! Stored as JSON at `<state_dir>/drafts/<owner>/<repo>/<pr>.json`.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::types::ReviewComment;

fn draft_path(root: &Path, repo: &str, pr: u64) -> PathBuf {
    let mut path = root.join("drafts");
    for part in repo.split('/').filter(|p| !p.is_empty() && *p != "..") {
        path.push(part);
    }
    path.join(format!("{pr}.json"))
}

fn load_from(root: &Path, repo: &str, pr: u64) -> Result<Vec<ReviewComment>> {
    let path = draft_path(root, repo, pr);
    match std::fs::read_to_string(&path) {
        Ok(json) => Ok(serde_json::from_str(&json)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.into()),
    }
}

fn save_to(root: &Path, repo: &str, pr: u64, comments: &[ReviewComment]) -> Result<()> {
    let path = draft_path(root, repo, pr);
    if comments.is_empty() {
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(e.into()),
            _ => return Ok(()),
        }
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Write to a temp file and rename so a crash never leaves a truncated draft.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(comments)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

pub fn load(repo: &str, pr: u64) -> Result<Vec<ReviewComment>> {
    load_from(&crate::dirs::state_dir(), repo, pr)
}

pub fn save(repo: &str, pr: u64, comments: &[ReviewComment]) -> Result<()> {
    save_to(&crate::dirs::state_dir(), repo, pr, comments)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Side;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-review-drafts-test-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn comment(body: &str) -> ReviewComment {
        ReviewComment {
            path: "src/main.rs".into(),
            line: 10,
            side: Side::Right,
            body: body.into(),
            start_line: Some(8),
            start_side: Some(Side::Left),
        }
    }

    #[test]
    fn missing_draft_loads_empty() {
        let root = temp_root("missing");
        assert!(load_from(&root, "o/r", 1).unwrap().is_empty());
    }

    #[test]
    fn round_trip_and_clear() {
        let root = temp_root("roundtrip");
        let comments = vec![comment("first"), comment("second ✓")];
        save_to(&root, "o/r", 7, &comments).unwrap();

        let loaded = load_from(&root, "o/r", 7).unwrap();
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[1].body, "second ✓");
        assert_eq!(loaded[0].start_side, Some(Side::Left));
        // Other PRs are unaffected.
        assert!(load_from(&root, "o/r", 8).unwrap().is_empty());

        save_to(&root, "o/r", 7, &[]).unwrap();
        assert!(!draft_path(&root, "o/r", 7).exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
