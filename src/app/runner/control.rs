//! Live control channel for driving a running app from outside the process.
//!
//! Setting `TUI_LIPAN_CONTROL=<path>` makes [`AppRunner::run`](super::AppRunner::run)
//! listen on a Unix socket. A client sends one command per line and reads a
//! length-prefixed reply, so an agent can inspect and drive a live TUI the way a
//! browser tool drives a page: take a snapshot, pick a widget by automation ID, act, look
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
//! Requests are bounded, versioned `\n`-terminated frames:
//!
//! ```text
//! tui-lipan/1 <request-id> <deadline-ms> <command>\n
//! ```
//!
//! `hello` negotiates capabilities. Commands include `keys`, `snapshot`,
//! `snapshot json`, `snapshot png`, `act <script>`, `highlight`, `cancel
//! <request-id>`, and `quit`. PNG bytes are returned to the client; the server
//! never accepts an output path.
//!
//! Replies are a status line followed by exactly the declared bytes:
//!
//! ```text
//! tui-lipan/1 <request-id> ok - <byte-length>\n<payload>
//! tui-lipan/1 <request-id> err <code> <byte-length>\n<message>
//! ```
//!
//! Length prefixing keeps payloads binary- and newline-safe without escaping, so
//! a client in any language is a few lines of code.

