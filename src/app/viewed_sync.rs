//! Two-way sync of GitHub's per-file "Viewed" checkbox.
//!
//! Toggling a file updates it locally (and folds it, like github.com does),
//! then pushes the change through the sync worker. GitHub's state is pulled
//! after the file list loads and with `:sync_viewed`. Marking a file viewed
//! folds it; unmarking unfolds it, whichever side made the change.

use std::collections::{HashMap, HashSet};

use crate::sync::SyncOp;
use crate::types::DiffFile;

use super::{App, Focus};

/// Apply GitHub's viewed states to `files`, skipping files whose own toggle is
/// still in flight (their state is newer than anything GitHub reports).
/// Returns `(file_idx, now_viewed)` for each file whose state changed.
fn apply_viewed_states(
    files: &mut [DiffFile],
    states: &HashMap<String, bool>,
    in_flight: &HashSet<String>,
) -> Vec<(usize, bool)> {
    let mut changed = Vec::new();
    for (idx, file) in files.iter_mut().enumerate() {
        if in_flight.contains(&file.path) {
            continue;
        }
        let viewed = states.get(&file.path).copied().unwrap_or(false);
        if viewed != file.viewed {
            file.viewed = viewed;
            changed.push((idx, viewed));
        }
    }
    changed
}

impl App {
    fn files_mut(&mut self, pr: u64) -> Option<&mut Vec<DiffFile>> {
        if pr == self.pr_number {
            Some(&mut self.files)
        } else {
            self.pr_cache.get_mut(pr).map(|s| &mut s.files)
        }
    }

    fn set_fold(&mut self, file_idx: usize, folded: bool) {
        if folded {
            self.diff_view.collapsed_files.insert(file_idx);
        } else {
            self.diff_view.collapsed_files.remove(&file_idx);
        }
    }

    fn refresh_file_views(&mut self) {
        self.file_picker.set_files(&self.files);
        self.rebuild_display();
    }

    pub(crate) fn request_viewed_pull(&self, pr: u64) {
        self.enqueue(SyncOp::PullViewed { pr });
    }

    /// Toggle "Viewed" on the file under the cursor (or selected in the file
    /// list) and push it to GitHub.
    pub(crate) fn toggle_file_viewed(&mut self) {
        let idx = if self.focus == Focus::FilePicker {
            Some(self.file_picker.selected)
        } else {
            self.diff_view.current_file_idx()
        };
        let Some(file) = idx.and_then(|i| self.files.get_mut(i)) else {
            return;
        };
        file.viewed = !file.viewed;
        let (viewed, path) = (file.viewed, file.path.clone());
        let idx = idx.unwrap_or_default();

        self.set_fold(idx, viewed);
        *self
            .viewed_inflight
            .entry((self.pr_number, path.clone()))
            .or_insert(0) += 1;
        self.enqueue(SyncOp::SetViewed {
            pr: self.pr_number,
            path,
            viewed,
        });
        self.refresh_file_views();
    }

    fn finish_viewed_push(&mut self, pr: u64, path: &str) -> usize {
        let key = (pr, path.to_string());
        let remaining = self.viewed_inflight.get(&key).copied().unwrap_or(1) - 1;
        if remaining == 0 {
            self.viewed_inflight.remove(&key);
        } else {
            self.viewed_inflight.insert(key, remaining);
        }
        remaining
    }

    pub(super) fn on_viewed_set(&mut self, pr: u64, path: String) {
        self.finish_viewed_push(pr, &path);
    }

    pub(super) fn on_viewed_set_failed(
        &mut self,
        pr: u64,
        path: String,
        attempted: bool,
        error: String,
    ) {
        let remaining = self.finish_viewed_push(pr, &path);
        self.status
            .error(format!("Couldn't update viewed state on GitHub: {error}"));
        // Revert, unless a newer toggle for this file is still in flight (it
        // decides the final state).
        if remaining > 0 {
            return;
        }
        let Some(files) = self.files_mut(pr) else {
            return;
        };
        let Some((idx, file)) = files
            .iter_mut()
            .enumerate()
            .find(|(_, f)| f.path == path && f.viewed == attempted)
        else {
            return;
        };
        file.viewed = !attempted;
        if pr == self.pr_number {
            self.set_fold(idx, !attempted);
            self.refresh_file_views();
        }
    }

    pub(super) fn on_viewed_pulled(&mut self, pr: u64, states: HashMap<String, bool>) {
        let in_flight: HashSet<String> = self
            .viewed_inflight
            .keys()
            .filter(|(p, _)| *p == pr)
            .map(|(_, path)| path.clone())
            .collect();
        let Some(files) = self.files_mut(pr) else {
            return;
        };
        let changed = apply_viewed_states(files, &states, &in_flight);
        if pr != self.pr_number || changed.is_empty() {
            return;
        }
        for (idx, viewed) in changed {
            self.set_fold(idx, viewed);
        }
        self.refresh_file_views();
    }
}

#[cfg(test)]
mod tests;
