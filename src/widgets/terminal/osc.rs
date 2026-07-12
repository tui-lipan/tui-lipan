//! Parallel OSC 7 / OSC 9;9 / OSC 133 semantic-state observer.
//!
//! [`TerminalScreen`](super::TerminalScreen) feeds every PTY byte stream through the primary
//! Alacritty grid parser *and* a second, independent [`vte::Parser`](alacritty_terminal::vte::Parser)
//! driven by [`SemanticObserver`]. The observer only implements [`Perform::osc_dispatch`] (every
//! other callback keeps the trait's no-op default), so it tracks working-directory and
//! command-lifecycle escape sequences without touching cell/cursor state or the render snapshot.
//!
//! This state is intentionally kept out of [`TerminalRenderSnapshot`](super::TerminalRenderSnapshot):
//! it is runtime metadata (current working directory, foreground command phase, reported
//! executable), not something the renderer paints.

use std::sync::Arc;

use alacritty_terminal::vte::Perform;

/// Upper bound on a decoded OSC 7/9;9/133 value this module will retain.
///
/// `vte`'s own OSC buffer (`MAX_OSC_RAW`) already bounds the raw wire payload; this is a second,
/// deliberately generous bound applied after percent-decoding so a pathological child cannot force
/// unbounded allocation here even if the wire-level limit changes upstream.
const MAX_SEMANTIC_VALUE_LEN: usize = 8192;

/// A working directory reported by the child program via `OSC 7` or `OSC 9;9`.
///
/// `OSC 7` (`file://host/path`) is percent-encoded; the decoded path may contain non-UTF-8 bytes on
/// Unix (arbitrary filenames are valid `OsStr` data, not necessarily valid UTF-8), so the path is
/// kept as raw bytes rather than lossily converted. Build an `OsStr`/`PathBuf` from
/// [`TerminalWorkingDirectory::path`] with `std::os::unix::ffi::OsStrExt` (or the platform
/// equivalent) at the call site instead of assuming UTF-8.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalWorkingDirectory {
    /// The `host` component of an `OSC 7` URI, when the child reported one.
    ///
    /// A present, non-local host means the path names a location on a *different* machine (e.g.
    /// an SSH hop): callers must not pass it to a local `Command::current_dir` without first
    /// checking it against the local hostname. `OSC 9;9` never carries a host and always reports
    /// `None` here.
    pub host: Option<Arc<str>>,
    /// Percent-decoded path bytes, exactly as reported (no lossy UTF-8 conversion).
    pub path: Arc<[u8]>,
    /// Which OSC sequence produced this report.
    pub source: TerminalWorkingDirectorySource,
}

impl TerminalWorkingDirectory {
    /// Borrow the path as `&str` when it happens to be valid UTF-8.
    ///
    /// Returns `None` for the (rare but valid) case of non-UTF-8 path bytes; use
    /// [`path`](Self::path) plus a platform-specific `OsStr` conversion to handle those exactly.
    pub fn path_str(&self) -> Option<&str> {
        std::str::from_utf8(&self.path).ok()
    }
}

/// Which escape sequence produced a [`TerminalWorkingDirectory`] report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalWorkingDirectorySource {
    /// `OSC 7 ; file://host/path`.
    Osc7,
    /// `OSC 9 ; 9 ; path` (ConEmu/Windows Terminal convention).
    Osc9,
}

/// Command-lifecycle phase reported via `OSC 133 A/B/C/D`.
///
/// `A` = prompt start, `B` = end of prompt / start of user input, `C` = command execution start,
/// `D` = command finished. Shells that never emit `OSC 133` leave this at
/// [`TerminalCommandPhase::Unknown`] forever, which callers should treat as "no shell integration
/// available" rather than "idle at a prompt".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TerminalCommandPhase {
    /// No `OSC 133` markers observed yet (or the child never emits them).
    #[default]
    Unknown,
    /// `OSC 133;A` - the shell is drawing a prompt.
    Prompt,
    /// `OSC 133;B` - the prompt finished; the shell is reading a command line.
    Input,
    /// `OSC 133;C` - a command is executing.
    Executing,
    /// `OSC 133;D[;exit_code]` - the command finished.
    Completed {
        /// The reported exit status, when the shell included one.
        exit_status: Option<i32>,
    },
}

