//! The commits panel and the read-only commit / since views of the diff.
//!
//! The panel lists the PR's commits. From it you can view one commit's diff,
//! view everything since a commit (up to the PR head), open a commit on
//! GitHub, or copy its SHA. While a commit or since view is showing, the
//! full PR diff's files are set aside in `full_files` and the diff is
//! read-only: review comments anchor to the full diff's lines, so they are
//! hidden and commenting is disabled until you return.

use crate::event::AppEvent;
use crate::types::{Commit, DiffFile, DiffSource};

use super::{App, Focus};

impl App {
    /// Run a background task, if there is a runtime (there isn't in unit
    /// tests, which only check state transitions).
    fn spawn_bg<F>(&self, task: F)
    where
        F: std::future::Future<Output = ()> + Send + 'static,
    {
        if tokio::runtime::Handle::try_current().is_ok() {
            tokio::spawn(task);
        }
    }

    pub(crate) fn toggle_commits_panel(&mut self) {
        if self.commits_panel.visible {
            self.commits_panel.visible = false;
            if self.focus == Focus::Commits {
                self.focus = Focus::DiffView;
            }
        } else {
            // The right-hand drawer holds one panel at a time.
            self.description_panel.visible = false;
            self.comments_panel.visible = false;
            self.commits_panel.visible = true;
            self.focus = Focus::Commits;
            if !self.commits_panel.is_loaded() {
                self.request_commits();
            }
        }
    }

    pub(crate) fn request_commits(&self) {
        let tx = self.tx.clone();
        let repo = self.repo.clone();
        let pr = self.pr_number;
        self.spawn_bg(async move {
            let data = crate::gh::fetch_pr_commits(&repo, pr)
                .await
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(AppEvent::CommitsLoaded { pr, data });
        });
    }

    pub(super) fn on_commits_loaded(&mut self, pr: u64, data: Result<Vec<Commit>, String>) {
        if pr != self.pr_number {
            return;
        }
        match data {
            Ok(commits) => self.commits_panel.set_commits(commits),
            Err(error) => {
                self.status
                    .error(format!("Failed to load commits: {error}"));
                self.commits_panel.set_error(error);
            }
        }
    }

    /// Enter on a commit: show only its changes. Enter on the "All changes"
    /// row: return to the full PR diff.
    pub(crate) fn view_selected_commit(&mut self) {
        match self.commits_panel.selected_commit().cloned() {
            None => self.show_full_diff(),
            Some(commit) => self.request_diff(DiffSource::Commit(commit.sha), None),
        }
    }

    /// Show everything after the selected commit, up to the PR head.
    pub(crate) fn view_since_selected(&mut self) {
        let Some(base) = self.commits_panel.selected_commit().cloned() else {
            self.status.info("Select a commit to see changes since it");
            return;
        };
        let Some(head) = self.commits_panel.head_commit().cloned() else {
            return;
        };
        if base.sha == head.sha {
            self.status.info("No changes after the newest commit");
            return;
        }
        self.request_diff(DiffSource::Since(base.sha), Some(head.sha));
    }

    /// Load the files for a commit/since view. `head` is the comparison's end
    /// (only used for `Since`).
    fn request_diff(&mut self, source: DiffSource, head: Option<String>) {
        if self.diff_source == source {
            self.status
                .info(format!("Already showing {}", source.label()));
            return;
        }
        self.pending_source = Some(source.clone());
        self.status.info(format!("Loading {}…", source.label()));

        let tx = self.tx.clone();
        let repo = self.repo.clone();
        let pr = self.pr_number;
        self.spawn_bg(async move {
            let files = match (&source, &head) {
                (DiffSource::Commit(sha), _) => crate::gh::fetch_commit_files(&repo, sha).await,
                (DiffSource::Since(base), Some(head)) => {
                    crate::gh::fetch_compare_files(&repo, base, head).await
                }
                _ => return,
            };
            let _ = tx.send(match files {
                Ok(files) => AppEvent::AltDiffLoaded { pr, source, files },
                Err(e) => AppEvent::Error(format!("Failed to load {}: {e:#}", source.label())),
            });
        });
    }

