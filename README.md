# gh-review

Terminal UI for reviewing GitHub pull requests. View diffs (unified and side-by-side), comment on lines, suggest changes, resolve threads, expand context, and approve — all without leaving the terminal.

![gh-review screenshot](docs/screenshot.png)

## Install

```bash
cargo install gh-review
```

Requires the [GitHub CLI](https://cli.github.com/) (`gh`) to be installed and authenticated.

## Usage

```bash
gh-review <OWNER/REPO> <PR_NUMBER>
gh-review <URL>
gh-review <PR_NUMBER>
```

```bash
gh-review octocat/hello-world 42
```

Accepts GitHub and Graphite PR URLs. When given just a PR number, the repository is inferred from `git remote get-url origin`.

### With gh-dash

[gh-dash](https://github.com/dlvhdr/gh-dash) is a terminal dashboard for GitHub PRs, issues, and notifications. gh-review is designed to complement it — use gh-dash to browse and triage, press a key to jump into gh-review for deep code review.

#### Setup

1. Install gh-dash if you haven't already:

   ```bash
   gh extension install dlvhdr/gh-dash
   ```

2. Add a custom keybinding to `~/.config/gh-dash/config.yml`:

   ```yaml
   keybindings:
     prs:
       - key: R
         name: review (gh-review)
         command: >
           gh-review {{.RepoName}} {{.PrNumber}}
   ```

   `{{.RepoName}}` and `{{.PrNumber}}` are template variables that gh-dash fills in with the currently selected PR.

3. Run gh-dash:

   ```bash
   gh dash
   ```

4. Navigate to a PR and press `R`. gh-dash suspends and gh-review takes over the terminal. When you quit gh-review (`q`), you're back in gh-dash.

#### Workflow

```
gh-dash (browse PRs)
  │
  ├─ R  → gh-review (diff, comment, approve)
  ├─ D  → delta side-by-side diff (quick read-only view)
  ├─ d  → unified diff in pager
  └─ V  → approve directly
```

## Keybindings

### Navigation

| Key | Action |
|-----|--------|
| `j` / `k` / `↑` / `↓` | Scroll line (works in all panels) |
| `gg` / `G` | Go to first / last line |
| `Ctrl+D` / `Ctrl+U` | Half page down / up |
| `Ctrl+F` / `Ctrl+B` | Full page down / up |
| `Space` / `Shift+Space` | Full page down / up (`Shift+Space` needs a terminal with the kitty keyboard protocol, e.g. kitty, Ghostty, WezTerm, foot) |
| `H` / `M` / `L` | Cursor to screen top / middle / bottom |
| `]` / `}` | Next hunk (diff) / next section (description) |
| `[` / `{` | Previous hunk (diff) / previous section (description) |
| `)` / `(` | Next / previous change |
| `gc` / `gC` | Next / previous comment thread |
| `n` / `N` | Next / previous file (or search match when search active) |
| `zo` / `zc` | Expand / collapse file at cursor |
| `Enter` | Toggle comment expand or file fold |
| `zz` / `zt` / `zb` | Center / top / bottom cursor in viewport |

### Panels

| Key | Action |
|-----|--------|
| `h` / `←` | Focus previous panel |
| `l` / `→` | Focus next panel |
| `Tab` | Cycle panel focus |
| `Enter` | In the file list: move focus to the diff |
| `Esc` | Close description panel (when focused) |
| `b` | Toggle file list panel |
| `p` | Toggle commits panel |
| `i` | Toggle comments panel |

Panels: **File List** ↔ **Diff** ↔ **Description** (when open). The file list can be hidden with `b`, which gives the diff the full width.

### Search

| Key | Action |
|-----|--------|
| `/` | Search forward (regex, smart-case) |
| `?` | Search backward (in diff view) |
| `n` / `N` | Next / previous match (respects search direction) |
| `Esc` | Cancel search and restore cursor |
| `Enter` | Confirm search |

In the file picker, `/` opens a fuzzy file filter instead.

### Diff

| Key | Action |
|-----|--------|
| `t` | Toggle unified / side-by-side view |
| `zo` / `zc` | Expand / collapse file fold |
| `e` | Suggest change on current line |
| `E` | Expand context around cursor (+10 lines) |
| `m` | Mark file viewed / unviewed (diff or file list; syncs with GitHub) |

### Review

| Key | Action |
|-----|--------|
| `c` | Comment on current line (or edit pending / reply) |
| `v` | Visual select mode for multi-line comments |
| `x` | Discard pending comment at cursor |
| `Esc` | Cancel comment / cancel visual selection |
| `a` | Approve (quick confirm) |
| `r` | Resolve / unresolve comment thread |
| `s` | Submit review as comment-only (quick confirm) |
| `u` | Unapprove — dismiss your own approval |
| `y` | Accept suggestion (apply as commit) |

Comments are batched into a pending review and submitted together when you press `a` or `s`. These open a quick confirm popup (Enter / Esc). For request changes or review submissions with a body message, use the `:` command mode (see below).

#### Commits panel

`p` opens a right-hand drawer listing the PR's commits (short SHA, headline, author, age); it shares the drawer with the description panel, so opening one closes the other. The first row, **All changes**, is the normal full PR diff.

| Key (in the panel) | Action |
|-----|--------|
| `j` / `k` | Move through the commits |
| `Ctrl+D` / `Ctrl+U` | Half page down / up |
| `Ctrl+F` / `Ctrl+B`, `Space` / `Shift+Space` | Full page down / up |
| `gg` / `G` | Jump to the first row ("All changes") / the last commit |
| `Enter` | View just that commit's diff (on **All changes**: back to the full PR diff) |
| `s` | View everything *since* that commit, up to the PR head (what's new since you last looked) |
| `y` | Copy the commit's full SHA (platform clipboard tool, else the OSC 52 terminal sequence) |
| `o` | Open the commit on GitHub |
| `p` / `q` / `Esc` | Close the panel |

A commit or since view is **read-only**: it is marked in the title bar, review comments are hidden (they anchor to the full diff's lines), and commenting, suggesting, expanding context and the viewed mark are disabled until you return to **All changes**. Your drafts and viewed marks are kept. GitHub lists at most 300 files per commit view.

#### Where comments appear in the diff

- **On their line** when they have one.
- **Outdated comments** (written against code that has since changed, so GitHub no longer gives them a line) sit at the end of their own file's section, tagged `outdated · was line N`.
- **File-level comments** sit at the top of their file, tagged `file comment`.
- **A line comment whose line isn't in the shown diff** (for example on a file without a patch) sits at the end of its file, tagged `line N isn't in the shown diff`, rather than disappearing.
- **Conversation comments and review summaries**, which belong to no file, are collected under a `Conversation and review comments` heading after the last file.

A folded file hides its comments along with its diff.

#### Comments panel

`i` opens a right-hand drawer listing every review thread in the PR plus your pending drafts, in diff order, so comments can be found and handled without scrolling the diff. It shares the drawer with the description and commits panels (opening one closes the others).

Each entry shows its status (`○` open, `✓` resolved, `✎` your draft), `file:line`, the author, a reply count and a preview of the root comment.

| Key (in the panel) | Action | Applies to |
|-----|--------|------------|
| `j` / `k`, `gg` / `G`, `Ctrl+D` / `Ctrl+U`, `Ctrl+F` / `Ctrl+B`, `Space` | Move through the list | all |
| `Enter` | Jump to the comment in the diff (unfolds its file; leaves a commit view) | all |
| `r` | Reply to the thread | threads |
| `R` | Resolve / unresolve the thread | threads |
| `e` | Edit the draft | drafts |
| `x` | Discard the draft | drafts |
| `o` | Open the thread on GitHub | threads |
| `y` | Copy the comment text | all |
| `f` | Cycle the filter: all → unresolved → resolved → drafts | — |
| `i` / `q` / `Esc` | Close the panel | — |

The list updates as comments change, so resolving a thread, posting a reply or discarding a draft shows up straight away. Entries are labelled by where they sit: `file:line`, `file (outdated, was :12)`, `file (file)` for a whole-file comment, or `PR conversation`. Conversation comments are listed last, appear only under the `all` filter, and can be jumped to, copied and opened, but not replied to or resolved from here (they aren't review threads).

#### Viewed files

`m` toggles GitHub's per-file "Viewed" checkbox for the file at the cursor (or selected in the file list), the same checkbox as on github.com. Viewed files show a `✓` in the file list and header, and fold like on github.com; unmarking unfolds them.

The state is synced both ways: toggling pushes to GitHub immediately, and GitHub's state is pulled once the file list loads and with `:sync_viewed`. If a push fails, the toggle is reverted and the error is shown.

#### Pending review sync

Your draft comments are kept in sync with your pending review on GitHub, in both directions:

- Adding, editing, or discarding a draft updates your pending review on GitHub immediately, so it also shows up on github.com and in other clients.
- Drafts created or changed on GitHub are pulled in when a PR opens, and again with `:sync_drafts`.
- If the same comment was edited on both sides (or deleted on GitHub but edited here), a prompt shows both versions: `l` keeps yours, `g` keeps GitHub's, `Esc` decides later. Submitting is blocked until conflicts are resolved.
- If a push fails (for example, you're offline), the draft stays local and is retried on the next sync or when you submit.

GitHub allows one pending review per user per PR, and only shows it to you. Submitting from gh-review submits that pending review, including any comments you added to it on github.com.

### Other

| Key | Action |
|-----|--------|
| `:` | Open command prompt |
| `o` | Open PR in browser (all panels) |
| `!` | Show help overlay |
| `q` | Quit |

## Architecture

See [docs/architecture.md](docs/architecture.md) for the source tree layout and design overview.

## Roadmap

See [docs/ROADMAP.md](docs/ROADMAP.md) for planned features including stacked PR support, syntax highlighting, configuration, and more.

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines on how to get started, submit pull requests, and report issues.

This project follows the [Contributor Covenant Code of Conduct](CODE_OF_CONDUCT.md).

## License

[MIT](LICENSE)
