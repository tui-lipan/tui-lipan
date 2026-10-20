//! OSC 7501 revision 0.4 decoding and terminal-owned records.
use std::collections::BTreeMap;
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::{STANDARD, STANDARD_NO_PAD};

/// A protocol state. `clear` is an operation, never a stored state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "terminal-serde", serde(rename_all = "lowercase"))]
pub enum ProgramStatusState {
    /// At rest.
    Idle,
    /// Running.
    Working,
    /// Waiting for user action.
    Blocked,
    /// Finished successfully.
    Done,
    /// Failed and stopped.
    Error,
}

impl ProgramStatusState {
    /// The protocol spelling.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked => "blocked",
            Self::Done => "done",
            Self::Error => "error",
        }
    }

    fn active(self) -> bool {
        matches!(self, Self::Working | Self::Blocked)
    }
}

/// One independent record. Empty `id` names the root. Text is untrusted plain UTF-8.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct ProgramStatusRecord {
    /// Record path, or empty for root.
    pub id: Arc<str>,
    /// Reported state.
    pub state: ProgramStatusState,
    /// Explicit application identifier. Use the snapshot's `app_for` for inheritance.
    pub app: Option<Arc<str>>,
    /// Recognized blocking kind, only when blocked.
    pub kind: Option<Arc<str>>,
    /// Decoded human-readable label.
    pub title: Option<Arc<str>>,
    /// Decoded single-line message.
    pub msg: Option<Arc<str>>,
    /// Percentage, only when working or blocked.
    pub progress: Option<u8>,
    /// Monotonic report order for eviction and selection.
    pub updated: u64,
}

/// Immutable, bounded snapshot in stable ID order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct ProgramStatusSnapshot {
    /// Monotonic effective-store revision.
    pub revision: u64,
    /// At most 256 records, in ID order.
    pub records: Vec<ProgramStatusRecord>,
}

impl ProgramStatusSnapshot {
    /// Resolve the nearest explicit ancestor application, including the root.
    pub fn app_for<'a>(&'a self, record: &'a ProgramStatusRecord) -> Option<&'a str> {
        if let Some(app) = record.app.as_deref() {
            return Some(app);
        }
        let mut id = record.id.as_ref();
        while !id.is_empty() {
            id = id.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(app) = self
                .records
                .iter()
                .find(|r| r.id.as_ref() == id)
                .and_then(|r| r.app.as_deref())
            {
                return Some(app);
            }
        }
        None
    }
}

/// Why a record changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramStatusCause {
    /// A report created or replaced the record.
    Report,
    /// An explicit `state=clear` removed the record, alone or with an ancestor.
    Clear,
    /// A shell prompt or process exit removed the record because it was working or blocked.
    Lifecycle,
    /// A full reset removed every record.
    Reset,
    /// The store was full and this was its least recently updated record.
    Evicted,
}

/// One record's change, in the order the terminal applied it.
///
/// Drain these after each output batch. Unlike the final snapshot, they keep every transition a
/// batch contains: a program that reports `working` and then `done` in one write yields both.
/// Changes applied by one operation, such as a hierarchical clear or a report that evicts another
/// record, share its revision.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct ProgramStatusEvent {
    /// Monotonic effective-store revision after the change.
    pub revision: u64,
    /// Why the record changed.
    pub cause: ProgramStatusCause,
    /// The record's ID; empty for the root.
    pub id: Arc<str>,
    /// The record's application when it changed: its own, or the nearest ancestor's.
    pub app: Option<Arc<str>>,
    /// The record's state before the change, or `None` when it did not exist.
    pub previous: Option<ProgramStatusState>,
    /// The record after the change, or `None` when it was removed.
    pub record: Option<ProgramStatusRecord>,
}

#[derive(Debug, Default)]
pub(super) struct ProgramStatusStore {
    pub enabled: bool,
    records: BTreeMap<Arc<str>, ProgramStatusRecord>,
    revision: u64,
    order: u64,
    events: Vec<ProgramStatusEvent>,
}

impl ProgramStatusStore {
    pub fn snapshot(&self) -> ProgramStatusSnapshot {
        ProgramStatusSnapshot {
            revision: self.revision,
            records: self.records.values().cloned().collect(),
        }
    }

    pub fn drain_events(&mut self) -> Vec<ProgramStatusEvent> {
        std::mem::take(&mut self.events)
    }

    /// The record's own application, or the nearest stored ancestor's.
    fn app_of(&self, record: &ProgramStatusRecord) -> Option<Arc<str>> {
        if let Some(app) = &record.app {
            return Some(app.clone());
        }
        let mut id = record.id.as_ref();
        while !id.is_empty() {
            id = id.rsplit_once('/').map_or("", |(parent, _)| parent);
            if let Some(app) = self.records.get(id).and_then(|r| r.app.clone()) {
                return Some(app);
            }
        }
        None
    }

