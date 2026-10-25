//! Sendable acquisition/encoding work; the retained parser and its transactions stay on its owner.
use std::collections::VecDeque;
use std::sync::Arc;

use super::graphics::{GraphicsCommand, GraphicsScanner, GraphicsSegment, MediaFiles, RelayBase};
use super::{GraphicsMediaPolicy, GraphicsRelayPolicy};

/// An input prefix ending at one named-media command, ready for an embedder's bounded worker queue.
///
/// Run this outside the screen's caller thread. Until its result is applied, preserve stream
/// order: do not feed more bytes, resize or reset that screen. Reconcile relay-policy changes with
/// [`TerminalPreparedInput::with_relay_policy`] before commit. Keep the
/// embedder's ingress budget charged while the batch is pending. Dropping an unstarted work item
/// does not claim its sources; running it consumes accepted temporary files and shared memory.
///
/// When the input already holds later silent replacements of the same image, the batch extends
/// over them and only the newest one that can be read is acquired. The sources of the earlier
/// ones are claimed unread and the parser skips them, which leaves the same state as applying
/// every one. A producer that outpaces its reader therefore costs one acquisition per batch.
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
    pub(super) files: Option<MediaFiles>,
    /// Indices into `commands` of successive replacements of one image, oldest first.
    pub(super) replacements: Vec<usize>,
    /// What readers hold for the image the batch ends by replacing, when nothing earlier in the
    /// batch touches it. Inline relay then sends only what changed.
    pub(super) base: Option<Box<RelayBase>>,
}

impl TerminalMediaWork {
    /// Acquire and validate named media without touching a screen. Raw pixels go to an immutable
    /// file snapshot when the screen stores images as files; otherwise they are decoded. Output
    /// that relays a source is rewritten too: compressed, encoded and chunked when inline.
    pub fn run(mut self) -> TerminalPreparedInput {
        let targets = edit_targets(&self.replacements, self.commands.len());
        let base = self.base.as_deref();
        // Newest first: the first replacement that succeeds decides the image, so every older
        // one is unobservable. Newer ones that failed stay in place and fail as they would have.
        for (position, &index) in self.replacements.iter().enumerate().rev() {
            let command = &mut self.commands[index];
            command.prepare_media(
                self.media,
                self.relay,
                self.recording,
                self.files.as_ref(),
                base,
            );
            if command.prepared_successfully() {
                for &older in &self.replacements[..position] {
                    self.commands[older].supersede(self.media);
                }
                break;
            }
        }
        for (index, command) in self.commands.iter_mut().enumerate() {
            command.prepare_media(
                self.media,
                self.relay,
                self.recording,
                self.files.as_ref(),
                base.filter(|_| targets.contains(&index)),
            );
        }
        TerminalPreparedInput {
            bytes: self.bytes,
            length: self.length,
            media: self.media,
            relay: self.relay,
            recording: self.recording,
            files: self.files,
            screen_id: self.screen_id,
            input_revision: self.input_revision,
            commands: self.commands.into(),
            replacements: self.replacements.into_boxed_slice(),
            base: self.base,
        }
    }
}

/// The commands a relay base applies to: every link of a replacement chain, or else the command
/// that ends the batch.
fn edit_targets(replacements: &[usize], commands: usize) -> Vec<usize> {
    if replacements.is_empty() {
        commands.checked_sub(1).into_iter().collect()
    } else {
        replacements.to_vec()
    }
}

/// [`RelayBase`] for a batch, when every command before its final replacement leaves that image
/// alone, so readers will hold exactly the base when the replacement arrives.
pub(super) fn batch_relay_base(
    graphics: &super::graphics::TerminalGraphics,
    commands: &[GraphicsCommand],
    replacements: &[usize],
) -> Option<Box<RelayBase>> {
    let first = *edit_targets(replacements, commands.len()).first()?;
    let id = commands[first].transmitted_id()?;
    commands[..first]
        .iter()
        .all(|command| command.replaces_whole_image() && command.transmitted_id() != Some(id))
        .then(|| graphics.relay_base(id))
        .flatten()
        .map(Box::new)
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
    pub(super) files: Option<MediaFiles>,
    pub(super) bytes: Arc<[u8]>,
    pub(super) length: usize,
    pub(super) screen_id: u64,
    pub(super) input_revision: u64,
    pub(super) commands: VecDeque<GraphicsCommand>,
    pub(super) replacements: Box<[usize]>,
    pub(super) base: Option<Box<RelayBase>>,
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
            files: self.files,
            replacements: self.replacements.into_vec(),
            base: self.base,
        }
    }
}