/// A semantic change extracted from PTY output by
/// [`TerminalScreen::drain_semantic_events`](crate::widgets::terminal::TerminalScreen::drain_semantic_events).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalSemanticEvent {
    /// The reported working directory changed (`OSC 7` or `OSC 9;9`).
    WorkingDirectoryChanged(TerminalWorkingDirectory),
    /// The command lifecycle phase changed (`OSC 133 A/B/C/D`).
    CommandPhaseChanged(TerminalCommandPhase),
    /// The foreground executable identity changed, or was cleared (`None`).
    ///
    /// Only ever carries a normalized basename - shell integrations are expected to emit just the
    /// executable name (e.g. `htop`, `vim`), never a full command line with arguments.
    ExecutableChanged(Option<Arc<str>>),
}

/// Current semantic state accumulated from `OSC 7`/`OSC 9;9`/`OSC 133` sequences.
///
/// Returned by
/// [`TerminalScreen::semantic_state`](crate::widgets::terminal::TerminalScreen::semantic_state)
/// and accepted by
/// [`TerminalScreen::restore_semantic_state`](crate::widgets::terminal::TerminalScreen::restore_semantic_state)
/// so state can be persisted and reapplied (e.g. across session resurrection) without replaying
/// synthetic escape sequences.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalSemanticState {
    /// Most recently reported working directory, if any.
    pub cwd: Option<TerminalWorkingDirectory>,
    /// Current command lifecycle phase.
    pub command_phase: TerminalCommandPhase,
    /// Most recently reported foreground executable basename, if any.
    pub executable: Option<Arc<str>>,
}

/// Parallel `vte::Perform` implementation that only reacts to `osc_dispatch`.
///
/// Every other `Perform` method (print, execute, csi_dispatch, ...) keeps the trait's no-op
/// default: this observer never touches grid/cursor state and cannot affect rendering.
#[derive(Debug, Default)]
pub(super) struct SemanticObserver {
    state: TerminalSemanticState,
    events: Vec<TerminalSemanticEvent>,
}

impl SemanticObserver {
    pub(super) fn state(&self) -> TerminalSemanticState {
        self.state.clone()
    }

    pub(super) fn restore_state(&mut self, state: TerminalSemanticState) {
        self.state = state;
    }

    pub(super) fn drain_events(&mut self) -> Vec<TerminalSemanticEvent> {
        std::mem::take(&mut self.events)
    }

    fn set_cwd(&mut self, cwd: TerminalWorkingDirectory) {
        if self.state.cwd.as_ref() != Some(&cwd) {
            self.state.cwd = Some(cwd.clone());
            self.events
                .push(TerminalSemanticEvent::WorkingDirectoryChanged(cwd));
        }
    }

    fn set_phase(&mut self, phase: TerminalCommandPhase) {
        if self.state.command_phase != phase {
            self.state.command_phase = phase;
            self.events
                .push(TerminalSemanticEvent::CommandPhaseChanged(phase));
        }
    }

    fn set_executable(&mut self, executable: Option<Arc<str>>) {
        if self.state.executable != executable {
            self.state.executable = executable.clone();
            self.events
                .push(TerminalSemanticEvent::ExecutableChanged(executable));
        }
    }

    fn handle_osc7(&mut self, rest: &[&[u8]]) {
        let Some(raw) = join_params(rest) else {
            return;
        };
        let Some((host, path)) = parse_file_uri(&raw) else {
            return;
        };
        self.set_cwd(TerminalWorkingDirectory {
            host,
            path: path.into(),
            source: TerminalWorkingDirectorySource::Osc7,
        });
    }

    fn handle_osc9_9(&mut self, rest: &[&[u8]]) {
        let Some(raw) = join_params(rest) else {
            return;
        };
        if raw.is_empty() || raw.contains(&0) {
            return;
        }
        self.set_cwd(TerminalWorkingDirectory {
            host: None,
            path: raw.into(),
            source: TerminalWorkingDirectorySource::Osc9,
        });
    }

