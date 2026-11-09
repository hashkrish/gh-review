//! The comments panel: every review thread in the PR plus your pending
//! drafts, with the actions that apply to each.
//!
//! | entry  | jump | reply | resolve | edit | discard | open | copy |
//! |--------|------|-------|---------|------|---------|------|------|
//! | thread | yes  | yes   | yes     | no   | no      | yes  | yes  |
//! | draft  | yes  | no    | no      | yes  | yes     | no   | yes  |
//!
//! The list is rebuilt from the app's comment data in `rebuild_display`, so
//! resolving, replying and editing show up without any extra bookkeeping.

use crate::components::comments_panel::{Anchor, CommentEntry, EntryKind, build_entries};
use crate::diff::renderer::DisplayRow;

use super::{App, Focus};

/// What resolving `entry` would toggle: its thread id and current state.
fn resolve_target(entry: &CommentEntry) -> Result<(String, bool), &'static str> {
    if entry.anchor == Anchor::Conversation {
        return Err("Conversation comments aren't review threads, so they can't be resolved");
    }
    match (entry.kind, &entry.thread_node_id) {
        (EntryKind::Draft, _) => Err("Drafts can't be resolved; they're not published yet"),
        (EntryKind::Thread, Some(id)) => Ok((id.clone(), entry.is_resolved)),
        (EntryKind::Thread, None) => Err("This comment's thread isn't loaded yet; try again"),
    }
}

/// Who and what a reply to `entry` goes to.
fn reply_target(entry: &CommentEntry) -> Result<(u64, String), &'static str> {
    if entry.anchor == Anchor::Conversation {
        return Err("Reply to conversation comments on GitHub; replies here go to review threads");
    }
    match (entry.kind, entry.root_id) {
        (EntryKind::Thread, Some(root)) => Ok((root, entry.author.clone())),
        _ => Err("Drafts can't be replied to; submit the review first"),
    }
}

/// The thread's page on GitHub. Drafts have none until they're submitted.
fn comment_url(repo: &str, pr: u64, entry: &CommentEntry) -> Option<String> {
    match (entry.kind, entry.root_id) {
        (EntryKind::Thread, Some(id)) if entry.anchor == Anchor::Conversation => Some(format!(
            "https://github.com/{repo}/pull/{pr}#issuecomment-{id}"
        )),
        (EntryKind::Thread, Some(root)) => Some(format!(
            "https://github.com/{repo}/pull/{pr}#discussion_r{root}"
        )),
        _ => None,
    }
}

impl App {
    /// Rebuild the panel's list from the current comment data.
    pub(crate) fn refresh_comments_panel(&mut self) {
        let entries = build_entries(
            &self.existing_comments,
            &self.pending_comments,
            &self.thread_map,
        );
        self.comments_panel.set_entries(entries);
    }

    pub(crate) fn toggle_comments_panel(&mut self) {
        if self.comments_panel.visible {
            self.comments_panel.visible = false;
            if self.focus == Focus::Comments {
                self.focus = Focus::DiffView;
            }
        } else {
            // The right-hand drawer holds one panel at a time.
            self.description_panel.visible = false;
            self.commits_panel.visible = false;
            self.comments_panel.visible = true;
            self.focus = Focus::Comments;
            self.refresh_comments_panel();
        }
    }

    fn selected_comment(&mut self) -> Option<CommentEntry> {
        let entry = self.comments_panel.selected_entry().cloned();
        if entry.is_none() {
            self.status.info("No comment selected");
        }
        entry
    }

    /// The diff row of `entry`'s header, if it's currently displayed.
    fn comment_row(&self, entry: &CommentEntry) -> Option<usize> {
        self.diff_view
            .display_rows
            .iter()
            .position(|row| match row {
                DisplayRow::CommentHeader {
                    is_reply: false,
                    is_pending,
                    thread_root_id,
                    pending_idx,
                    ..
                } => match entry.kind {
                    EntryKind::Thread => !is_pending && *thread_root_id == entry.root_id,
                    EntryKind::Draft => *is_pending && *pending_idx == entry.draft_idx,
                },
                _ => false,
            })
    }

    /// Enter: put the diff cursor on the selected comment and focus the diff.
    pub(crate) fn jump_to_selected_comment(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        // Comments only exist in the full PR diff.
        self.leave_commit_view();

        // A folded file hides its comments; open it.
        if self.comment_row(&entry).is_none()
            && let Some(file_idx) = self.files.iter().position(|f| f.path == entry.path)
            && self.diff_view.collapsed_files.remove(&file_idx)
        {
            self.rebuild_display();
        }

        match self.comment_row(&entry) {
            Some(row) => {
                self.diff_view.cursor = row;
                self.focus = Focus::DiffView;
            }
            None => self.status.info("Couldn't find that comment in the diff"),
        }
    }

    pub(crate) fn reply_to_selected_comment(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        match reply_target(&entry) {
            Ok((root_id, author)) => self.comment_input.open_reply(root_id, author),
            Err(why) => self.status.info(why),
        }
    }

    pub(crate) fn resolve_selected_comment(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        match resolve_target(&entry) {
            Ok((thread_node_id, is_resolved)) => {
                self.toggle_resolve_thread(thread_node_id, is_resolved)
            }
            Err(why) => self.status.info(why),
        }
    }

    pub(crate) fn edit_selected_draft(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        let draft = entry
            .draft_idx
            .and_then(|idx| self.pending_comments.get(idx).map(|c| (idx, c)));
        match draft {
            Some((idx, c)) => {
                self.comment_input
                    .open_edit(idx, c.path.clone(), c.line, c.side, &c.body)
            }
            None => self.status.info("Only your drafts can be edited here"),
        }
    }

    pub(crate) fn discard_selected_draft(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        match entry.draft_idx {
            Some(idx) => {
                self.remove_draft(idx);
                self.rebuild_display();
                self.status.info("Draft discarded");
            }
            None => self.status.info("Only your drafts can be discarded"),
        }
    }

    pub(crate) fn open_selected_comment(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        match comment_url(&self.repo, self.pr_number, &entry) {
            Some(url) => {
                if let Err(e) = open::that(&url) {
                    self.status.error(format!("Failed to open browser: {e}"));
                }
            }
            None => self
                .status
                .info("Drafts have no page on GitHub until the review is submitted"),
        }
    }

    pub(crate) fn copy_selected_comment(&mut self) {
        let Some(entry) = self.selected_comment() else {
            return;
        };
        match crate::clipboard::copy(&entry.body) {
            Ok(via) => self.status.success(format!("Copied comment text ({via})")),
            Err(e) => self.status.error(format!("Couldn't copy: {e}")),
        }
    }
}

#[cfg(test)]
mod tests;