    /// Remove every record `remove` selects, as one change with one event per record.
    fn remove(&mut self, cause: ProgramStatusCause, remove: impl Fn(&ProgramStatusRecord) -> bool) {
        // Applications resolve before anything goes, since an ancestor may leave in the same change.
        let removed: Vec<_> = self
            .records
            .values()
            .filter(|record| remove(record))
            .map(|record| (record.id.clone(), self.app_of(record), record.state))
            .collect();
        if removed.is_empty() {
            return;
        }
        self.records.retain(|_, record| !remove(record));
        self.revision += 1;
        for (id, app, state) in removed {
            self.events.push(ProgramStatusEvent {
                revision: self.revision,
                cause,
                id,
                app,
                previous: Some(state),
                record: None,
            });
        }
    }

    pub fn clear_active(&mut self) {
        self.remove(ProgramStatusCause::Lifecycle, |record| {
            record.state.active()
        });
    }

    pub fn clear(&mut self, id: &str) {
        self.remove(ProgramStatusCause::Clear, |record| {
            is_descendant(&record.id, id)
        });
    }

    pub fn reset(&mut self) {
        self.remove(ProgramStatusCause::Reset, |_| true);
    }

    pub fn report(&mut self, body: &[u8]) {
        if !self.enabled {
            return;
        }
        let Some(report) = decode(body) else {
            return;
        };
        let Report::Record(mut record) = report else {
            if let Report::Clear(id) = report {
                self.clear(&id);
            }
            return;
        };
        self.order += 1;
        // Every valid report refreshes eviction order, including otherwise identical reports.
        record.updated = self.order;
        self.revision += 1;
        if self.records.len() == 256 && !self.records.contains_key(&record.id) {
            let oldest = self
                .records
                .values()
                .min_by_key(|r| r.updated)
                .map(|r| (r.id.clone(), self.app_of(r), r.state));
            if let Some((id, app, state)) = oldest {
                self.records.remove(&id);
                self.events.push(ProgramStatusEvent {
                    revision: self.revision,
                    cause: ProgramStatusCause::Evicted,
                    id,
                    app,
                    previous: Some(state),
                    record: None,
                });
            }
        }
        let previous = self.records.get(&record.id).map(|r| r.state);
        let app = self.app_of(&record);
        self.events.push(ProgramStatusEvent {
            revision: self.revision,
            cause: ProgramStatusCause::Report,
            id: record.id.clone(),
            app,
            previous,
            record: Some(record.clone()),
        });
        self.records.insert(record.id.clone(), record);
    }
}