    fn handle_osc133(&mut self, rest: &[&[u8]]) {
        let Some(&marker) = rest.first() else {
            return;
        };
        let Some(&letter) = marker.first() else {
            return;
        };

        let mut exit_status: Option<i32> = None;
        let mut executable: Option<Arc<str>> = None;
        for param in rest.iter().skip(1) {
            if let Some(eq) = param.iter().position(|&b| b == b'=') {
                let key = &param[..eq];
                let value = &param[eq + 1..];
                if value.len() > MAX_SEMANTIC_VALUE_LEN {
                    continue;
                }
                match key {
                    b"hyprmux_exe" => {
                        if let Some(decoded) = percent_decode(value)
                            && let Some(name) = normalize_executable(&decoded)
                        {
                            executable = Some(name);
                        }
                    }
                    b"cmdline_url" => {
                        if let Some(decoded) = percent_decode(value)
                            && let Some(name) = first_token_basename(&decoded)
                        {
                            executable = Some(name);
                        }
                    }
                    b"exit_code" if letter == b'D' => {
                        exit_status = parse_ascii_i32(value);
                    }
                    _ => {}
                }
            } else if letter == b'D' {
                // Plain-integer form: `OSC 133;D;<exit_code>` (iTerm2/VS Code convention).
                exit_status = exit_status.or_else(|| parse_ascii_i32(param));
            }
        }

        match letter {
            b'A' => {
                self.set_phase(TerminalCommandPhase::Prompt);
                self.set_executable(None);
            }
            b'B' => self.set_phase(TerminalCommandPhase::Input),
            b'C' => {
                self.set_phase(TerminalCommandPhase::Executing);
                if executable.is_some() {
                    self.set_executable(executable);
                }
            }
            b'D' => self.set_phase(TerminalCommandPhase::Completed { exit_status }),
            _ => {}
        }
    }
}

impl Perform for SemanticObserver {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(&code) = params.first() else {
            return;
        };
        match code {
            b"7" => self.handle_osc7(&params[1..]),
            b"9" if params.get(1).copied() == Some(&b"9"[..]) => self.handle_osc9_9(&params[2..]),
            b"133" => self.handle_osc133(&params[1..]),
            _ => {}
        }
    }
}

/// Join OSC sub-params back with `;`, the delimiter `vte` split them on.
///
/// Paths/URIs can legally contain `;`, which `vte`'s generic OSC splitting has no way to know
/// about; rejoining recovers the original payload. Returns `None` once the joined length exceeds
/// [`MAX_SEMANTIC_VALUE_LEN`], so a pathological child cannot force unbounded work here.
fn join_params(parts: &[&[u8]]) -> Option<Vec<u8>> {
    if parts.is_empty() {
        return Some(Vec::new());
    }
    let total: usize = parts.iter().map(|p| p.len()).sum::<usize>() + parts.len() - 1;
    if total > MAX_SEMANTIC_VALUE_LEN {
        return None;
    }
    let mut out = Vec::with_capacity(total);
    for (i, part) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b';');
        }
        out.extend_from_slice(part);
    }
    Some(out)
}

/// Parse a `file://host/path` (or bare `//host/path` / bare `path`) URI into `(host, path)`.
///
/// The path is percent-decoded; the host is percent-decoded and required to be valid UTF-8 (a host
/// name is not expected to carry arbitrary binary data). Returns `None` when the decoded path is
/// empty, contains a NUL byte, or the host is present but not valid UTF-8 - callers should treat
/// that as "no usable report" and fall back to the next CWD source.
fn parse_file_uri(raw: &[u8]) -> Option<(Option<Arc<str>>, Vec<u8>)> {
    let remainder: &[u8] = if let Some(stripped) = strip_scheme(raw, b"file://") {
        stripped
    } else if let Some(stripped) = raw.strip_prefix(b"//") {
        stripped
    } else {
        raw
    };

    let (host_raw, path_raw) = match remainder.iter().position(|&b| b == b'/') {
        Some(idx) => (&remainder[..idx], &remainder[idx..]),
        None => (&[][..], remainder),
    };

    let host = if host_raw.is_empty() {
        None
    } else {
        let decoded = percent_decode(host_raw)?;
        let host = String::from_utf8(decoded).ok()?;
        Some(Arc::from(host))
    };

    let path = percent_decode(path_raw)?;
    if path.is_empty() || path.contains(&0) {
        return None;
    }
    Some((host, path))
}

/// Case-insensitively strip a URI scheme prefix (e.g. `file://`).
fn strip_scheme<'a>(raw: &'a [u8], scheme: &[u8]) -> Option<&'a [u8]> {
    if raw.len() < scheme.len() {
        return None;
    }
    let (prefix, rest) = raw.split_at(scheme.len());
    prefix.eq_ignore_ascii_case(scheme).then_some(rest)
}