/// Extend a media prefix that ends with a whole-image replacement over later complete
/// replacements of that image in `bytes`. Returns the chain's command indices, oldest first, or
/// nothing when no later replacement qualifies.
///
/// Between two links only text and replacements of other images may appear: anything else could
/// observe the earlier frame. Each link is a complete command, so the batch never ends inside one.
pub(super) fn extend_replacements(
    scanner: &mut GraphicsScanner,
    bytes: &[u8],
    commands: &mut Vec<GraphicsCommand>,
    length: &mut usize,
) -> Vec<usize> {
    let Some(id) = commands
        .last()
        .filter(|last| last.needs_media() && last.replaces_whole_image())
        .and_then(GraphicsCommand::transmitted_id)
    else {
        return Vec::new();
    };
    let mut chain = vec![commands.len() - 1];
    while *length < bytes.len() {
        let mut ahead = scanner.clone();
        let (segments, used) = ahead.scan_media_prefix(&bytes[*length..]);
        let mut found = Vec::new();
        for segment in segments {
            match segment {
                GraphicsSegment::Text(_) | GraphicsSegment::HeldEscape => {}
                GraphicsSegment::Command(command) => found.push(*command),
                GraphicsSegment::Wire(_) => return finish(chain),
            }
        }
        let Some((next, between)) = found.split_last() else {
            break;
        };
        let links = next.needs_media()
            && next.replaces_whole_image()
            && next.transmitted_id() == Some(id)
            && between.iter().all(|command| {
                command.replaces_whole_image() && command.transmitted_id() != Some(id)
            });
        if !links {
            break;
        }
        chain.push(commands.len() + between.len());
        commands.extend(found);
        *length += used;
        *scanner = ahead;
    }
    finish(chain)
}