fn is_descendant(candidate: &str, parent: &str) -> bool {
    parent.is_empty()
        || candidate == parent
        || candidate
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

enum Report {
    Record(ProgramStatusRecord),
    Clear(Arc<str>),
}

fn identifier(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.+-".contains(&b))
}

fn valid_id(id: &str) -> bool {
    id.len() <= 128
        && id.split('/').count() <= 8
        && id
            .split('/')
            .all(|segment| segment.len() <= 32 && identifier(segment))
}

fn text(value: &str, encoded: usize, decoded: usize) -> Option<Arc<str>> {
    if value.len() > encoded {
        return None;
    }
    let bytes = STANDARD
        .decode(value)
        .or_else(|_| STANDARD_NO_PAD.decode(value))
        .ok()?;
    if bytes.len() > decoded {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    if text.chars().any(char::is_control) {
        return None;
    }
    Some(text.into())
}

/// Check hard limits on every pair, then validate the last value of each text field.
/// Malformed pairs are skipped, unknown keys ignored.
fn pairs(body: &[u8]) -> Option<BTreeMap<&str, &str>> {
    let mut fields = BTreeMap::new();
    for pair in body.split(|b| *b == b':') {
        let Some(eq) = pair.iter().position(|b| *b == b'=') else {
            continue;
        };
        let (raw_key, raw_value) = (pair[..eq].trim_ascii(), pair[eq + 1..].trim_ascii());
        check_pair_limits(raw_key, raw_value)?;
        let (Ok(key), Ok(value)) = (std::str::from_utf8(raw_key), std::str::from_utf8(raw_value))
        else {
            if raw_key == b"id" {
                return None;
            }
            continue;
        };
        if key == "id" {
            // Invalid IDs never redirect to root. Last occurrence wins after all hard limits.
            fields.insert(key, value);
            continue;
        }
        if !pair_syntax(key, value) {
            continue;
        }
        fields.insert(key, value);
    }
    for (key, value) in &fields {
        validate_text_pair(key, value)?;
    }
    Some(fields)
}

fn check_pair_limits(key: &[u8], value: &[u8]) -> Option<()> {
    if key.len() > 16 {
        return None;
    }
    let limit = match key {
        b"app" => 32,
        b"title" => 256,
        b"msg" => 2732,
        b"id" => 128,
        _ => usize::MAX,
    };
    if value.len() > limit {
        return None;
    }
    if key == b"id"
        && (value.split(|b| *b == b'/').count() > 8
            || value.split(|b| *b == b'/').any(|s| s.len() > 32))
    {
        return None;
    }
    Some(())
}

fn pair_syntax(key: &str, value: &str) -> bool {
    !key.is_empty()
        && key.bytes().all(|b| b.is_ascii_lowercase())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.,+/=-".contains(&b))
}

fn validate_text_pair(key: &str, value: &str) -> Option<()> {
    match key {
        "msg" => {
            text(value, 2732, 2048)?;
        }
        "title" => {
            text(value, 256, 192)?;
        }
        _ => {}
    }
    Some(())
}

fn decode(body: &[u8]) -> Option<Report> {
    let fields = pairs(body)?;
    let id = fields.get("id").copied().unwrap_or("");
    if fields.contains_key("id") && !valid_id(id) {
        return None;
    }
    let state = match *fields.get("state")? {
        "clear" => return Some(Report::Clear(id.into())),
        "idle" => ProgramStatusState::Idle,
        "working" => ProgramStatusState::Working,
        "blocked" => ProgramStatusState::Blocked,
        "done" => ProgramStatusState::Done,
        "error" => ProgramStatusState::Error,
        _ => return None,
    };
    let app = fields
        .get("app")
        .filter(|v| identifier(v))
        .map(|v| Arc::from(*v));
    let kind = fields
        .get("kind")
        .filter(|v| {
            state == ProgramStatusState::Blocked && ["permission", "question", "auth"].contains(v)
        })
        .map(|v| Arc::from(*v));
    let progress = fields
        .get("progress")
        .filter(|_| state.active())
        .filter(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse::<u8>().ok())
        .filter(|v| *v <= 100);
    Some(Report::Record(ProgramStatusRecord {
        id: id.into(),
        state,
        app,
        kind,
        progress,
        title: fields.get("title").and_then(|v| text(v, 256, 192)),
        msg: fields.get("msg").and_then(|v| text(v, 2732, 2048)),
        updated: 0,
    }))
}

pub(super) const RESPONSE: &[u8] =
    b"\x1b]7501;?:states=idle,working,done,blocked,error:kinds=permission,question,auth\x1b\\";

/// A bounded wire scanner is necessary because vte removes C0 bytes before OSC dispatch and
/// dispatches on ESC before verifying ST. Limits apply to the original sequence, not that payload.
#[derive(Debug, Default)]
pub(super) struct ProgramStatusScanner {
    mode: WireMode,
    body: Vec<u8>,
    bytes: usize,
    // VTE ignores C0 bytes in OSC 133; keep its prefix separate from the raw OSC 7501 body.
    prompt_prefix: [u8; 6],
    prompt_prefix_len: usize,
}

#[derive(Debug, Default, Clone, Copy)]
enum WireMode {
    #[default]
    Ground,
    Escape,
    Intermediate,
    Csi,
    String,
    StringEscape,
    Osc,
    OscEscape,
}

impl ProgramStatusScanner {
    pub fn scan(&mut self, bytes: &[u8], store: &mut ProgramStatusStore) -> Vec<usize> {
        let mut replies = Vec::new();
        // Enablement changes reset wire state, and an empty disabled store has no lifecycle work.
        if !store.enabled && store.records.is_empty() {
            return replies;
        }
        let mut i = 0;
        while i < bytes.len() {
            if matches!(self.mode, WireMode::Ground) {
                let Some(offset) = memchr::memchr(0x1b, &bytes[i..]) else {
                    break;
                };
                i += offset;
            }
            let byte = bytes[i];
            if self.step(byte, store) {
                replies.push(i + 1);
            }
            i += 1;
        }
        replies
    }

    fn step(&mut self, byte: u8, store: &mut ProgramStatusStore) -> bool {
        use WireMode::*;
        match self.mode {
            // scan() only calls a ground step after locating ESC.
            Ground => self.mode = Escape,
            Escape => self.escape(byte, store),
            Intermediate | Csi => {
                let start = if matches!(self.mode, Csi) { 0x40 } else { 0x30 };
                self.end_control(byte, start);
            }
            String | StringEscape => self.string_byte(byte, store),
            Osc | OscEscape => return self.osc_step(byte, store),
        }
        false
    }

    fn string_byte(&mut self, byte: u8, store: &mut ProgramStatusStore) {
        if matches!(self.mode, WireMode::StringEscape) {
            self.mode = WireMode::Ground;
            if byte != b'\\' {
                self.escape(byte, store);
            }
        } else if byte == 0x1b {
            self.mode = WireMode::StringEscape;
        }
    }

    fn osc_step(&mut self, byte: u8, store: &mut ProgramStatusStore) -> bool {
        if !matches!(self.mode, WireMode::OscEscape) {
            return self.osc_byte(byte, store);
        }
        self.bytes = self.bytes.saturating_add(1);
        self.mode = WireMode::Ground;
        if byte == b'\\' {
            return self.finish(store);
        }
        self.escape(byte, store);
        false
    }

    fn end_control(&mut self, byte: u8, final_start: u8) {
        if byte == 0x1b {
            self.mode = WireMode::Escape;
        } else if (final_start..=0x7e).contains(&byte) {
            self.mode = WireMode::Ground;
        }
    }

    fn osc_byte(&mut self, byte: u8, store: &mut ProgramStatusStore) -> bool {
        self.bytes = self.bytes.saturating_add(1);
        // VTE dispatches OSC 133 at any OSC ending, even before ESC is confirmed as ST.
        if matches!(byte, 7 | 0x1b | 0x18 | 0x1a) {
            self.finish_prompt(store);
        }
        match byte {
            7 => {
                self.mode = WireMode::Ground;
                return self.finish(store);
            }
            0x1b => self.mode = WireMode::OscEscape,
            0x18 | 0x1a => self.mode = WireMode::Ground,
            _ => self.collect_osc_byte(byte),
        }
        false
    }

    fn collect_osc_byte(&mut self, byte: u8) {
        if self.bytes <= 4096 {
            self.body.push(byte);
        }
        if byte >= 0x20 && self.prompt_prefix_len < self.prompt_prefix.len() {
            self.prompt_prefix[self.prompt_prefix_len] = byte;
            self.prompt_prefix_len += 1;
        }
    }

    fn escape(&mut self, byte: u8, store: &mut ProgramStatusStore) {
        use WireMode::*;
        self.mode = match byte {
            b']' => {
                self.body.clear();
                self.bytes = 2;
                self.prompt_prefix_len = 0;
                Osc
            }
            b'[' => Csi,
            b'P' | b'_' | b'X' | b'^' => String,
            b'c' => {
                store.reset();
                Ground
            }
            0x20..=0x2f => Intermediate,
            0x1b => Escape,
            _ => Ground,
        };
    }

    fn finish(&self, store: &mut ProgramStatusStore) -> bool {
        if let Some(body) = self.body.strip_prefix(b"7501;") {
            if !store.enabled || self.bytes > 4096 {
                return false;
            }
            if body == b"?" {
                return true;
            }
            store.report(body);
        }
        false
    }

    fn finish_prompt(&self, store: &mut ProgramStatusStore) {
        if matches!(
            &self.prompt_prefix[..self.prompt_prefix_len],
            b"133;A" | b"133;A;"
        ) {
            store.clear_active();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::terminal::{TerminalCommandPhase, TerminalScreen};

    fn screen() -> TerminalScreen {
        let mut screen = TerminalScreen::new(24, 80, 100);
        screen.set_program_status_enabled(true);
        screen
    }

    fn report(screen: &mut TerminalScreen, body: &str) {
        screen.process_bytes(format!("\x1b]7501;{body}\x1b\\").as_bytes());
    }

    /// The prompt removed the working root and the blocked child, in one change, and kept `done`.
    fn assert_lifecycle_removed(screen: &mut TerminalScreen) {
        let events = screen.drain_program_status_events();
        let removed: Vec<_> = events
            .iter()
            .map(|e| (e.id.as_ref(), e.previous, e.record.is_none(), e.cause))
            .collect();
        assert_eq!(
            removed,
            [
                (
                    "",
                    Some(ProgramStatusState::Working),
                    true,
                    ProgramStatusCause::Lifecycle
                ),
                (
                    "child",
                    Some(ProgramStatusState::Blocked),
                    true,
                    ProgramStatusCause::Lifecycle
                ),
            ]
        );
        assert_eq!(events[0].revision, events[1].revision);
    }

    /// `(id, app, previous, current, cause)` for one drained event.
    type Transition = (
        String,
        Option<String>,
        Option<ProgramStatusState>,
        Option<ProgramStatusState>,
        ProgramStatusCause,
    );

    fn transitions(screen: &mut TerminalScreen) -> Vec<Transition> {
        screen
            .drain_program_status_events()
            .into_iter()
            .map(|e| {
                (
                    e.id.to_string(),
                    e.app.as_deref().map(str::to_string),
                    e.previous,
                    e.record.map(|r| r.state),
                    e.cause,
                )
            })
            .collect()
    }

    #[test]
    fn program_status_events_keep_every_transition_of_a_batch() {
        use ProgramStatusCause::Report;
        use ProgramStatusState::{Blocked, Done, Working};
        let mut s = screen();
        // One write, as an agent finishing a short turn produces: the snapshot shows only `done`.
        s.process_bytes(
            b"\x1b]7501;state=working:app=codex\x07\x1b]7501;state=blocked:app=codex:kind=permission\x07\x1b]7501;state=working:app=codex\x07\x1b]7501;state=done:app=codex\x07",
        );
        assert_eq!(s.program_status_snapshot().records[0].state, Done);
        let codex = Some("codex".to_string());
        assert_eq!(
            transitions(&mut s),
            [
                (String::new(), codex.clone(), None, Some(Working), Report),
                (
                    String::new(),
                    codex.clone(),
                    Some(Working),
                    Some(Blocked),
                    Report
                ),
                (
                    String::new(),
                    codex.clone(),
                    Some(Blocked),
                    Some(Working),
                    Report
                ),
                (String::new(), codex, Some(Working), Some(Done), Report),
            ]
        );
    }

    #[test]
    fn program_status_events_name_the_inherited_app_and_why_records_left() {
        use ProgramStatusCause::{Clear, Report, Reset};
        use ProgramStatusState::{Blocked, Done, Working};
        let mut s = screen();
        report(&mut s, "state=working:app=claude-code");
        report(&mut s, "state=blocked:id=tool");
        report(&mut s, "state=done:id=other:app=cargo");
        let claude = Some("claude-code".to_string());
        let cargo = Some("cargo".to_string());
        assert_eq!(
            transitions(&mut s),
            [
                (String::new(), claude.clone(), None, Some(Working), Report),
                ("tool".into(), claude.clone(), None, Some(Blocked), Report),
                ("other".into(), cargo.clone(), None, Some(Done), Report),
            ]
        );
        // Clearing an ancestor removes its children too, each still named for the app it inherited.
        report(&mut s, "state=clear");
        let cleared = transitions(&mut s);
        assert_eq!(
            cleared,
            [
                (String::new(), claude.clone(), Some(Working), None, Clear),
                ("other".into(), cargo, Some(Done), None, Clear),
                ("tool".into(), claude, Some(Blocked), None, Clear),
            ]
        );
        report(&mut s, "state=done");
        s.drain_program_status_events();
        s.process_bytes(b"\x1bc");
        assert_eq!(
            transitions(&mut s),
            [(String::new(), None, Some(Done), None, Reset)]
        );
        // Nothing left to remove is no change at all.
        s.process_bytes(b"\x1bc");
        s.program_status_process_exited();
        assert!(s.drain_program_status_events().is_empty());
    }

    #[test]
    fn program_status_eviction_precedes_the_report_that_caused_it() {
        let mut s = screen();
        for i in 0..256 {
            report(&mut s, &format!("state=done:id=r{i}"));
        }
        s.drain_program_status_events();
        report(&mut s, "state=working:id=new");
        let events = s.drain_program_status_events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].cause, ProgramStatusCause::Evicted);
        assert_eq!(events[0].id.as_ref(), "r0");
        assert!(events[0].record.is_none());
        assert_eq!(events[1].cause, ProgramStatusCause::Report);
        assert_eq!(events[1].id.as_ref(), "new");
        assert_eq!(events[0].revision, events[1].revision);
        assert_eq!(s.program_status_snapshot().revision, events[1].revision);
    }

    #[test]
    fn program_status_fragmentation_and_terminators() {
        let mut s = screen();
        for byte in b"\x1b]7501;state=working:app=cargo:msg=SGVsbG8:progress=42\x1b\\" {
            s.process_bytes(&[*byte]);
        }
        let snapshot = s.program_status_snapshot();
        let record = &snapshot.records[0];
        assert_eq!(record.msg.as_deref(), Some("Hello"));
        assert_eq!(record.progress, Some(42));
        s.process_bytes(b"\x1b]7501;state=error\x07");
        let snapshot = s.program_status_snapshot();
        assert_eq!(snapshot.records[0].state, ProgramStatusState::Error);
        assert!(snapshot.records[0].app.is_none());
        assert!(snapshot.records[0].msg.is_none());
    }

    #[test]
    fn program_status_validation_and_repeated_fields() {
        let mut s = screen();
        for invalid in [
            "state=unknown",
            "id=bad//id:state=done",
            "id=:state=done",
            "state=done:msg=%%%",
            "state=done:msg=AA==",
            "state=done:title=woc=",
            "state=done:msg=/w==",
            "state=done:id=bad$id",
            "state=done:id=a/b/c/d/e/f/g/h/i",
        ] {
            report(&mut s, invalid);
            // Syntactically malformed free-text pairs are skipped, as the spec requires.
            if invalid == "state=done:msg=%%%" {
                assert_eq!(s.program_status_snapshot().records.len(), 1);
                report(&mut s, "state=clear");
            } else {
                assert!(s.program_status_snapshot().records.is_empty(), "{invalid}");
            }
        }
        report(
            &mut s,
            "state=idle:id=bad//id:id=good:unknown=ok:bad: =a:state=blocked:kind=question:progress=100:title=SGk=",
        );
        let r = s.program_status_snapshot().records.remove(0);
        assert_eq!(r.state, ProgramStatusState::Blocked);
        assert_eq!(r.kind.as_deref(), Some("question"));
        assert_eq!(r.title.as_deref(), Some("Hi"));
        report(&mut s, "state=done:kind=auth:progress=100");
        let r = s.program_status_snapshot().records.remove(0);
        assert!(r.kind.is_none());
        assert!(r.progress.is_none());
        for progress in ["-1", "101", "1.0", "", "+1"] {
            report(&mut s, &format!("state=working:progress={progress}"));
            assert!(s.program_status_snapshot().records[0].progress.is_none());
        }
    }

    #[test]
    fn program_status_repeated_text_validates_last_value() {
        for key in ["title", "msg"] {
            for invalid in ["AA==", "woc=", "/w==", "A"] {
                let mut s = screen();
                report(&mut s, &format!("state=done:{key}={invalid}:{key}=SGk="));
                let record = s.program_status_snapshot().records.remove(0);
                let value = if key == "title" {
                    record.title
                } else {
                    record.msg
                };
                assert_eq!(value.as_deref(), Some("Hi"), "{key}={invalid}");
                let before = s.program_status_snapshot();
                report(&mut s, &format!("state=done:{key}=SGk=:{key}={invalid}"));
                assert_eq!(s.program_status_snapshot(), before);
                report(&mut s, &format!("state=clear:{key}=SGk=:{key}={invalid}"));
                assert_eq!(s.program_status_snapshot(), before);
            }
        }
    }

    #[test]
    fn program_status_repeated_text_checks_every_encoded_limit() {
        for (key, limit) in [("title", 256), ("msg", 2732)] {
            let mut s = screen();
            report(&mut s, "state=working");
            let before = s.program_status_snapshot();
            let oversized = "a".repeat(limit + 1);
            report(&mut s, &format!("state=done:{key}={oversized}:{key}=SGk="));
            assert_eq!(s.program_status_snapshot(), before);
        }
    }

    #[test]
    fn program_status_enablement_discards_partial_sequences() {
        for (partial, suffix) in [
            (b"\x1b]7501;state=work".as_slice(), b"ing\x07".as_slice()),
            (b"\x1b]7501;?", b"\x07"),
            (b"\x1b", b"]7501;state=working\x07"),
        ] {
            let mut s = screen();
            s.process_bytes(partial);
            s.set_program_status_enabled(false);
            s.process_bytes(b"junk");
            s.set_program_status_enabled(true);
            s.process_bytes(suffix);
            assert!(s.program_status_snapshot().records.is_empty());
            assert!(s.drain_program_status_events().is_empty());
            assert!(s.drain_responses().is_empty());
            report(&mut s, "state=done");
            assert_eq!(s.program_status_snapshot().records.len(), 1);
        }
        let mut s = screen();
        s.set_program_status_enabled(false);
        s.process_bytes(b"\x1b]7501;state=work");
        s.set_program_status_enabled(true);
        s.process_bytes(b"ing\x07");
        assert!(s.program_status_snapshot().records.is_empty());
    }

    #[test]
    fn program_status_redundant_enablement_preserves_partial_sequences() {
        let mut s = screen();
        s.process_bytes(b"\x1b]7501;state=work");
        s.set_program_status_enabled(true);
        s.process_bytes(b"ing\x07");
        assert_eq!(
            s.program_status_snapshot().records[0].state,
            ProgramStatusState::Working
        );
    }

    #[test]
    fn program_status_disabled_still_observes_lifecycle() {
        let mut s = screen();
        report(&mut s, "state=working");
        report(&mut s, "state=done:id=child");
        s.drain_program_status_events();
        let before = s.program_status_snapshot();
        s.set_program_status_enabled(false);
        report(&mut s, "state=error:id=child");
        s.process_bytes(b"\x1b]7501;?\x07");
        assert_eq!(s.program_status_snapshot(), before);
        assert!(s.drain_responses().is_empty());
        s.process_bytes(b"\x1b]133;A\x07");
        let snapshot = s.program_status_snapshot();
        assert_eq!(snapshot.records.len(), 1);
        assert_eq!(snapshot.records[0].state, ProgramStatusState::Done);
        assert_eq!(s.drain_program_status_events().len(), 1);
        s.process_bytes(b"\x1bc");
        assert!(s.program_status_snapshot().records.is_empty());
        assert_eq!(s.drain_program_status_events().len(), 1);
        s.set_program_status_enabled(true);
        assert!(s.program_status_snapshot().records.is_empty());
    }

    #[test]
    fn program_status_interrupted_prompts_match_terminal_lifecycle() {
        for enabled in [true, false] {
            for ending in [b"\x18".as_slice(), b"\x1a", b"\x1b", b"\x1b[0m", b"\x1b\\"] {
                let mut s = screen();
                report(&mut s, "state=working");
                report(&mut s, "state=blocked:id=child");
                report(&mut s, "state=done:id=finished");
                s.drain_program_status_events();
                s.set_program_status_enabled(enabled);
                s.process_bytes(b"\x1b]133;A");
                // ESC alone must expire records before the next byte confirms or interrupts ST.
                for byte in ending {
                    s.process_bytes(&[*byte]);
                    assert_eq!(
                        s.semantic_state().command_phase,
                        TerminalCommandPhase::Prompt
                    );
                    let snapshot = s.program_status_snapshot();
                    assert_eq!(snapshot.records.len(), 1);
                    assert_eq!(snapshot.records[0].state, ProgramStatusState::Done);
                }
                assert_lifecycle_removed(&mut s);
                s.set_program_status_enabled(true);
                report(&mut s, "state=idle");
                assert_eq!(s.program_status_snapshot().records.len(), 2);
            }
        }
    }

    #[test]
    fn program_status_interrupted_reports_and_queries_are_ignored() {
        for body in ["state=done", "state=clear", "?"] {
            for ending in [b"\x18".as_slice(), b"\x1a", b"\x1b[0m"] {
                let mut s = screen();
                report(&mut s, "state=working");
                s.drain_program_status_events();
                let before = s.program_status_snapshot();
                s.process_bytes(format!("\x1b]7501;{body}").as_bytes());
                for byte in ending {
                    s.process_bytes(&[*byte]);
                }
                assert_eq!(s.program_status_snapshot(), before);
                assert!(s.drain_program_status_events().is_empty());
                assert!(s.drain_responses().is_empty());
                report(&mut s, "state=done");
                assert_eq!(
                    s.program_status_snapshot().records[0].state,
                    ProgramStatusState::Done
                );
            }
        }
    }

    #[test]
    fn program_status_long_prompts_clear_active_records() {
        for enabled in [true, false] {
            for terminator in ["\x07", "\x1b\\"] {
                let mut s = screen();
                report(&mut s, "state=working");
                report(&mut s, "state=blocked:id=child");
                report(&mut s, "state=done:id=finished");
                s.drain_program_status_events();
                s.set_program_status_enabled(enabled);
                let prompt = format!("\x1b]133;A;{}{}", "x".repeat(5000), terminator);
                for chunk in prompt.as_bytes().chunks(127) {
                    s.process_bytes(chunk);
                }
                let snapshot = s.program_status_snapshot();
                assert_eq!(snapshot.records.len(), 1);
                assert_eq!(snapshot.records[0].state, ProgramStatusState::Done);
                assert_lifecycle_removed(&mut s);
            }
        }
    }

    #[test]
    fn program_status_prompts_ignore_c0_bytes_without_changing_report_validation() {
        for enabled in [true, false] {
            for ignored in [0x00..=0x06, 0x08..=0x17, 0x19..=0x19, 0x1c..=0x1f]
                .into_iter()
                .flatten()
            {
                let mut s = screen();
                report(&mut s, "state=working");
                s.set_program_status_enabled(enabled);
                let mut prompt = b"\x1b]".to_vec();
                // Force the meaningful prefix beyond the raw report buffer's limit.
                prompt.extend(std::iter::repeat_n(ignored, 5000));
                for byte in b"133;A" {
                    prompt.extend_from_slice(&[ignored, *byte, ignored]);
                }
                prompt.push(7);
                for chunk in prompt.chunks(127) {
                    s.process_bytes(chunk);
                }
                assert!(
                    s.program_status_snapshot().records.is_empty(),
                    "C0 {ignored:#x}"
                );
            }
        }
        let mut s = screen();
        s.process_bytes(b"\x1b]7501;state=done:msg=SG\x00k=\x07");
        assert!(s.program_status_snapshot().records[0].msg.is_none());
        s.process_bytes(b"\x1b]75\x0001;state=working\x07");
        assert_eq!(
            s.program_status_snapshot().records[0].state,
            ProgramStatusState::Done
        );
    }

    #[test]
    fn program_status_disabled_empty_store_skips_scanning() {
        let mut scanner = ProgramStatusScanner::default();
        let mut store = ProgramStatusStore::default();
        assert!(scanner.scan(b"\x1b]7501;state=work", &mut store).is_empty());
        assert!(matches!(scanner.mode, WireMode::Ground));
        assert!(scanner.body.is_empty());
        assert_eq!(scanner.bytes, 0);
        store.enabled = true;
        scanner.scan(b"\x1b]7501;state=working\x07", &mut store);
        store.enabled = false;
        scanner.scan(b"\x1b]133;A\x07", &mut store);
        assert!(store.records.is_empty());
        let bytes = scanner.bytes;
        scanner.scan(b"\x1b]7501;state=work", &mut store);
        assert!(matches!(scanner.mode, WireMode::Ground));
        assert_eq!(scanner.bytes, bytes);
    }

    #[test]
    fn program_status_hierarchy_inheritance_replacement_and_eviction() {
        let mut s = screen();
        report(&mut s, "state=working:app=cargo");
        report(&mut s, "state=blocked:id=build/test");
        report(&mut s, "state=working:id=build:app=runner");
        report(&mut s, "state=done:id=builder");
        let snapshot = s.program_status_snapshot();
        assert_eq!(snapshot.app_for(&snapshot.records[2]), Some("runner"));
        report(&mut s, "state=clear:id=build");
        assert_eq!(s.program_status_snapshot().records.len(), 2);
        report(&mut s, "state=clear");
        for i in 0..256 {
            report(&mut s, &format!("state=done:id=r{i}"));
        }
        report(&mut s, "state=done:id=r0");
        report(&mut s, "state=done:id=new");
        let snapshot = s.program_status_snapshot();
        assert_eq!(snapshot.records.len(), 256);
        assert!(!snapshot.records.iter().any(|r| r.id.as_ref() == "r1"));
        assert!(snapshot.records.iter().any(|r| r.id.as_ref() == "r0"));
    }

    #[test]
    fn program_status_ordered_prompt_reset_and_exit() {
        let mut s = screen();
        report(&mut s, "state=working");
        s.process_bytes(b"\x1b]133;Anything\x07");
        assert_eq!(s.program_status_snapshot().records.len(), 1);
        s.drain_program_status_events();
        s.process_bytes(b"\x1b]133;A\x07\x1b]7501;state=working\x07\x1b]133;A\x07\x1b]7501;state=blocked\x07\x1b]133;A\x07\x1b]7501;state=error\x07");
        assert_eq!(
            s.program_status_snapshot().records[0].state,
            ProgramStatusState::Error
        );
        assert_eq!(s.drain_program_status_events().len(), 6);
        report(&mut s, "state=working:id=child");
        s.program_status_process_exited();
        assert_eq!(s.program_status_snapshot().records.len(), 1);
        s.process_bytes(b"\x1b[!p\x1b[?1049h\x1b[?1049l");
        s.resize(30, 90);
        assert_eq!(s.program_status_snapshot().records.len(), 1);
        s.process_bytes(b"\x1bc\x1b]7501;state=idle\x07");
        assert_eq!(
            s.program_status_snapshot().records[0].state,
            ProgramStatusState::Idle
        );
        s.program_status_process_exited();
        assert_eq!(s.program_status_snapshot().records.len(), 1);
        s.process_bytes(b"\x1bc");
        assert!(s.program_status_snapshot().records.is_empty());
    }

    #[test]
    fn program_status_limits_and_raw_wire_bytes() {
        let mut s = screen();
        for (key, limit) in [("app", 32), ("id", 32), ("title", 192), ("msg", 2048)] {
            let value = if key == "title" || key == "msg" {
                STANDARD.encode("a".repeat(limit))
            } else {
                "a".repeat(limit)
            };
            report(&mut s, &format!("state=done:{key}={value}"));
            assert_eq!(s.program_status_snapshot().records.len(), 1, "{key}");
            report(&mut s, "state=clear");
            let value = if key == "title" || key == "msg" {
                STANDARD.encode("a".repeat(limit + 1))
            } else {
                "a".repeat(limit + 1)
            };
            report(&mut s, &format!("state=done:{key}={value}"));
            assert!(s.program_status_snapshot().records.is_empty(), "{key}");
        }
        // The whole sequence cap includes ignored keys and raw C0 bytes removed by vte.
        let prefix = "\x1b]7501;state=done:x=";
        let legal = format!("{prefix}{}\x07", "a".repeat(4096 - prefix.len() - 1));
        s.process_bytes(legal.as_bytes());
        assert_eq!(s.program_status_snapshot().records.len(), 1);
        report(&mut s, "state=clear");
        s.process_bytes(format!("{prefix}{}\x07", "a".repeat(4096 - prefix.len())).as_bytes());
        assert!(s.program_status_snapshot().records.is_empty());
        report(&mut s, "state=done:abcdefghijklmnopq=x");
        assert!(s.program_status_snapshot().records.is_empty());
        let invalid = format!("\x1b]7501;state=done:x={}\x07", "\x00".repeat(4096));
        s.process_bytes(invalid.as_bytes());
        assert!(s.program_status_snapshot().records.is_empty());
    }

    #[test]
    fn program_status_queries_are_ordered_and_opt_in() {
        let mut s = screen();
        s.process_bytes(b"\x1b]7501;?\x1b\\\x1b[c");
        let replies = s.drain_responses();
        assert_eq!(replies.len(), 2);
        assert_eq!(replies[0], RESPONSE);
        s.process_bytes(b"\x1b[c\x1b]7501;?\x07\x1b[>q");
        let replies = s.drain_responses();
        assert_eq!(replies.len(), 3);
        assert_eq!(replies[1], RESPONSE);
        let mut passive = TerminalScreen::new(24, 80, 100);
        passive.process_bytes(b"\x1b]7501;?\x07\x1b]7501;state=done\x07");
        assert!(passive.drain_responses().is_empty());
        assert!(passive.program_status_snapshot().records.is_empty());
    }
}