use std::collections::{HashMap, VecDeque};
use std::io::{BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::sync::{Arc, Mutex};

const PROTOCOL_VERSION: u16 = 1;
const MAX_REQUEST_BYTES: usize = 64 * 1024;
pub(crate) const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

/// Socket path; setting it enables the control channel.
pub(crate) const CONTROL_ENV: &str = "TUI_LIPAN_CONTROL";

/// Requests waiting for the UI thread.
///
/// The queue carries the payload and [`RunnerEvent::Control`] is only a wakeup,
/// so the event enum stays cheap to clone and compare.
#[derive(Clone, Default)]
pub(crate) struct ControlQueue {
    state: Arc<ControlState>,
}

#[derive(Default)]
struct ControlState {
    pending: Mutex<VecDeque<ControlRequest>>,
    cancellations: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

impl ControlQueue {
    fn push(&self, request: ControlRequest) -> Result<(), ()> {
        self.state
            .pending
            .lock()
            .map_err(|_| ())?
            .push_back(request);
        Ok(())
    }

    pub(crate) fn pop(&self) -> Option<ControlRequest> {
        self.state.pending.lock().ok()?.pop_front()
    }

    fn register(&self, id: &str, cancelled: Arc<AtomicBool>) -> Result<(), ()> {
        let mut registrations = self.state.cancellations.lock().map_err(|_| ())?;
        if registrations.contains_key(id) {
            return Err(());
        }
        registrations.insert(id.to_owned(), cancelled);
        Ok(())
    }

    fn unregister(&self, id: &str) {
        if let Ok(mut registrations) = self.state.cancellations.lock() {
            registrations.remove(id);
        }
    }

    fn cancel(&self, id: &str) -> bool {
        let Ok(registrations) = self.state.cancellations.lock() else {
            return false;
        };
        let Some(cancelled) = registrations.get(id) else {
            return false;
        };
        cancelled.store(true, Ordering::Release);
        true
    }
}

/// A command from a client, with the channel its reply must go back on.
pub(crate) struct ControlRequest {
    /// Client-chosen request identity.
    pub(crate) id: String,
    /// Raw command line, without the trailing newline.
    pub(crate) command: String,
    /// Wall-clock deadline.
    pub(crate) deadline: std::time::Instant,
    /// Cooperative cancellation set by another request or deadline.
    pub(crate) cancelled: Arc<AtomicBool>,
    /// Where the UI thread sends the reply.
    pub(crate) reply: Sender<ControlReply>,
}

/// The result of running one command.
#[derive(Debug)]
pub(crate) enum ControlReply {
    /// Success, with a payload that may be empty.
    Ok(Vec<u8>),
    /// Failure, with a stable wire code and message.
    Err { code: &'static str, message: String },
}

impl ControlReply {
    /// Serialise as a status line plus length-prefixed payload.
    fn encode(&self, id: &str) -> Vec<u8> {
        let (status, code, payload): (&str, &str, &[u8]) = match self {
            Self::Ok(payload) => ("ok", "-", payload),
            Self::Err { code, message } => ("err", code, message.as_bytes()),
        };
        let mut out = format!(
            "tui-lipan/{PROTOCOL_VERSION} {id} {status} {code} {}\n",
            payload.len()
        )
        .into_bytes();
        out.extend_from_slice(payload);
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
                let queue = queue.clone();
                let events = events.clone();
                let _ = std::thread::Builder::new()
                    .name("tui-lipan-control-client".into())
                    .spawn(move || {
                        let _ = serve(stream, &queue, &events);
                    });
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
    let mut reader = BufReader::new(stream);

    loop {
        let mut line = String::new();
        let read = match reader
            .by_ref()
            .take((MAX_REQUEST_BYTES + 1) as u64)
            .read_line(&mut line)
        {
            Ok(read) => read,
            Err(_) => return Ok(()),
        };
        if read == 0 {
            return Ok(());
        }
        if read > MAX_REQUEST_BYTES || !line.ends_with('\n') {
            let reply = ControlReply::Err {
                code: "REQUEST_TOO_LARGE",
                message: format!("request exceeds {MAX_REQUEST_BYTES} bytes"),
            };
            let _ = writer.write_all(&reply.encode("-"));
            let _ = writer.flush();
            return Ok(());
        }
        let (id, timeout, command) = match parse_request_header(line.trim_end()) {
            Ok(request) => request,
            Err(reply) => {
                if writer.write_all(&reply.encode("-")).is_err() || writer.flush().is_err() {
                    return Ok(());
                }
                continue;
            }
        };

        let reply = match exchange(id.clone(), timeout, command, queue, events) {
            Ok(reply) => reply,
            // The UI thread is gone; stop listening rather than hanging clients.
            Err(()) => return Err(()),
        };
        if writer.write_all(&reply.encode(&id)).is_err() || writer.flush().is_err() {
            return Ok(());
        }
    }
}

/// Hand one command to the UI thread and wait for its reply.
fn exchange(
    id: String,
    timeout: std::time::Duration,
    command: String,
    queue: &ControlQueue,
    events: &Sender<super::RunnerEvent>,
) -> Result<ControlReply, ()> {
    if let Some(target_id) = command.strip_prefix("cancel ").map(str::trim) {
        return Ok(if queue.cancel(target_id) {
            ControlReply::Ok(Vec::new())
        } else {
            ControlReply::Err {
                code: "UNKNOWN_REQUEST",
                message: format!("no active request `{target_id}`"),
            }
        });
    }
    let (reply_tx, reply_rx): (Sender<ControlReply>, Receiver<ControlReply>) = channel();
    let deadline = std::time::Instant::now()
        .checked_add(timeout)
        .unwrap_or_else(std::time::Instant::now);
    let cancelled = Arc::new(AtomicBool::new(false));
    if queue.register(&id, Arc::clone(&cancelled)).is_err() {
        return Ok(ControlReply::Err {
            code: "DUPLICATE_REQUEST_ID",
            message: format!("request `{id}` is already active"),
        });
    }
    if queue
        .push(ControlRequest {
            id: id.clone(),
            command,
            deadline,
            cancelled: Arc::clone(&cancelled),
            reply: reply_tx,
        })
        .is_err()
    {
        queue.unregister(&id);
        return Err(());
    }
    // The event only wakes the loop; the payload travels in the queue.
    if events.send(super::RunnerEvent::Control).is_err() {
        queue.unregister(&id);
        return Err(());
    }
    let reply = match reply_rx.recv_timeout(timeout) {
        Ok(reply) => Ok(reply),
        Err(RecvTimeoutError::Timeout) => {
            cancelled.store(true, Ordering::Release);
            Ok(ControlReply::Err {
                code: "DEADLINE_EXCEEDED",
                message: "request deadline elapsed".into(),
            })
        }
        Err(RecvTimeoutError::Disconnected) => Err(()),
    };
    queue.unregister(&id);
    reply
}

fn parse_request_header(line: &str) -> Result<(String, std::time::Duration, String), ControlReply> {
    let Some(rest) = line.strip_prefix(&format!("tui-lipan/{PROTOCOL_VERSION} ")) else {
        return Err(ControlReply::Err {
            code: "UNSUPPORTED_VERSION",
            message: format!("expected tui-lipan/{PROTOCOL_VERSION} request"),
        });
    };
    let mut parts = rest.splitn(3, ' ');
    let id = parts.next().unwrap_or_default();
    let timeout_ms = parts.next().unwrap_or_default();
    let command = parts.next().unwrap_or_default().trim();
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        return Err(ControlReply::Err {
            code: "INVALID_REQUEST_ID",
            message: "request ID must be 1-64 ASCII letters, digits, '-' or '_'".into(),
        });
    }
    let timeout_ms: u64 = timeout_ms.parse().map_err(|_| ControlReply::Err {
        code: "INVALID_DEADLINE",
        message: "deadline must be milliseconds".into(),
    })?;
    if timeout_ms == 0 || timeout_ms > 60_000 {
        return Err(ControlReply::Err {
            code: "INVALID_DEADLINE",
            message: "deadline must be between 1 and 60000 milliseconds".into(),
        });
    }
    if command.is_empty() {
        return Err(ControlReply::Err {
            code: "MALFORMED_REQUEST",
            message: "request command is empty".into(),
        });
    }
    Ok((
        id.to_owned(),
        std::time::Duration::from_millis(timeout_ms),
        command.to_owned(),
    ))
}

/// A parsed control command.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ControlCommand {
    /// Negotiate protocol capabilities.
    Hello,
    /// Liveness check.
    Ping,
    /// List rendered automation IDs.
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
    /// The widget carrying this automation ID.
    AutomationId(String),
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
    /// PNG bytes returned in the reply.
    Png,
}

/// Parse one command line.
pub(crate) fn parse_command(line: &str) -> Result<ControlCommand, String> {
    let line = line.trim();
    let (verb, rest) = match line.split_once(char::is_whitespace) {
        Some((verb, rest)) => (verb, rest.trim()),
        None => (line, ""),
    };

    match verb {
        "hello" => Ok(ControlCommand::Hello),
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
                None => Ok(ControlCommand::Highlight(Some(
                    HighlightTarget::AutomationId(target.trim_start_matches('#').to_owned()),
                ))),
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
            "png" => Ok(ControlCommand::Snapshot(SnapshotFormat::Png)),
            other => Err(format!(
                "unknown snapshot format `{other}`; expected markdown, json, or png"
            )),
        },
        // `cancel <id>` never reaches here - the transport answers it without the UI thread - but
        // a `cancel` missing its argument does, and the list is the only place a client is told
        // the verb exists at all.
        "cancel" => Err("cancel needs a request ID, e.g. `cancel req-7`".into()),
        other => Err(format!(
            "unknown command `{other}`; expected hello, ping, keys, snapshot, act, highlight, \
             cancel, or quit"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_status_line_plus_length_prefixed_payload() {
        assert_eq!(
            ControlReply::Ok(b"hi".to_vec()).encode("req"),
            b"tui-lipan/1 req ok - 2\nhi".to_vec()
        );
        assert_eq!(
            ControlReply::Ok(Vec::new()).encode("req"),
            b"tui-lipan/1 req ok - 0\n".to_vec()
        );
        assert_eq!(
            ControlReply::Err {
                code: "TEST",
                message: "nope".into(),
            }
            .encode("req"),
            b"tui-lipan/1 req err TEST 4\nnope".to_vec()
        );
    }

    #[test]
    fn payloads_with_newlines_need_no_escaping() {
        // Length prefixing is the whole reason a markdown snapshot can be sent
        // verbatim; a line-delimited reply would have to escape it.
        let payload = "line one\nline two\n";
        let encoded = ControlReply::Ok(payload.as_bytes().to_vec()).encode("req");
        assert!(encoded.starts_with(b"tui-lipan/1 req ok - 18\n"));
        assert!(encoded.ends_with(payload.as_bytes()));
    }

    #[test]
    fn simple_verbs_parse() {
        assert_eq!(parse_command("hello"), Ok(ControlCommand::Hello));
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
            parse_command("snapshot png"),
            Ok(ControlCommand::Snapshot(SnapshotFormat::Png))
        );
    }

    #[test]
    fn request_headers_are_versioned_bounded_and_deadlined() {
        let (id, timeout, command) =
            parse_request_header("tui-lipan/1 req_1 500 snapshot png").unwrap();
        assert_eq!(id, "req_1");
        assert_eq!(timeout, std::time::Duration::from_millis(500));
        assert_eq!(command, "snapshot png");
        assert!(parse_request_header("ping").is_err());
        assert!(parse_request_header("tui-lipan/1 ! 500 ping").is_err());
        assert!(parse_request_header("tui-lipan/1 req 0 ping").is_err());
    }

    #[test]
    fn cancellation_registry_targets_only_active_request_ids() {
        let queue = ControlQueue::default();
        let cancelled = Arc::new(AtomicBool::new(false));
        queue.register("active", Arc::clone(&cancelled)).unwrap();
        assert!(queue.cancel("active"));
        assert!(cancelled.load(Ordering::Acquire));
        queue.unregister("active");
        assert!(!queue.cancel("active"));
    }

    #[test]
    fn act_keeps_the_whole_script_including_spaces() {
        assert_eq!(
            parse_command("act click:#add; type:buy milk"),
            Ok(ControlCommand::Act("click:#add; type:buy milk".into()))
        );
    }

    #[test]
    fn highlight_takes_an_automation_id_or_clears() {
        assert_eq!(
            parse_command("highlight add"),
            Ok(ControlCommand::Highlight(Some(
                HighlightTarget::AutomationId("add".into())
            )))
        );
        // A leading `#` is accepted for symmetry with action-script targets.
        assert_eq!(
            parse_command("highlight #add"),
            Ok(ControlCommand::Highlight(Some(
                HighlightTarget::AutomationId("add".into())
            )))
        );
        // Widgets without IDs are still inspectable by cell.
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
        for line in ["frobnicate", "act", "snapshot sideways"] {
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
