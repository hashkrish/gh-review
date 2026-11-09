use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CommentState {
    pub is_pending: bool,
    pub is_resolved: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum RowContext {
    File,
    Code,
    Comment(CommentState),
    Suggestion(CommentState),
}

impl RowContext {
    /// Coarse match for key dispatch: ignores inner CommentState.
    pub fn matches(self, binding_ctx: RowContext) -> bool {
        use RowContext::*;
        matches!(
            (self, binding_ctx),
            (File, File)
                | (Code, Code)
                | (Comment(_), Comment(_))
                | (Suggestion(_), Suggestion(_))
                | (Suggestion(_), Comment(_))
        )
    }

    pub fn comment_state(self) -> CommentState {
        match self {
            RowContext::Comment(cs) | RowContext::Suggestion(cs) => cs,
            _ => CommentState::default(),
        }
    }

    /// Convenience for binding definitions that target any comment row.
    pub const COMMENT: Self = Self::Comment(CommentState {
        is_pending: false,
        is_resolved: false,
    });

    /// Convenience for binding definitions that target any suggestion row.
    pub const SUGGESTION: Self = Self::Suggestion(CommentState {
        is_pending: false,
        is_resolved: false,
    });
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
}

impl FileStatus {
    pub fn from_str(s: &str) -> Self {
        match s {
            "added" => Self::Added,
            "removed" => Self::Deleted,
            "renamed" => Self::Renamed,
            "copied" => Self::Copied,
            _ => Self::Modified,
        }
    }

    pub fn symbol(&self) -> &str {
        match self {
            Self::Added => "A",
            Self::Modified => "M",
            Self::Deleted => "D",
            Self::Renamed => "R",
            Self::Copied => "C",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiffFile {
    pub path: String,
    pub status: FileStatus,
    pub additions: usize,
    pub deletions: usize,
    pub hunks: Vec<Hunk>,
    /// GitHub's per-file "Viewed" checkbox for the current user.
    pub viewed: bool,
}

#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: String,
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone)]
pub struct DiffLine {
    pub kind: LineKind,
    pub old_lineno: Option<usize>,
    pub new_lineno: Option<usize>,
    pub content: String,
    pub highlighted_content: Option<ratatui::text::Line<'static>>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Left,
    Right,
}

#[derive(Debug, Clone)]
pub struct ReviewComment {
    pub path: String,
    pub line: usize,
    pub side: Side,
    pub body: String,
    pub start_line: Option<usize>,
    pub start_side: Option<Side>,
    /// Stable id for this draft within the session; lets async sync results
    /// find the draft even after edits or deletions shift list indices.
    pub local_id: u64,
    /// GraphQL node id of the matching comment on GitHub's pending review,
    /// once the draft has been pushed (or imported).
    pub node_id: Option<String>,
    /// Body as last seen identical on both sides. Comparing it with the local
    /// and remote bodies tells us which side has changed since the last sync.
    pub synced_body: Option<String>,
}

impl ReviewComment {
    /// A new, not-yet-synced draft. `App::add_draft` assigns `local_id`.
    pub fn new_local(
        path: String,
        line: usize,
        side: Side,
        body: String,
        start_line: Option<usize>,
        start_side: Option<Side>,
    ) -> Self {
        Self {
            path,
            line,
            side,
            body,
            start_line,
            start_side,
            local_id: 0,
            node_id: None,
            synced_body: None,
        }
    }
}

/// An inline comment on the viewer's pending review as stored on GitHub.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteDraft {
    pub node_id: String,
    pub path: String,
    pub line: usize,
    pub side: Side,
    pub body: String,
    pub start_line: Option<usize>,
    pub start_side: Option<Side>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReviewEvent {
    Approve,
    RequestChanges,
    Comment,
    Unapprove,
}

impl ReviewEvent {
    pub fn as_api_str(&self) -> &str {
        match self {
            Self::Approve => "APPROVE",
            Self::RequestChanges => "REQUEST_CHANGES",
            Self::Comment => "COMMENT",
            Self::Unapprove => "DISMISS",
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::Approve => "Approve",
            Self::RequestChanges => "Request Changes",
            Self::Comment => "Comment",
            Self::Unapprove => "Unapprove",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DiffMode {
    Unified,
    SideBySide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewState {
    Approved,
    ChangesRequested,
    Pending,
    Commented,
    Dismissed,
}

impl ReviewState {
    pub fn from_str(s: &str) -> Self {
        match s {
            "APPROVED" => Self::Approved,
            "CHANGES_REQUESTED" => Self::ChangesRequested,
            "COMMENTED" => Self::Commented,
            "DISMISSED" => Self::Dismissed,
            _ => Self::Pending,
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Approved => "✓",
            Self::ChangesRequested => "✗",
            Self::Pending => "●",
            Self::Commented => "○",
            Self::Dismissed => "─",
        }
    }

    pub fn color(self) -> ratatui::style::Color {
        use ratatui::style::Color;
        match self {
            Self::Approved => Color::Green,
            Self::ChangesRequested => Color::Red,
            Self::Pending => Color::DarkGray,
            Self::Commented => Color::Yellow,
            Self::Dismissed => Color::DarkGray,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ReviewerInfo {
    pub login: String,
    pub state: ReviewState,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PrMetadata {
    pub number: u64,
    pub title: String,
    pub body: Option<String>,
    pub state: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default, rename = "reviewDecision")]
    pub review_decision: Option<String>,
    pub head: PrRef,
    pub base: PrRef,
    pub user: PrUser,
    pub additions: Option<usize>,
    pub deletions: Option<usize>,
    pub changed_files: Option<usize>,
    #[serde(skip)]
    pub reviewers: Vec<ReviewerInfo>,
}

#[derive(Debug, Deserialize)]
pub struct PrRef {
    pub sha: String,
    #[serde(rename = "ref")]
    pub ref_name: String,
}

#[derive(Debug, Deserialize)]
pub struct PrUser {
    pub login: String,
}

#[derive(Debug, Deserialize)]
pub struct GhFile {
    pub filename: String,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
    pub patch: Option<String>,
}

/// What the diff panel is showing.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum DiffSource {
    /// The whole PR (the normal, reviewable view).
    #[default]
    Full,
    /// A single commit's changes, by SHA. Read-only.
    Commit(String),
    /// Everything after a commit up to the PR head, by SHA. Read-only.
    Since(String),
}

impl DiffSource {
    pub fn is_full(&self) -> bool {
        matches!(self, Self::Full)
    }

    /// Short description for status messages and the title bar.
    pub fn label(&self) -> String {
        let short = |sha: &str| sha[..sha.len().min(7)].to_string();
        match self {
            Self::Full => "full PR diff".to_string(),
            Self::Commit(sha) => format!("commit {}", short(sha)),
            Self::Since(sha) => format!("changes since {}", short(sha)),
        }
    }
}

/// A commit in a PR, as shown in the commits panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Commit {
    pub sha: String,
    /// First line of the commit message.
    pub headline: String,
    pub author: String,
    /// ISO 8601 timestamp, e.g. `2026-11-09T07:21:52Z`.
    pub date: String,
}

impl Commit {
    pub fn short_sha(&self) -> &str {
        &self.sha[..self.sha.len().min(7)]
    }
}

/// `GET /repos/{repo}/pulls/{n}/commits` item (only the fields we use).
#[derive(Debug, Deserialize)]
pub struct GhCommit {
    pub sha: String,
    pub commit: GhCommitDetail,
    /// The GitHub account, absent when the commit email matches no account.
    pub author: Option<PrUser>,
}

#[derive(Debug, Deserialize)]
pub struct GhCommitDetail {
    pub message: String,
    pub author: Option<GhCommitPerson>,
}

#[derive(Debug, Deserialize)]
pub struct GhCommitPerson {
    pub name: String,
    pub date: String,
}

impl From<GhCommit> for Commit {
    fn from(c: GhCommit) -> Self {
        let (name, date) = c
            .commit
            .author
            .map(|p| (p.name, p.date))
            .unwrap_or_default();
        Self {
            sha: c.sha,
            headline: c.commit.message.lines().next().unwrap_or("").to_string(),
            // Prefer the GitHub login; fall back to the git author name.
            author: c.author.map(|u| u.login).unwrap_or(name),
            date,
        }
    }
}

/// Response of `GET /repos/{repo}/commits/{sha}` and `/compare/{a}...{b}`;
/// both list the changed files the same way.
#[derive(Debug, Deserialize)]
pub struct GhFilesResponse {
    #[serde(default)]
    pub files: Vec<GhFile>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct ExistingComment {
    pub id: u64,
    pub path: String,
    pub line: Option<usize>,
    pub side: Option<String>,
    pub start_line: Option<usize>,
    pub body: String,
    pub user: PrUser,
    #[serde(rename = "created_at")]
    pub created_at: String,
    #[serde(default)]
    pub in_reply_to_id: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct FileContent {
    pub content: String,
    pub encoding: String,
}

#[derive(Debug, Clone)]
pub struct ThreadInfo {
    pub thread_node_id: String,
    pub is_resolved: bool,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
pub struct PrReview {
    pub id: u64,
    pub user: PrUser,
    pub state: String,
    #[serde(default)]
    pub body: Option<String>,
}