    pub(super) fn on_alt_diff_loaded(&mut self, pr: u64, source: DiffSource, files: Vec<DiffFile>) {
        // Ignore a response for a view the user has since left or replaced.
        if pr != self.pr_number || self.pending_source.as_ref() != Some(&source) {
            return;
        }
        self.pending_source = None;
        if self.diff_source.is_full() {
            self.full_files = std::mem::take(&mut self.files);
        }
        let count = files.len();
        self.files = files;
        self.diff_source = source;
        self.reset_diff_view();
        self.status.success(format!(
            "Viewing {} (read-only) · {count} file{}",
            self.diff_source.label(),
            if count == 1 { "" } else { "s" }
        ));
    }

    /// Return to the full PR diff.
    pub(crate) fn show_full_diff(&mut self) {
        if self.diff_source.is_full() {
            // Also cancels a commit/since view that is still loading.
            let was_loading = self.pending_source.take().is_some();
            if was_loading {
                self.status.info("Cancelled");
            } else {
                self.status.info("Already showing the full PR diff");
            }
            return;
        }
        self.leave_commit_view();
        self.status.success("Showing the full PR diff");
    }

    /// Restore the full PR diff if a commit/since view is showing (or loading).
    pub(crate) fn leave_commit_view(&mut self) {
        self.pending_source = None;
        if self.diff_source.is_full() {
            return;
        }
        self.files = std::mem::take(&mut self.full_files);
        self.diff_source = DiffSource::Full;
        self.reset_diff_view();
        // Files marked viewed fold, as when the PR first loaded.
        for (idx, file) in self.files.iter().enumerate() {
            if file.viewed {
                self.diff_view.collapsed_files.insert(idx);
            }
        }
        self.rebuild_display();
    }

    /// Start the diff and file list over for a new set of files.
    fn reset_diff_view(&mut self) {
        self.diff_view.collapsed_files.clear();
        self.diff_view.cursor = 0;
        self.file_picker.set_files(&self.files);
        self.file_picker.selected = 0;
        self.rebuild_display();
    }

    pub(crate) fn commit_url(&self, sha: &str) -> String {
        format!("https://github.com/{}/commit/{sha}", self.repo)
    }

    pub(crate) fn open_selected_commit(&mut self) {
        let Some(commit) = self.commits_panel.selected_commit() else {
            self.status.info("Select a commit to open it on GitHub");
            return;
        };
        let url = self.commit_url(&commit.sha);
        if let Err(e) = open::that(&url) {
            self.status.error(format!("Failed to open browser: {e}"));
        }
    }

    pub(crate) fn copy_selected_sha(&mut self) {
        let Some(commit) = self.commits_panel.selected_commit() else {
            self.status.info("Select a commit to copy its SHA");
            return;
        };
        let (sha, short) = (commit.sha.clone(), commit.short_sha().to_string());
        match crate::clipboard::copy(&sha) {
            Ok(via) => self.status.success(format!("Copied {short} ({via})")),
            Err(e) => self.status.error(format!("Couldn't copy: {e}")),
        }
    }

    /// For actions that only make sense on the full PR diff (commenting,
    /// viewed marks, expanding context). Returns `true`, with an explanation
    /// in the status line, when a commit/since view is showing.
    pub(crate) fn read_only_guard(&mut self) -> bool {
        if self.diff_source.is_full() {
            return false;
        }
        let key = self.keymap.key_label("toggle_commits");
        self.status.error(format!(
            "Read-only: showing {}. Open the commits panel ({key}) and pick \"All changes\" to return.",
            self.diff_source.label()
        ));
        true
    }
}

#[cfg(test)]
mod tests;
