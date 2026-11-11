//! The drag-and-drop protocol (OSC 72) as a terminal sees it from the program it runs.
//!
//! A [`TerminalScreen`](super::TerminalScreen) with the protocol enabled answers the program's
//! support query in stream order, keeps what the program registered, and queues every other
//! command for the embedder, which owns the conversation with the host and the user. See
//! [`crate::dnd`].

use crate::dnd::{
    ChildRegistration, CommandDecoder, DragRegistration, DropRegistration, OscScanner,
    ProgramCommand, TaggedCommand, TerminalEvent,
};

/// The screen's end of the protocol.
#[derive(Debug, Default)]
pub(super) struct DragAndDropCapture {
    pub(super) enabled: bool,
    scanner: OscScanner,
    decoder: CommandDecoder,
    registration: ChildRegistration,
    generation: u64,
    commands: Vec<TaggedCommand>,
    /// The previous byte was an escape outside any escape code: `ESC c` would be a full reset.
    after_escape: bool,
}

impl DragAndDropCapture {
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if self.enabled != enabled {
            self.enabled = enabled;
            self.scanner = OscScanner::default();
            self.decoder = CommandDecoder::default();
            if !enabled {
                self.clear_registration();
                self.commands.clear();
            }
        }
    }

    /// Read `bytes`; returns the replies owed to the program, each with the offset just past the
    /// escape code that asked for it.
    pub(super) fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
        if !self.enabled {
            return Vec::new();
        }
        let mut replies = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            let reset = self.after_escape && byte == b'c';
            self.after_escape = byte == 0x1b && !self.scanner.in_escape_code();
            if let Some(body) = self.scanner.step(byte) {
                for tagged in self.decoder.push_body(&body) {
                    if let Some(reply) = self.apply(tagged) {
                        replies.push((index + 1, reply));
                    }
                }
            } else if reset {
                self.reset();
            }
        }
        replies
    }

    fn apply(&mut self, tagged: TaggedCommand) -> Option<Vec<u8>> {
        let TaggedCommand { client_id, command } = tagged;
        let before = self.registration.clone();
        match command {
            ProgramCommand::Query => {
                let mut reply = Vec::new();
                TerminalEvent::Supported {
                    options: Vec::new(),
                }
                .encode(client_id, &mut reply);
                return Some(reply);
            }
            ProgramCommand::AcceptDrops { mimes } => {
                let machine_id = self
                    .registration
                    .drops
                    .take()
                    .and_then(|drops| drops.machine_id);
                self.registration.drops = Some(DropRegistration {
                    client_id,
                    mimes,
                    machine_id,
                });
            }
            ProgramCommand::DropMachineId(id) => {
                let drops = self
                    .registration
                    .drops
                    .get_or_insert_with(|| DropRegistration {
                        client_id,
                        ..DropRegistration::default()
                    });
                drops.machine_id = (!id.is_empty()).then_some(id);
            }
            ProgramCommand::StopAcceptingDrops => self.registration.drops = None,
            ProgramCommand::OfferDrags { machine_id } => {
                self.registration.drags = Some(DragRegistration {
                    client_id,
                    machine_id,
                });
            }
            ProgramCommand::StopOfferingDrags => self.registration.drags = None,
            command => self.commands.push(TaggedCommand { client_id, command }),
        }
        if self.registration != before {
            self.generation = self.generation.wrapping_add(1);
        }
        None
    }

    pub(super) fn registration(&self) -> &ChildRegistration {
        &self.registration
    }

    pub(super) fn generation(&self) -> u64 {
        self.generation
    }

    pub(super) fn drain(&mut self) -> Vec<TaggedCommand> {
        std::mem::take(&mut self.commands)
    }

    /// Forget what the program registered: it has gone, or the shell is back at its prompt.
    pub(super) fn clear_registration(&mut self) {
        if !self.registration.is_empty() {
            self.registration = ChildRegistration::default();
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// A full reset of the terminal: partial escape codes go, and so does the registration.
    pub(super) fn reset(&mut self) {
        self.after_escape = false;
        self.scanner = OscScanner::default();
        self.decoder = CommandDecoder::default();
        self.commands.clear();
        self.clear_registration();
    }
}

#[cfg(test)]
mod tests {
    use crate::dnd::{DataTarget, DropOperation, ProgramCommand};
    use crate::widgets::terminal::TerminalScreen;

    fn screen() -> TerminalScreen {
        let mut screen = TerminalScreen::new(24, 80, 100);
        screen.set_drag_and_drop_enabled(true);
        screen
    }

    fn send(screen: &mut TerminalScreen, command: ProgramCommand, client_id: Option<u32>) {
        let mut bytes = Vec::new();
        command.encode(client_id, &mut bytes);
        screen.process_bytes(&bytes);
    }

    #[test]
    fn the_query_is_answered_before_a_later_device_attributes_request() {
        let mut screen = screen();
        screen.process_bytes(b"\x1b]72;t=q:i=5\x1b\\\x1b[c");
        let responses = screen.drain_responses();
        let joined: Vec<u8> = responses.concat();
        let query = joined
            .windows(5)
            .position(|w| w == b"\x1b]72;")
            .expect("the query is answered");
        let attributes = joined
            .windows(3)
            .position(|w| w == b"\x1b[?")
            .expect("the device attributes are answered");
        assert!(query < attributes, "{}", String::from_utf8_lossy(&joined));
        assert!(String::from_utf8_lossy(&joined).contains("t=q:i=5"));
    }

    #[test]
    fn a_disabled_screen_neither_answers_nor_records() {
        let mut screen = TerminalScreen::new(24, 80, 100);
        screen.process_bytes(b"\x1b]72;t=q\x1b\\\x1b]72;t=a;text/plain\x1b\\");
        assert!(screen.drain_responses().concat().is_empty());
        assert!(screen.drag_and_drop_registration().is_empty());
    }

    #[test]
    fn registration_is_kept_and_other_commands_queue_in_order() {
        let mut screen = screen();
        let generation = screen.drag_and_drop_generation();
        send(
            &mut screen,
            ProgramCommand::AcceptDrops {
                mimes: vec!["text/uri-list".into()],
            },
            Some(3),
        );
        send(
            &mut screen,
            ProgramCommand::DropMachineId("1:abcdef0123456789abcdef".into()),
            Some(3),
        );
        send(
            &mut screen,
            ProgramCommand::OfferDrags { machine_id: None },
            Some(3),
        );
        send(
            &mut screen,
            ProgramCommand::DropStatus {
                operation: DropOperation::Copy,
                mimes: None,
            },
            Some(3),
        );
        send(
            &mut screen,
            ProgramCommand::RequestData(DataTarget::Mime { index: 1 }),
            Some(3),
        );
        let registration = screen.drag_and_drop_registration().clone();
        let drops = registration.drops.expect("accepting drops");
        assert_eq!(drops.client_id, Some(3));
        assert_eq!(drops.mimes.len(), 1);
        assert_eq!(
            drops.machine_id.as_deref(),
            Some("1:abcdef0123456789abcdef")
        );
        assert!(registration.drags.is_some());
        assert_ne!(screen.drag_and_drop_generation(), generation);
        let commands: Vec<_> = screen
            .drain_drag_and_drop_commands()
            .into_iter()
            .map(|tagged| tagged.command)
            .collect();
        assert!(matches!(commands[0], ProgramCommand::DropStatus { .. }));
        assert!(matches!(commands[1], ProgramCommand::RequestData(_)));
        assert!(screen.drain_drag_and_drop_commands().is_empty());
    }

    #[test]
    fn a_prompt_or_a_reset_forgets_the_registration() {
        let mut screen = screen();
        send(
            &mut screen,
            ProgramCommand::AcceptDrops { mimes: vec![] },
            None,
        );
        assert!(!screen.drag_and_drop_registration().is_empty());
        screen.process_bytes(b"\x1b]133;A\x07$ ");
        assert!(
            screen.drag_and_drop_registration().is_empty(),
            "the shell is back"
        );

        send(
            &mut screen,
            ProgramCommand::OfferDrags { machine_id: None },
            None,
        );
        screen.process_bytes(b"\x1bc");
        assert!(
            screen.drag_and_drop_registration().is_empty(),
            "a full reset"
        );

        send(
            &mut screen,
            ProgramCommand::AcceptDrops { mimes: vec![] },
            None,
        );
        screen.clear_drag_and_drop_registration();
        assert!(
            screen.drag_and_drop_registration().is_empty(),
            "the embedder said so"
        );
    }
}
