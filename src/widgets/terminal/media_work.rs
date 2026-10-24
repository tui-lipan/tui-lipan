//! Sendable acquisition/encoding work; the retained parser and its transactions stay on its owner.
use std::collections::VecDeque;
use std::sync::Arc;

use super::graphics::GraphicsCommand;
use super::{GraphicsMediaPolicy, GraphicsRelayPolicy};

/// An input prefix ending at one named-media command, ready for an embedder's bounded worker queue.
///
/// Run this outside the screen's caller thread. Until its result is applied, preserve stream
/// order: do not feed more bytes, resize or reset that screen. Reconcile relay-policy changes with
/// [`TerminalPreparedInput::with_relay_policy`] before commit. Keep the
/// embedder's ingress budget charged while the batch is pending. Dropping an unstarted work item
/// does not claim its sources; running it consumes accepted temporary files and shared memory.
#[derive(Debug)]
pub struct TerminalMediaWork {
    pub(super) bytes: Arc<[u8]>,
    pub(super) length: usize,
    pub(super) screen_id: u64,
    pub(super) input_revision: u64,
    pub(super) commands: Vec<GraphicsCommand>,
    pub(super) media: GraphicsMediaPolicy,
    pub(super) relay: GraphicsRelayPolicy,
    pub(super) recording: bool,
}

impl TerminalMediaWork {
    /// Acquire, validate/decode, compress, encode and chunk named media without touching a screen.
    pub fn run(mut self) -> TerminalPreparedInput {
        for command in &mut self.commands {
            command.prepare_media(self.media, self.relay, self.recording);
        }
        TerminalPreparedInput {
            bytes: self.bytes,
            length: self.length,
            media: self.media,
            relay: self.relay,
            recording: self.recording,
            screen_id: self.screen_id,
            input_revision: self.input_revision,
            commands: self.commands.into(),
        }
    }
}

/// Prepared bytes and media to commit atomically through their original screen's parser.
///
/// This contains owned pixels, never a second consuming reference. Applying it uses the ordinary
/// parser and output-transaction classifier, including surrounding text, queries and sync frames.
#[derive(Debug)]
pub struct TerminalPreparedInput {
    pub(super) media: GraphicsMediaPolicy,
    pub(super) relay: GraphicsRelayPolicy,
    pub(super) recording: bool,
    pub(super) bytes: Arc<[u8]>,
    pub(super) length: usize,
    pub(super) screen_id: u64,
    pub(super) input_revision: u64,
    pub(super) commands: VecDeque<GraphicsCommand>,
}

impl TerminalPreparedInput {
    /// Number of bytes committed from the original batch. Feed the remaining tail afterward.
    pub fn consumed_bytes(&self) -> usize {
        self.length
    }

    /// Relay policy used by the worker that produced this input.
    pub fn relay_policy(&self) -> GraphicsRelayPolicy {
        self.relay
    }