fn finish(chain: Vec<usize>) -> Vec<usize> {
    if chain.len() < 2 { Vec::new() } else { chain }
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
            self.command_for(medium, 9)
        }
        fn command_for(&self, medium: char, id: u32) -> Vec<u8> {
            let name = base64::engine::general_purpose::STANDARD
                .encode(self.0.to_string_lossy().as_bytes());
            format!("\x1b_Ga=T,t={medium},f=32,s=1,v=1,i={id},p=1,c=1,r=1,C=1,q=2;{name}\x1b\\")
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
                    TerminalOutputClass::Graphics {
                        dependency: GraphicsDependency::EditsPrevious,
                        ..
                    } => 3,
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

    fn pixel(screen: &TerminalScreen) -> Vec<u8> {
        screen.capture_frame().images[0].rgba[..4].to_vec()
    }

    #[test]
    fn a_burst_of_one_image_acquires_only_its_newest_frame() {
        let frames: Vec<_> = (0..4u8).map(|n| Source::new(&[n, 0, 0, 255])).collect();
        let mut bytes = Vec::new();
        for (index, frame) in frames.iter().enumerate() {
            bytes.extend(format!("text {index}").bytes());
            bytes.extend(frame.command('t'));
        }
        bytes.extend(b"tail");
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        let work = screen.prepare_media_input(bytes.clone().into()).unwrap();
        assert!(frames.iter().all(|frame| frame.0.exists()));
        let prepared = work.run();
        assert_eq!(prepared.consumed_bytes(), bytes.len() - b"tail".len());
        assert!(
            frames.iter().all(|frame| !frame.0.exists()),
            "every source is claimed"
        );
        screen.process_prepared_media_input(prepared).unwrap();
        assert_eq!(pixel(&screen), [3, 0, 0, 255]);
        let transactions = output(&mut screen);
        let commands = transactions
            .iter()
            .flat_map(|(_, bytes)| bytes.windows(3))
            .filter(|window| *window == b"\x1b_G")
            .count();
        assert_eq!(commands, 1, "superseded frames are never relayed");
        let text: Vec<u8> = transactions
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        for index in 0..4 {
            let marker = format!("text {index}");
            assert!(text.windows(marker.len()).any(|w| w == marker.as_bytes()));
        }
    }

    #[test]
    fn a_failed_newest_frame_falls_back_to_the_previous_replacement() {
        let good = Source::new(&[7, 0, 0, 255]);
        let empty = Source::new(&[]);
        let mut bytes = good.command('t');
        bytes.extend(empty.command('t'));
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        let prepared = screen
            .prepare_media_input(bytes.clone().into())
            .unwrap()
            .run();
        assert_eq!(prepared.consumed_bytes(), bytes.len());
        screen.process_prepared_media_input(prepared).unwrap();
        assert_eq!(pixel(&screen), [7, 0, 0, 255]);
        assert!(!good.0.exists() && !empty.0.exists());
    }

    #[test]
    fn anything_that_could_observe_the_earlier_frame_ends_the_burst() {
        for between in [
            &b"\x1b_Ga=p,i=9,p=2,q=2\x1b\\"[..],
            b"\x1b_Ga=d,d=a,q=2\x1b\\",
        ] {
            let first = Source::new(&[255, 0, 0, 255]);
            let second = Source::new(&[0, 255, 0, 255]);
            let mut bytes = first.command('t');
            let expected = bytes.len();
            bytes.extend(between);
            bytes.extend(second.command('t'));
            let screen = screen(GraphicsRelayPolicy::Consuming);
            let prepared = screen.prepare_media_input(bytes.into()).unwrap().run();
            assert_eq!(prepared.consumed_bytes(), expected);
            assert!(!first.0.exists());
            assert!(second.0.exists());
        }
    }

    /// A `w`x`h` RGBA frame of noise that barely compresses, distinct for each `seed`.
    fn frame_pixels(w: usize, h: usize, seed: u8) -> Vec<u8> {
        let mut state = 0x9e37_79b9_u32 ^ u32::from(seed);
        (0..w * h * 4)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                state as u8
            })
            .collect()
    }

    fn frame_command(source: &Source, w: usize, h: usize) -> Vec<u8> {
        let name =
            base64::engine::general_purpose::STANDARD.encode(source.0.to_string_lossy().as_bytes());
        format!("\x1b_Ga=T,t=t,f=32,s={w},v={h},i=4,p=1,C=1,z=-1,q=2;{name}\x1b\\").into_bytes()
    }

    fn deferred(screen: &mut TerminalScreen, bytes: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut pending: Arc<[u8]> = bytes.into();
        let mut out = Vec::new();
        while let Some(work) = screen.prepare_media_input(Arc::clone(&pending)) {
            let prepared = std::thread::spawn(move || work.run()).join().unwrap();
            let consumed = prepared.consumed_bytes();
            screen.process_prepared_media_input(prepared).unwrap();
            out.extend(output(screen));
            pending = pending[consumed..].into();
        }
        screen.process_bytes(&pending);
        out.extend(output(screen));
        out
    }

    type Placed = (u32, i32, i32, u16, u16, i32, Vec<u8>);

    /// Every visible placement and the RGBA pixels behind it, however they are stored.
    fn placed(screen: &mut TerminalScreen) -> Vec<Placed> {
        screen
            .render_snapshot()
            .images
            .iter()
            .map(|p| {
                let pixels = p.image.pixels().unwrap().to_rgba8().into_raw();
                (p.image_id, p.row, p.col, p.rows, p.cols, p.z, pixels)
            })
            .collect()
    }

    #[test]
    fn an_inline_relay_sends_only_changed_rectangles_and_readers_match_the_server() {
        let (w, h) = (96, 64);
        let first = frame_pixels(w, h, 0);
        let mut small = first.clone();
        small[(10 * w + 20) * 4..(10 * w + 26) * 4].fill(255);
        let mut rewritten = frame_pixels(w, h, 99);
        rewritten[0] = 1;
        let frames = [first.clone(), small.clone(), small, rewritten, first];
        let mut server = TerminalScreen::new(24, 80, 100);
        server.set_image_relay_policy(GraphicsRelayPolicy::All);
        server.set_output_transaction_recording(true);
        let mut reader = TerminalScreen::new(24, 80, 100);
        reader.set_image_media_policy(GraphicsMediaPolicy::NONE);
        // Receives each frame whole, inline: what every reader must end up holding.
        let mut reference = TerminalScreen::new(24, 80, 100);
        let mut sizes = Vec::new();
        let mut classes = Vec::new();
        for (index, pixels) in frames.iter().enumerate() {
            let source = Source::new(pixels);
            // Text between frames, ending at the same cursor so the placement does not move.
            let text = format!("\x1b[2;1Hframe {index}\x1b[1;1H");
            let mut bytes = text.clone().into_bytes();
            bytes.extend(frame_command(&source, w, h));
            let transactions = deferred(&mut server, &bytes);
            let wire: Vec<u8> = transactions.iter().flat_map(|(_, b)| b.clone()).collect();
            assert!(!wire.windows(3).any(|key| key == b"t=t"));
            classes.push(transactions.iter().map(|(class, _)| *class).max().unwrap());
            sizes.push(wire.len());
            reader.process_bytes(&wire);
            let direct = base64::engine::general_purpose::STANDARD.encode(pixels);
            reference.process_bytes(text.as_bytes());
            reference.process_bytes(
                format!("\x1b_Ga=T,f=32,s={w},v={h},i=4,p=1,C=1,z=-1,q=2;{direct}\x1b\\")
                    .as_bytes(),
            );
            let held = placed(&mut reader);
            assert_eq!(held, placed(&mut server), "frame {index}");
            assert_eq!(held, placed(&mut reference), "frame {index}");
            assert_eq!(held.len(), 1);
            assert_eq!(held[0].6, *pixels, "frame {index} pixels");
        }
        // Small and unchanged frames travel as edits of the previous frame, still marked as a
        // stream of whole frames. A rewritten frame travels whole and stays replaceable.
        assert_eq!(classes, [1, 3, 3, 2, 2]);
        assert!(
            sizes[1] * 20 < sizes[0],
            "an edit is far smaller: {sizes:?}"
        );
        assert!(
            sizes[2] * 20 < sizes[0],
            "an unchanged frame sends no pixels: {sizes:?}"
        );
        assert!(!server.export_replay_bytes().is_empty());
    }

    fn direct(keys: &str, pixels: &[u8]) -> Vec<u8> {
        let data = base64::engine::general_purpose::STANDARD.encode(pixels);
        format!("\x1b_G{keys};{data}\x1b\\").into_bytes()
    }

    #[test]
    fn a_root_frame_edit_rewrites_pixels_in_place_and_keeps_the_placement() {
        let mut screen = TerminalScreen::new(24, 80, 100);
        let base: Vec<u8> = [[10, 20, 30, 255]; 8].concat();
        screen.process_bytes(&direct("a=T,f=32,s=4,v=2,i=3,p=1,C=1,q=2", &base));
        let before = placed(&mut screen);
        // Overwrite two pixels of the top row, then blend a half-transparent one below them.
        screen.process_bytes(&direct(
            "a=f,r=1,i=3,x=1,y=0,s=2,v=1,X=1,q=2",
            &[[255, 0, 0, 0], [0, 255, 0, 255]].concat(),
        ));
        screen.process_bytes(&direct(
            "a=f,r=1,i=3,x=1,y=1,s=1,v=1,q=2",
            &[250, 0, 0, 128],
        ));
        let after = placed(&mut screen);
        assert_eq!(
            after.len(),
            1,
            "the placement survives without being made again"
        );
        assert_eq!(
            after[0].0..after[0].6.len() as u32,
            before[0].0..before[0].6.len() as u32
        );
        let pixels = &after[0].6;
        assert_eq!(&pixels[4..8], &[255, 0, 0, 0], "overwrite ignores alpha");
        assert_eq!(&pixels[8..12], &[0, 255, 0, 255]);
        assert_eq!(&pixels[..4], &[10, 20, 30, 255], "pixels outside are kept");
        let blended = &pixels[(4 + 1) * 4..(4 + 1) * 4 + 4];
        assert!(
            blended[0] > 100 && blended[0] < 250 && blended[3] >= 254,
            "{blended:?}"
        );
    }

    #[test]
    fn frame_edits_outside_the_root_frame_or_image_are_refused() {
        let mut screen = TerminalScreen::new(24, 80, 100);
        screen.process_bytes(&direct("a=T,f=32,s=2,v=2,i=3,q=2", &[0; 16]));
        for keys in [
            "a=f,r=2,i=3,s=1,v=1,X=1",
            "a=f,r=1,c=1,i=3,s=1,v=1,X=1",
            "a=f,r=1,i=3,x=2,s=1,v=1,X=1",
            "a=f,r=1,i=9,s=1,v=1,X=1",
        ] {
            screen.process_bytes(&direct(keys, &[1, 2, 3, 4]));
            let responses = String::from_utf8(screen.drain_responses().concat()).unwrap();
            assert!(!responses.contains(";OK"), "{keys}: {responses}");
            assert!(
                responses.contains("ENOTSUPP")
                    || responses.contains("EINVAL")
                    || responses.contains("ENOENT"),
                "{keys}: {responses}"
            );
        }
        assert_eq!(placed_pixels_unchanged(&mut screen), [0; 16]);
    }

    fn placed_pixels_unchanged(screen: &mut TerminalScreen) -> Vec<u8> {
        screen.process_bytes(b"\x1b_Ga=p,i=3,q=2\x1b\\");
        placed(screen)[0].6.clone()
    }

    #[test]
    fn edits_are_not_used_without_a_base_readers_are_known_to_hold() {
        let (w, h) = (32, 16);
        let first = Source::new(&frame_pixels(w, h, 0));
        let second = Source::new(&frame_pixels(w, h, 0));
        let mut server = TerminalScreen::new(24, 80, 100);
        server.set_image_relay_policy(GraphicsRelayPolicy::All);
        server.set_output_transaction_recording(true);
        deferred(&mut server, &frame_command(&first, w, h));
        // A delete of the image earlier in the batch means readers will not hold it.
        let mut bytes = b"\x1b_Ga=d,d=I,i=4,q=2\x1b\\".to_vec();
        bytes.extend(frame_command(&second, w, h));
        let wire: Vec<u8> = deferred(&mut server, &bytes)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert!(!wire.windows(4).any(|key| key == b"a=f,"));
        // A screen that relays files to local readers never sends edits either.
        let mut local = screen(GraphicsRelayPolicy::Consuming);
        let third = Source::new(&frame_pixels(w, h, 0));
        let fourth = Source::new(&frame_pixels(w, h, 0));
        deferred(&mut local, &frame_command(&third, w, h));
        let wire: Vec<u8> = deferred(&mut local, &frame_command(&fourth, w, h))
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert!(!wire.windows(4).any(|key| key == b"a=f,"));
    }

    #[test]
    fn relayed_consuming_replacements_are_independent() {
        let first = Source::new(&[255, 0, 0, 255]);
        let second = Source::new(&[0, 255, 0, 255]);
        let mut screen = screen(GraphicsRelayPolicy::Consuming);
        screen.process_bytes(&first.command('t'));
        screen.process_bytes(&second.command('t'));
        let classes: Vec<_> = output(&mut screen)
            .into_iter()
            .map(|(class, _)| class)
            .collect();
        assert_eq!(classes, [1, 2]);
    }

    #[test]
    fn a_burst_of_distinct_images_prepares_one_named_image_at_a_time() {
        let first = Source::new(&[255, 0, 0, 255]);
        let second = Source::new(&[0, 255, 0, 255]);
        let mut bytes = first.command_for('t', 9);
        let expected = bytes.len();
        bytes.extend(second.command_for('t', 10));
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

    fn file_screen(policy: GraphicsRelayPolicy) -> TerminalScreen {
        let mut screen = screen(policy);
        screen.set_image_file_storage_enabled(true);
        screen
    }

    fn commit(screen: &mut TerminalScreen, bytes: Vec<u8>) -> Vec<u8> {
        let work = screen.prepare_media_input(bytes.into()).unwrap();
        let prepared = std::thread::spawn(move || work.run()).join().unwrap();
        screen.process_prepared_media_input(prepared).unwrap();
        output(screen)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect()
    }

    /// The path a relayed `t=f` transmission names.
    fn relayed_path(wire: &[u8]) -> std::path::PathBuf {
        let text = std::str::from_utf8(wire).unwrap();
        assert!(text.contains("t=f"), "{text}");
        let name = &text[text.find(';').unwrap() + 1..text.len() - 2];
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(name)
            .unwrap();
        std::path::PathBuf::from(String::from_utf8(bytes).unwrap())
    }

    fn shown_pixel(screen: &mut TerminalScreen) -> Vec<u8> {
        screen.capture_frame().images[0].rgba[..4].to_vec()
    }

    #[test]
    fn local_relay_names_an_immutable_snapshot_instead_of_inline_pixels() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut screen = file_screen(GraphicsRelayPolicy::NamedAsFiles);
        let wire = commit(&mut screen, source.command('t'));
        assert!(!source.0.exists(), "the screen claims the source once");
        let snapshot = relayed_path(&wire);
        assert_ne!(snapshot, source.0);
        assert!(
            screen.render_snapshot().images[0]
                .image
                .file_pixels()
                .is_some(),
            "the owner keeps the pixels in the same file, not on the heap"
        );
        for _ in 0..2 {
            let mut reader = TerminalScreen::new(5, 20, 100);
            reader.set_image_media_policy(GraphicsMediaPolicy::SHARED);
            reader.process_bytes(&wire);
            assert_eq!(shown_pixel(&mut reader), [255, 0, 0, 255]);
        }
        // Late readers replay inline pixels, never the snapshot path or the consumed source.
        std::fs::write(&source.0, [0, 255, 0, 255]).unwrap();
        let replay = screen.export_replay_bytes();
        assert!(!replay.windows(3).any(|key| key == b"t=f" || key == b"t=t"));
        let mut late = TerminalScreen::new(5, 20, 100);
        late.set_image_media_policy(GraphicsMediaPolicy::NONE);
        late.process_bytes(&replay);
        assert_eq!(shown_pixel(&mut late), [255, 0, 0, 255]);
    }

    #[test]
    fn replaced_relay_snapshots_stay_readable_and_are_never_rewritten() {
        let mut screen = file_screen(GraphicsRelayPolicy::NamedAsFiles);
        let mut relayed = Vec::new();
        let mut retained = Vec::new();
        for frame in 0..8u8 {
            let source = Source::new(&[frame, frame, frame, 255]);
            let work = screen
                .prepare_media_input(source.command('t').into())
                .unwrap();
            screen.process_prepared_media_input(work.run()).unwrap();
            let transactions = screen.take_output_transactions();
            let wire: Vec<_> = transactions
                .iter()
                .flat_map(|t| t.bytes.iter().copied())
                .collect();
            let path = relayed_path(&wire);
            retained.extend(transactions);
            assert!(!relayed.contains(&path), "a named snapshot is never reused");
            relayed.push(path);
            // A reader still catching up on earlier output finds each frame it was promised.
            for (earlier, path) in relayed.iter().enumerate() {
                let value = earlier as u8;
                assert_eq!(std::fs::read(path).unwrap(), [value, value, value, 255]);
            }
        }
        screen.reset();
        assert!(relayed.iter().all(|path| path.exists()));
        drop(retained);
        assert!(relayed.iter().all(|path| !path.exists()));
    }

    #[test]
    fn local_relay_without_file_storage_falls_back_to_inline_pixels() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut screen = screen(GraphicsRelayPolicy::NamedAsFiles);
        let wire = commit(&mut screen, source.command('t'));
        assert!(!wire.windows(3).any(|key| key == b"t=f" || key == b"t=t"));
        let mut reader = TerminalScreen::new(5, 20, 100);
        reader.set_image_media_policy(GraphicsMediaPolicy::NONE);
        reader.process_bytes(&wire);
        assert_eq!(shown_pixel(&mut reader), [255, 0, 0, 255]);
    }

    #[test]
    fn a_remote_reader_arriving_mid_preparation_gets_inline_pixels() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut screen = file_screen(GraphicsRelayPolicy::NamedAsFiles);
        let prepared = screen
            .prepare_media_input(source.command('t').into())
            .unwrap()
            .run();
        let prepared = prepared.with_relay_policy(GraphicsRelayPolicy::All).run();
        screen.set_image_relay_policy(GraphicsRelayPolicy::All);
        screen.process_prepared_media_input(prepared).unwrap();
        let wire: Vec<_> = output(&mut screen)
            .into_iter()
            .flat_map(|(_, bytes)| bytes)
            .collect();
        assert!(!wire.windows(2).any(|key| key == b"t="));
        let mut reader = TerminalScreen::new(5, 20, 100);
        reader.set_image_media_policy(GraphicsMediaPolicy::NONE);
        reader.process_bytes(&wire);
        assert_eq!(shown_pixel(&mut reader), [255, 0, 0, 255]);
    }

    #[test]
    fn prepared_reusable_files_keep_file_storage() {
        let source = Source::new(&[255, 0, 0, 255]);
        let mut screen = file_screen(GraphicsRelayPolicy::None);
        let wire = commit(&mut screen, source.command('f'));
        assert_eq!(
            wire,
            source.command('f'),
            "local readers reopen the original file"
        );
        assert!(
            screen.render_snapshot().images[0]
                .image
                .file_pixels()
                .is_some()
        );
        std::fs::write(&source.0, [0, 255, 0, 255]).unwrap();
        assert_eq!(shown_pixel(&mut screen), [255, 0, 0, 255]);
    }

    #[test]
    fn file_backed_preparation_answers_every_source_length_like_memory() {
        for contents in [
            &[1, 2, 3, 255][..],
            &[1, 2, 3, 255, 9][..],
            &[1, 2][..],
            &[][..],
        ] {
            let mut results = Vec::new();
            for files in [false, true] {
                let source = Source::new(contents);
                let command = String::from_utf8(source.command('t'))
                    .unwrap()
                    .replace("q=2", "q=0")
                    .into_bytes();
                let mut screen = screen(GraphicsRelayPolicy::NamedAsFiles);
                screen.set_image_file_storage_enabled(files);
                let work = screen.prepare_media_input(command.into()).unwrap();
                screen.process_prepared_media_input(work.run()).unwrap();
                assert!(!source.0.exists());
                let image = screen
                    .capture_frame()
                    .images
                    .first()
                    .map(|image| image.rgba.clone());
                results.push((image, screen.drain_responses()));
            }
            assert_eq!(results[0], results[1], "{contents:?}");
        }
    }
}

