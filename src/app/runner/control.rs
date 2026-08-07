//! Live control channel for driving a running app from outside the process.
//!
//! Setting `TUI_LIPAN_CONTROL=<path>` makes [`AppRunner::run`](super::AppRunner::run)
//! listen on a Unix socket. A client sends one command per line and reads a
//! length-prefixed reply, so an agent can inspect and drive a live TUI the way a
//! browser tool drives a page: take a snapshot, pick a widget by key, act, look
//! again.
//!
//! # Threading
//!
//! Runtime state is single-threaded `Rc`/`RefCell`, so the listener thread never
//! touches it. It ships a [`ControlRequest`] over the same channel the terminal
//! reader uses and blocks on a reply channel; the event loop computes the
//! response on the UI thread. This is the pattern the crossterm reader already
//! follows.
//!
//! # Protocol
//!
//! Requests are single `\n`-terminated lines:
//!
//! ```text
//! ping                      liveness check
//! keys                      list reconciliation keys currently rendered
//! snapshot                  markdown snapshot of the current UI
//! snapshot json             JSON snapshot (needs `ui-snapshot-json`)
//! snapshot png <path>       write a PNG to <path> (needs `ui-snapshot-png`)
//! act <script>              run an action script (`click:#add; type:hi`)
//! highlight <key>           outline a widget by key
//! highlight <col>,<row>     outline the widget under a cell
//! highlight clear           remove the outline
//! quit                      ask the app to exit
//! ```
//!
//! Replies are a status line followed by exactly that many bytes:
//!
//! ```text
//! ok <byte-length>\n<payload>
//! err <byte-length>\n<message>
//! ```
//!
//! Length prefixing keeps payloads binary- and newline-safe without escaping, so
//! a client in any language is a few lines of code.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};

/// Socket path; setting it enables the control channel.
pub(crate) const CONTROL_ENV: &str = "TUI_LIPAN_CONTROL";

/// Requests waiting for the UI thread.
///
/// The queue carries the payload and [`RunnerEvent::Control`] is only a wakeup,
/// so the event enum stays cheap to clone and compare.
pub(crate) type ControlQueue = Arc<Mutex<VecDeque<ControlRequest>>>;

/// A command from a client, with the channel its reply must go back on.
pub(crate) struct ControlRequest {
    /// Raw command line, without the trailing newline.
    pub(crate) command: String,
    /// Where the UI thread sends the reply.
    pub(crate) reply: Sender<ControlReply>,
}

/// The result of running one command.
pub(crate) enum ControlReply {
    /// Success, with a payload that may be empty.
    Ok(String),
    /// Failure, with a message explaining what went wrong.
    Err(String),
}

impl ControlReply {
    /// Serialise as a status line plus length-prefixed payload.
    fn encode(&self) -> Vec<u8> {
        let (status, payload) = match self {
            Self::Ok(payload) => ("ok", payload.as_str()),
            Self::Err(message) => ("err", message.as_str()),
        };
        let mut out = format!("{status} {}\n", payload.len()).into_bytes();
        out.extend_from_slice(payload.as_bytes());
        out
    }
}

/// Returns the configured control socket path, if any.
pub(crate) fn control_path() -> Option<std::path::PathBuf> {
    std::env::var_os(CONTROL_ENV)
        .filter(|path| !path.is_empty())
        .map(std::path::PathBuf::from)
}

/// Owns the listening socket and removes it on drop.
///
/// A stale socket file left behind by a crashed run would make the next start
/// fail with `Address already in use`, so cleanup is tied to the guard.
pub(crate) struct ControlGuard {
    path: std::path::PathBuf,
}

impl Drop for ControlGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Start listening, forwarding each command to `events`.
///
/// The socket is created user-only: anything that can reach it can type into the
/// application, so it must not be world-writable.
#[cfg(unix)]
pub(crate) fn spawn(
    path: std::path::PathBuf,
    queue: ControlQueue,
    events: Sender<super::RunnerEvent>,
) -> crate::Result<ControlGuard> {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::net::UnixListener;

    // A leftover file from a previous run is not a live listener; binding over
    // it is the documented way to restart.
    let _ = std::fs::remove_file(&path);
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }

    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;

    std::thread::Builder::new()
        .name("tui-lipan-control".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { break };
                // Connections are served one at a time: the UI is a single
                // shared surface, so interleaving two drivers would produce a
                // session neither of them asked for.
                if serve(stream, &queue, &events).is_err() {
                    break;
                }
            }
        })?;

    Ok(ControlGuard { path })
}