    /// Rebuild only the recorded representation after attachments change, using acquired pixels.
    /// No source is reopened or consumed a second time. Run the returned work off the caller thread.
    pub fn with_relay_policy(self, relay: GraphicsRelayPolicy) -> TerminalMediaWork {
        TerminalMediaWork {
            bytes: self.bytes,
            length: self.length,
            screen_id: self.screen_id,
            input_revision: self.input_revision,
            commands: self.commands.into(),
            media: self.media,
            relay,
            recording: self.recording,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::TerminalScreen;
    use crate::{GraphicsDependency, TerminalOutputClass};
    use base64::Engine as _;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Source(std::path::PathBuf);

    impl Source {
        fn new(pixels: &[u8]) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "tty-graphics-protocol-work-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::write(&path, pixels).unwrap();
            Self(path)
        }
        fn command(&self, medium: char) -> Vec<u8> {
            let name = base64::engine::general_purpose::STANDARD
                .encode(self.0.to_string_lossy().as_bytes());
            format!("\x1b_Ga=T,t={medium},f=32,s=1,v=1,i=9,p=1,c=1,r=1,C=1,q=2;{name}\x1b\\")
                .into_bytes()
        }
    }
    impl Drop for Source {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    fn screen(policy: GraphicsRelayPolicy) -> TerminalScreen {
        let mut screen = TerminalScreen::new(5, 20, 100);
        screen.set_image_relay_policy(policy);
        screen.set_output_transaction_recording(true);
        screen
    }

    fn output(screen: &mut TerminalScreen) -> Vec<(u8, Vec<u8>)> {
        screen
            .take_output_transactions()
            .into_iter()
            .map(|transaction| {
                let class = match transaction.class {
                    TerminalOutputClass::Ordered => 0,
                    TerminalOutputClass::Graphics {
                        dependency: GraphicsDependency::DependsOnPrevious,
                        ..
                    } => 1,
                    TerminalOutputClass::Graphics {
                        dependency: GraphicsDependency::Independent,
                        ..
                    } => 2,
                };
                (class, transaction.bytes)
            })
            .collect()
    }

    #[test]
    fn worker_claims_source_once_and_commit_retains_the_image() {
        let source = Source::new(&[255, 0, 0, 255]);
        let bytes: Arc<[u8]> = source.command('t').into();
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        let work = screen.prepare_media_input(bytes).unwrap();
        assert!(source.0.exists(), "building work must not claim a source");
        let prepared = std::thread::spawn(move || work.run()).join().unwrap();
        assert!(!source.0.exists());
        assert!(
            !screen.has_images(),
            "only the owner commits the retained state"
        );
        screen.process_prepared_media_input(prepared).unwrap();
        assert!(screen.has_images());
        let wire: Vec<_> = output(&mut screen)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert!(!wire.windows(3).any(|key| key == b"t=t"));
        let mut receiver = TerminalScreen::new(5, 20, 100);
        receiver.set_image_media_policy(GraphicsMediaPolicy::NONE);
        receiver.process_bytes(&wire);
        assert!(receiver.has_images());
        assert!(!screen.export_replay_bytes().is_empty());
    }

    #[test]
    fn deferred_input_preserves_order_and_classification_at_every_split() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut bytes = b"before\x1b[?2026h".to_vec();
        bytes.extend(source.command('f'));
        bytes.extend(b"\x1b[?2026l\x1b[6nafter");
        for split in 0..=bytes.len() {
            let mut synchronous = screen(GraphicsRelayPolicy::All);
            let mut deferred = screen(GraphicsRelayPolicy::All);
            let mut expected = Vec::new();
            let mut actual = Vec::new();
            for part in [&bytes[..split], &bytes[split..]] {
                synchronous.process_bytes(part);
                expected.extend(output(&mut synchronous));
                let mut pending: Arc<[u8]> = part.into();
                while let Some(work) = deferred.prepare_media_input(Arc::clone(&pending)) {
                    let prepared = work.run();
                    let consumed = prepared.consumed_bytes();
                    deferred.process_prepared_media_input(prepared).unwrap();
                    actual.extend(output(&mut deferred));
                    pending = pending[consumed..].into();
                }
                deferred.process_bytes(&pending);
                actual.extend(output(&mut deferred));
            }
            assert_eq!(actual, expected, "split {split}");
            assert_eq!(deferred.drain_responses(), synchronous.drain_responses());
            assert_eq!(
                deferred.export_replay_bytes(),
                synchronous.export_replay_bytes()
            );
        }
    }

    #[test]
    fn a_burst_prepares_only_one_named_image_at_a_time() {
        let first = Source::new(&[255, 0, 0, 255]);
        let second = Source::new(&[0, 255, 0, 255]);
        let mut bytes = first.command('t');
        let expected = bytes.len();
        bytes.extend(second.command('t'));
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        let prepared = screen.prepare_media_input(bytes.into()).unwrap().run();
        assert_eq!(prepared.consumed_bytes(), expected);
        assert!(!first.0.exists());
        assert!(second.0.exists());
        screen.process_prepared_media_input(prepared).unwrap();
    }

    #[test]
    fn changed_attachment_policy_reuses_acquired_pixels_after_source_is_gone() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        let prepared = screen
            .prepare_media_input(source.command('f').into())
            .unwrap()
            .run();
        std::fs::remove_file(&source.0).unwrap();
        let prepared = prepared.with_relay_policy(GraphicsRelayPolicy::All).run();
        screen.set_image_relay_policy(GraphicsRelayPolicy::All);
        screen.process_prepared_media_input(prepared).unwrap();
        let wire: Vec<_> = output(&mut screen)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert!(!wire.windows(3).any(|key| key == b"t=f"));
        let mut receiver = TerminalScreen::new(5, 20, 100);
        receiver.set_image_media_policy(GraphicsMediaPolicy::NONE);
        receiver.process_bytes(&wire);
        assert!(receiver.has_images());
    }

    #[test]
    fn stream_barriers_reject_a_stale_preparation() {
        for barrier in [0, 1, 2] {
            let source = Source::new(&[255, 0, 0, 255]);
            let mut screen = screen(GraphicsRelayPolicy::All);
            let prepared = screen
                .prepare_media_input(source.command('f').into())
                .unwrap()
                .run();
            match barrier {
                0 => {
                    screen.process_bytes(b"later");
                }
                1 => screen.resize(6, 20),
                _ => screen.reset(),
            }
            assert!(screen.process_prepared_media_input(prepared).is_err());
            assert!(!screen.has_images());
        }
    }

    #[test]
    fn consuming_queries_and_failures_never_record_a_consuming_reference() {
        for contents in [&[255, 0, 0, 255][..], &[][..]] {
            let source = Source::new(contents);
            let mut command = source.command('t');
            let action = command.windows(3).position(|key| key == b"a=T").unwrap();
            command[action + 2] = b'q';
            let mut screen = screen(GraphicsRelayPolicy::Consuming);
            screen.process_bytes(&command);
            assert!(!source.0.exists());
            let transactions = output(&mut screen);
            if contents.is_empty() {
                assert_eq!(transactions, vec![(0, Vec::new())]);
            }
            let wire: Vec<_> = transactions
                .into_iter()
                .flat_map(|(_, bytes)| bytes)
                .collect();
            assert!(!wire.windows(3).any(|key| key == b"t=t"));
            assert!(!screen.has_images(), "a query must not retain an image");
        }
    }
}