#[cfg(test)]
mod immutable_tests {
    use super::super::*;
    use base64::Engine as _;

    fn output(sync: bool) -> (TerminalScreen, TerminalOutputTransaction) {
        let source = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(source.path(), [1, 2, 3, 255]).unwrap();
        let name = base64::engine::general_purpose::STANDARD
            .encode(source.path().to_string_lossy().as_bytes());
        let command = format!("\x1b_Ga=T,t=f,f=32,s=1,v=1,i=9,C=1,q=2;{name}\x1b\\");
        let bytes = if sync {
            format!("\x1b[?2026h\x1b[1;1H{command}\x1b[?2026l")
        } else {
            format!("text{command}")
        };
        let mut screen = TerminalScreen::new(5, 20, 100);
        screen.set_image_file_storage_enabled(true);
        screen.set_output_transaction_recording(true);
        screen.set_image_relay_policy(GraphicsRelayPolicy::NamedAsFiles);
        screen.process_bytes(bytes.as_bytes());
        let mut transactions = screen.take_output_transactions();
        let at = transactions
            .iter()
            .position(|t| !t.media.is_empty())
            .expect("resource-bearing output");
        (screen, transactions.remove(at))
    }

    #[test]
    fn immutable_binding_rebases_through_synchronized_output() {
        let (screen, transaction) = output(true);
        assert!(transaction.bytes.starts_with(b"\x1b[?2026h\x1b[1;1H"));
        assert_eq!(transaction.media.len(), 1);
        let binding = &transaction.media[0];
        assert_eq!(
            &transaction.bytes[binding.command_range.start..binding.command_range.start + 3],
            b"\x1b_G"
        );
        assert_eq!(
            &transaction.bytes[binding.command_range.end - 2..binding.command_range.end],
            b"\x1b\\"
        );
        let path = binding.file.path().to_owned();
        drop(screen);
        assert_eq!(std::fs::read(&path).unwrap(), [1, 2, 3, 255]);
        drop(transaction);
        assert!(!path.exists());
    }