/// Control channel needs Unix sockets; other platforms report that plainly.
#[cfg(not(unix))]
pub(crate) fn spawn(
    _path: std::path::PathBuf,
    _queue: ControlQueue,
    _events: Sender<super::RunnerEvent>,
) -> crate::Result<ControlGuard> {
    Err(std::io::Error::other("TUI_LIPAN_CONTROL requires Unix domain sockets").into())
}

/// Read commands from one client until it disconnects.
///
/// Returns `Err` only when the event loop has gone away, which ends the listener.
#[cfg(unix)]
fn serve(
    stream: std::os::unix::net::UnixStream,
    queue: &ControlQueue,
    events: &Sender<super::RunnerEvent>,
) -> Result<(), ()> {
    let mut writer = match stream.try_clone() {
        Ok(stream) => stream,
        Err(_) => return Ok(()),
    };
    let reader = BufReader::new(stream);

    for line in reader.lines() {
        let Ok(command) = line else { return Ok(()) };
        let command = command.trim().to_owned();
        if command.is_empty() {
            continue;
        }

        let reply = match exchange(command, queue, events) {
            Ok(reply) => reply,
            // The UI thread is gone; stop listening rather than hanging clients.
            Err(()) => return Err(()),
        };
        if writer.write_all(&reply.encode()).is_err() || writer.flush().is_err() {
            return Ok(());
        }
    }
    Ok(())
}

/// Hand one command to the UI thread and wait for its reply.
fn exchange(
    command: String,
    queue: &ControlQueue,
    events: &Sender<super::RunnerEvent>,
) -> Result<ControlReply, ()> {
    let (reply_tx, reply_rx): (Sender<ControlReply>, Receiver<ControlReply>) = channel();
    queue.lock().map_err(|_| ())?.push_back(ControlRequest {
        command,
        reply: reply_tx,
    });
    // The event only wakes the loop; the payload travels in the queue.
    events.send(super::RunnerEvent::Control).map_err(|_| ())?;
    reply_rx.recv().map_err(|_| ())
}

/// A parsed control command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ControlCommand {
    /// Liveness check.
    Ping,
    /// List rendered reconciliation keys.
    Keys,
    /// Capture the UI in the requested format.
    Snapshot(SnapshotFormat),
    /// Run an action script.
    Act(String),
    /// Outline a widget, or clear the outline.
    Highlight(Option<HighlightTarget>),
    /// Ask the app to quit.
    Quit,
}

/// What a `highlight` command points at.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum HighlightTarget {
    /// The widget carrying this reconciliation key.
    Key(String),
    /// The smallest widget covering this cell, the way an inspector picks.
    Cell(u16, u16),
}

/// Format requested by a `snapshot` command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SnapshotFormat {
    /// Agent-readable markdown.
    Markdown,
    /// Structured JSON.
    Json,
    /// PNG written to a path.
    Png(std::path::PathBuf),
}

