//! Copy text to the system clipboard.
//!
//! Tries a platform clipboard tool first (reliable when installed), then
//! falls back to the OSC 52 terminal escape, which works over SSH in
//! terminals that support it (kitty, WezTerm, Ghostty, foot, iTerm2, tmux
//! with `set-clipboard on`).

use std::io::Write;
use std::process::{Command, Stdio};

use base64::Engine;

/// Clipboard tools to try, in order: (program, args).
const TOOLS: &[(&str, &[&str])] = &[
    ("pbcopy", &[]),
    ("wl-copy", &[]),
    ("xclip", &["-selection", "clipboard"]),
    ("xsel", &["--clipboard", "--input"]),
];

/// The OSC 52 sequence that asks the terminal to set the clipboard.
pub fn osc52_sequence(text: &str) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(text);
    format!("\x1b]52;c;{encoded}\x07")
}

fn copy_with_tool(program: &str, args: &[&str], text: &str) -> bool {
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take()
        && stdin.write_all(text.as_bytes()).is_err()
    {
        let _ = child.kill();
        return false;
    }
    child.wait().map(|s| s.success()).unwrap_or(false)
}

/// Copy `text`, returning how it was done (for a status message).
pub fn copy(text: &str) -> Result<&'static str, String> {
    for (program, args) in TOOLS {
        if copy_with_tool(program, args, text) {
            return Ok(program);
        }
    }
    let mut out = std::io::stdout();
    out.write_all(osc52_sequence(text).as_bytes())
        .and_then(|()| out.flush())
        .map_err(|e| format!("no clipboard tool found and the terminal write failed: {e}"))?;
    Ok("terminal (OSC 52)")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn osc52_wraps_base64_text() {
        // "hello" -> aGVsbG8=
        assert_eq!(osc52_sequence("hello"), "\x1b]52;c;aGVsbG8=\x07");
    }

    #[test]
    fn osc52_handles_a_commit_sha() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        let seq = osc52_sequence(sha);
        let payload = seq
            .strip_prefix("\x1b]52;c;")
            .and_then(|s| s.strip_suffix('\x07'))
            .unwrap();
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .unwrap();
        assert_eq!(decoded, sha.as_bytes());
    }

    #[test]
    fn a_missing_tool_is_reported_as_failure() {
        assert!(!copy_with_tool(
            "definitely-not-a-real-clipboard-tool",
            &[],
            "x"
        ));
    }
}