    #[test]
    fn imported_binding_survives_server_drop_and_uses_revision_fence() {
        let (server, transaction) = output(false);
        let source = &transaction.media[0];
        let imported = TerminalImmutableFile::acquire(source.file.path(), 4, 32).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!(
                std::fs::metadata(source.file.path()).unwrap().ino(),
                std::fs::metadata(imported.path()).unwrap().ino()
            );
        }
        let imported_path = imported.path().to_owned();
        let mut client = TerminalScreen::new(5, 20, 100);
        client.set_image_file_storage_enabled(true);
        let bytes = std::sync::Arc::from(transaction.bytes.clone());
        let binding = TerminalOutputMedia {
            command_range: source.command_range.clone(),
            file: imported,
        };
        let prepared = client
            .prepare_immutable_media_input(bytes, vec![binding])
            .unwrap();
        drop(transaction);
        drop(server);
        client.process_prepared_media_input(prepared).unwrap();
        assert!(client.has_images());
        let snapshot = client.render_snapshot();
        let pixels = snapshot.images[0].image.file_pixels().unwrap();
        assert_eq!(
            pixels.path(),
            imported_path,
            "native handoff uses the acquired handle"
        );
        let host = pixels.host_link().unwrap();
        assert_eq!(std::fs::read(host.path()).unwrap(), [1, 2, 3, 255]);
    }

    #[test]
    fn imported_input_rebuilds_relay_after_attachments_change_without_reopening_source() {
        let (server, transaction) = output(false);
        let source = &transaction.media[0];
        let original_path = source.file.path().to_owned();
        let imported = TerminalImmutableFile::acquire(&original_path, 4, 32).unwrap();
        let binding = TerminalOutputMedia {
            command_range: source.command_range.clone(),
            file: imported,
        };
        let mut client = TerminalScreen::new(5, 20, 100);
        client.set_image_file_storage_enabled(true);
        client.set_output_transaction_recording(true);
        client.set_image_relay_policy(GraphicsRelayPolicy::NamedAsFiles);
        let prepared = client
            .prepare_immutable_media_input(
                std::sync::Arc::from(transaction.bytes.clone()),
                vec![binding],
            )
            .unwrap();
        drop(transaction);
        drop(server);
        assert!(!original_path.exists());
        let prepared = prepared.with_relay_policy(GraphicsRelayPolicy::All).run();
        client.set_image_relay_policy(GraphicsRelayPolicy::All);
        client.process_prepared_media_input(prepared).unwrap();
        let wire: Vec<_> = client
            .take_output_transactions()
            .into_iter()
            .flat_map(|t| t.bytes)
            .collect();
        assert!(!wire.is_empty(), "relay must not suppress acquired media");
        assert!(!wire.windows(3).any(|key| key == b"t=f"));
        let mut reader = TerminalScreen::new(5, 20, 100);
        reader.set_image_media_policy(GraphicsMediaPolicy::NONE);
        reader.process_bytes(&wire);
        assert!(reader.has_images(), "rebuilt relay must carry pixels");
        assert_eq!(&reader.capture_frame().images[0].rgba[..4], &[1, 2, 3, 255]);
    }

    #[test]
    fn ordinary_bytes_copy_server_paths_without_adopting_them() {
        let (_server, transaction) = output(false);
        let mut client = TerminalScreen::new(5, 20, 100);
        client.set_image_file_storage_enabled(true);
        client.process_bytes(&transaction.bytes);
        let snapshot = client.render_snapshot();
        let pixels = snapshot.images[0].image.file_pixels().unwrap();
        assert_ne!(pixels.path(), transaction.media[0].file.path());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(
                std::fs::metadata(pixels.path()).unwrap().ino(),
                std::fs::metadata(transaction.media[0].file.path())
                    .unwrap()
                    .ino()
            );
        }
    }

    #[test]
    fn immutable_import_rejects_wrong_length_format_and_command_binding() {
        let (_server, transaction) = output(false);
        let binding = &transaction.media[0];
        assert!(TerminalImmutableFile::acquire(binding.file.path(), 8, 32).is_err());
        assert!(TerminalImmutableFile::acquire(binding.file.path(), 4, 100).is_err());
        let client = TerminalScreen::new(5, 20, 100);
        let mut invalid = binding.clone();
        invalid.command_range.start += 1;
        assert!(
            client
                .prepare_immutable_media_input(
                    std::sync::Arc::from(transaction.bytes.clone()),
                    vec![invalid]
                )
                .is_err()
        );
        let prepared = client
            .prepare_immutable_media_input(
                std::sync::Arc::from(transaction.bytes.clone()),
                transaction.media.clone(),
            )
            .unwrap();
        let mut client = client;
        client.process_bytes(b"later");
        assert!(client.process_prepared_media_input(prepared).is_err());
        assert!(!client.has_images());
    }
}