/// Parse one command line.
pub(crate) fn parse_command(line: &str) -> Result<ControlCommand, String> {
    let line = line.trim();
    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((verb, rest)) => (verb, rest.trim()),
        None => (line, ""),
    };

    match verb {
        "ping" => Ok(ControlCommand::Ping),
        "keys" => Ok(ControlCommand::Keys),
        "quit" => Ok(ControlCommand::Quit),
        "highlight" => match rest {
            "" | "clear" | "off" | "none" => Ok(ControlCommand::Highlight(None)),
            target => match target.split_once(',') {
                // `col,row` picks whatever is under the cell, so unkeyed widgets
                // are still inspectable.
                Some((x, y)) => {
                    let x = x
                        .trim()
                        .parse()
                        .map_err(|_| format!("invalid highlight column in `{target}`"))?;
                    let y = y
                        .trim()
                        .parse()
                        .map_err(|_| format!("invalid highlight row in `{target}`"))?;
                    Ok(ControlCommand::Highlight(Some(HighlightTarget::Cell(x, y))))
                }
                None => Ok(ControlCommand::Highlight(Some(HighlightTarget::Key(
                    target.trim_start_matches('#').to_owned(),
                )))),
            },
        },
        "act" => {
            if rest.is_empty() {
                return Err("act needs a script, e.g. `act click:#submit`".into());
            }
            Ok(ControlCommand::Act(rest.to_owned()))
        }
        "snapshot" => match rest {
            "" | "md" | "markdown" => Ok(ControlCommand::Snapshot(SnapshotFormat::Markdown)),
            "json" => Ok(ControlCommand::Snapshot(SnapshotFormat::Json)),
            other => match other.split_once(char::is_whitespace) {
                Some(("png", path)) if !path.trim().is_empty() => Ok(ControlCommand::Snapshot(
                    SnapshotFormat::Png(std::path::PathBuf::from(path.trim())),
                )),
                _ => Err(format!(
                    "unknown snapshot format `{other}`; expected markdown, json, or `png <path>`"
                )),
            },
        },
        other => Err(format!(
            "unknown command `{other}`; expected ping, keys, snapshot, act, highlight, or quit"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_status_line_plus_length_prefixed_payload() {
        assert_eq!(ControlReply::Ok("hi".into()).encode(), b"ok 2\nhi".to_vec());
        assert_eq!(ControlReply::Ok(String::new()).encode(), b"ok 0\n".to_vec());
        assert_eq!(
            ControlReply::Err("nope".into()).encode(),
            b"err 4\nnope".to_vec()
        );
    }

    #[test]
    fn payloads_with_newlines_need_no_escaping() {
        // Length prefixing is the whole reason a markdown snapshot can be sent
        // verbatim; a line-delimited reply would have to escape it.
        let payload = "line one\nline two\n";
        let encoded = ControlReply::Ok(payload.into()).encode();
        assert!(encoded.starts_with(b"ok 18\n"));
        assert!(encoded.ends_with(payload.as_bytes()));
    }

    #[test]
    fn simple_verbs_parse() {
        assert_eq!(parse_command("ping"), Ok(ControlCommand::Ping));
        assert_eq!(parse_command("  keys  "), Ok(ControlCommand::Keys));
        assert_eq!(parse_command("quit"), Ok(ControlCommand::Quit));
    }

    #[test]
    fn snapshot_defaults_to_markdown_and_accepts_formats() {
        assert_eq!(
            parse_command("snapshot"),
            Ok(ControlCommand::Snapshot(SnapshotFormat::Markdown))
        );
        assert_eq!(
            parse_command("snapshot md"),
            Ok(ControlCommand::Snapshot(SnapshotFormat::Markdown))
        );
        assert_eq!(
            parse_command("snapshot json"),
            Ok(ControlCommand::Snapshot(SnapshotFormat::Json))
        );
        assert_eq!(
            parse_command("snapshot png /tmp/a.png"),
            Ok(ControlCommand::Snapshot(SnapshotFormat::Png(
                "/tmp/a.png".into()
            )))
        );
    }

    #[test]
    fn act_keeps_the_whole_script_including_spaces() {
        assert_eq!(
            parse_command("act click:#add; type:buy milk"),
            Ok(ControlCommand::Act("click:#add; type:buy milk".into()))
        );
    }

    #[test]
    fn highlight_takes_a_key_or_clears() {
        assert_eq!(
            parse_command("highlight add"),
            Ok(ControlCommand::Highlight(Some(HighlightTarget::Key(
                "add".into()
            ))))
        );
        // A leading `#` is accepted for symmetry with action-script targets.
        assert_eq!(
            parse_command("highlight #add"),
            Ok(ControlCommand::Highlight(Some(HighlightTarget::Key(
                "add".into()
            ))))
        );
        // Unkeyed widgets are still inspectable by cell.
        assert_eq!(
            parse_command("highlight 61,2"),
            Ok(ControlCommand::Highlight(Some(HighlightTarget::Cell(
                61, 2
            ))))
        );
        assert!(parse_command("highlight x,2").is_err());
        for clearing in ["highlight", "highlight clear", "highlight off"] {
            assert_eq!(
                parse_command(clearing),
                Ok(ControlCommand::Highlight(None)),
                "{clearing}"
            );
        }
    }

    #[test]
    fn malformed_commands_explain_themselves() {
        for line in ["frobnicate", "act", "snapshot sideways", "snapshot png"] {
            let err = parse_command(line).expect_err(line);
            assert!(!err.is_empty(), "{line} should explain the problem");
        }
        assert!(
            parse_command("frobnicate")
                .unwrap_err()
                .contains("expected")
        );
        assert!(parse_command("act").unwrap_err().contains("needs a script"));
    }
}