/// Percent-decode `%XX` escapes, preserving raw (possibly non-UTF-8) bytes.
///
/// Returns `None` if the input exceeds [`MAX_SEMANTIC_VALUE_LEN`] or contains a malformed `%`
/// escape (not followed by two hex digits) - malformed input is rejected outright rather than
/// guessed at.
fn percent_decode(input: &[u8]) -> Option<Vec<u8>> {
    if input.len() > MAX_SEMANTIC_VALUE_LEN {
        return None;
    }
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        match input[i] {
            b'%' => {
                let hi = *input.get(i + 1)?;
                let lo = *input.get(i + 2)?;
                let value = (hex_digit(hi)? << 4) | hex_digit(lo)?;
                out.push(value);
                i += 3;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    Some(out)
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn parse_ascii_i32(bytes: &[u8]) -> Option<i32> {
    std::str::from_utf8(bytes).ok()?.trim().parse().ok()
}

/// Reduce a decoded `hyprmux_exe` value to a normalized basename.
///
/// Only the executable identity is ever kept - never a full command line - and control/NUL bytes
/// invalidate the whole value rather than being silently stripped.
fn normalize_executable(decoded: &[u8]) -> Option<Arc<str>> {
    if decoded.is_empty() || decoded.len() > MAX_SEMANTIC_VALUE_LEN {
        return None;
    }
    let text = std::str::from_utf8(decoded).ok()?;
    basename(text.trim())
}

/// Extract the first whitespace-delimited token of a decoded command line and return its
/// basename, for integrations (Fish/Kitty `cmdline_url`) that report a full command line rather
/// than an isolated executable identity. Only the basename is kept: hyprmux never persists or
/// forwards full command lines through normalized protocol messages.
fn first_token_basename(decoded: &[u8]) -> Option<Arc<str>> {
    if decoded.len() > MAX_SEMANTIC_VALUE_LEN {
        return None;
    }
    let text = std::str::from_utf8(decoded).ok()?;
    let first = text.split_whitespace().next()?;
    basename(first)
}

fn basename(path: &str) -> Option<Arc<str>> {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    if name.is_empty() || name.contains('\0') {
        return None;
    }
    Some(Arc::from(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::vte::Parser as VteParser;

    fn drive(bytes: &[u8]) -> (TerminalSemanticState, Vec<TerminalSemanticEvent>) {
        let mut parser = VteParser::new();
        let mut observer = SemanticObserver::default();
        parser.advance(&mut observer, bytes);
        (observer.state(), observer.drain_events())
    }

    #[test]
    fn osc7_decodes_local_path_without_host() {
        let (state, events) = drive(b"\x1b]7;file:///home/user/my%20project\x07");
        let cwd = state.cwd.expect("cwd");
        assert_eq!(cwd.host, None);
        assert_eq!(cwd.path_str(), Some("/home/user/my project"));
        assert_eq!(cwd.source, TerminalWorkingDirectorySource::Osc7);
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn osc7_preserves_remote_host() {
        let (state, _) = drive(b"\x1b]7;file://build-box/srv/app\x07");
        let cwd = state.cwd.expect("cwd");
        assert_eq!(cwd.host, Some(Arc::from("build-box")));
        assert_eq!(cwd.path_str(), Some("/srv/app"));
    }

    #[test]
    fn osc7_accepts_st_termination_and_split_pty_chunks() {
        let mut parser = VteParser::new();
        let mut observer = SemanticObserver::default();
        // Split mid-escape, mid-path, and across the ST terminator.
        for chunk in [
            &b"\x1b]7;file://"[..],
            &b"/home/a"[..],
            &b"/b\x1b"[..],
            &b"\\"[..],
        ] {
            parser.advance(&mut observer, chunk);
        }
        let cwd = observer.state().cwd.expect("cwd");
        assert_eq!(cwd.path_str(), Some("/home/a/b"));
    }

    #[test]
    fn osc7_rejects_non_utf8_host_but_keeps_raw_path_bytes() {
        // Invalid UTF-8 in the host is rejected outright; invalid UTF-8 in the *path* is instead
        // preserved as raw bytes (percent-encoded so the wire bytes here are ASCII, but decoding
        // an arbitrary byte like %FF must still succeed rather than lossily mangling it).
        let (state, _) = drive(b"\x1b]7;file:///tmp/bad%FFname\x07");
        let cwd = state.cwd.expect("cwd");
        assert_eq!(cwd.path.as_ref(), b"/tmp/bad\xffname");
        assert_eq!(cwd.path_str(), None);
    }

    #[test]
    fn osc7_rejects_malformed_percent_escape() {
        let (state, events) = drive(b"\x1b]7;file:///tmp/bad%2\x07");
        assert!(state.cwd.is_none());
        assert!(events.is_empty());
    }

    #[test]
    fn osc7_semicolons_in_path_round_trip() {
        // `vte` splits OSC params on `;`; a literal `;` in the path must be rejoined, not lost.
        let (state, _) = drive(b"\x1b]7;file:///tmp/a;b\x07");
        assert_eq!(
            state.cwd.expect("cwd").path_str(),
            Some("/tmp/a;b"),
            "semicolon in path must survive OSC param splitting"
        );
    }

    #[test]
    fn osc9_9_reports_windows_style_path_without_host() {
        let (state, _) = drive(b"\x1b]9;9;C:\\Users\\dev\\project\x07");
        let cwd = state.cwd.expect("cwd");
        assert_eq!(cwd.host, None);
        assert_eq!(cwd.path_str(), Some("C:\\Users\\dev\\project"));
        assert_eq!(cwd.source, TerminalWorkingDirectorySource::Osc9);
    }

    #[test]
    fn osc133_tracks_prompt_input_execution_and_completion() {
        let mut parser = VteParser::new();
        let mut observer = SemanticObserver::default();

        parser.advance(&mut observer, b"\x1b]133;A\x07");
        assert_eq!(observer.state().command_phase, TerminalCommandPhase::Prompt);

        parser.advance(&mut observer, b"\x1b]133;B\x07");
        assert_eq!(observer.state().command_phase, TerminalCommandPhase::Input);

        parser.advance(&mut observer, b"\x1b]133;C\x07");
        assert_eq!(
            observer.state().command_phase,
            TerminalCommandPhase::Executing
        );

        parser.advance(&mut observer, b"\x1b]133;D;0\x07");
        assert_eq!(
            observer.state().command_phase,
            TerminalCommandPhase::Completed {
                exit_status: Some(0)
            }
        );

        let events = observer.drain_events();
        assert_eq!(events.len(), 4);
    }

    #[test]
    fn osc133_completion_reports_nonzero_exit_status() {
        let (state, _) = drive(b"\x1b]133;D;127\x07");
        assert_eq!(
            state.command_phase,
            TerminalCommandPhase::Completed {
                exit_status: Some(127)
            }
        );
    }

    #[test]
    fn osc133_completion_without_code_reports_none() {
        let (state, _) = drive(b"\x1b]133;D\x07");
        assert_eq!(
            state.command_phase,
            TerminalCommandPhase::Completed { exit_status: None }
        );
    }

    #[test]
    fn osc133_hyprmux_extension_reports_executable_basename() {
        let (state, _) = drive(b"\x1b]133;C;hyprmux_exe=%2Fusr%2Fbin%2Fhtop\x07");
        assert_eq!(state.executable, Some(Arc::from("htop")));
    }

    #[test]
    fn osc133_cmdline_url_extracts_first_token_basename_only() {
        // Fish/Kitty's `cmdline_url` carries a full command line; only the executable basename
        // may enter semantic state, never the full line (e.g. `--flag secret-arg` must vanish).
        let (state, _) = drive(b"\x1b]133;C;cmdline_url=%2Fusr%2Fbin%2Fgrep%20--flag%20secret\x07");
        assert_eq!(state.executable, Some(Arc::from("grep")));
    }

    #[test]
    fn osc133_new_prompt_clears_previous_executable() {
        let mut parser = VteParser::new();
        let mut observer = SemanticObserver::default();
        parser.advance(&mut observer, b"\x1b]133;C;hyprmux_exe=vim\x07");
        assert_eq!(observer.state().executable, Some(Arc::from("vim")));

        parser.advance(&mut observer, b"\x1b]133;A\x07");
        assert_eq!(observer.state().executable, None);
    }

    #[test]
    fn unknown_osc_codes_are_ignored() {
        let (state, events) = drive(b"\x1b]4;1;?\x07\x1b]2;title\x07");
        assert_eq!(state, TerminalSemanticState::default());
        assert!(events.is_empty());
    }

    #[test]
    fn restore_state_reapplies_without_emitting_events() {
        let mut observer = SemanticObserver::default();
        let restored = TerminalSemanticState {
            cwd: Some(TerminalWorkingDirectory {
                host: None,
                path: Arc::from(b"/tmp".as_slice()),
                source: TerminalWorkingDirectorySource::Osc7,
            }),
            command_phase: TerminalCommandPhase::Input,
            executable: Some(Arc::from("bash")),
        };
        observer.restore_state(restored.clone());
        assert_eq!(observer.state(), restored);
        assert!(observer.drain_events().is_empty());
    }

    #[test]
    fn oversized_osc7_payload_is_rejected() {
        let mut payload = b"\x1b]7;file:///".to_vec();
        payload.extend(std::iter::repeat_n(b'a', MAX_SEMANTIC_VALUE_LEN + 16));
        payload.extend_from_slice(b"\x07");
        let (state, events) = drive(&payload);
        assert!(state.cwd.is_none());
        assert!(events.is_empty());
    }
}
