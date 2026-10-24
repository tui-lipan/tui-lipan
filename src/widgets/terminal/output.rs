//! Lossless output transactions classified by the retained parser.
use std::collections::BTreeMap;

use super::graphics::FrameOp;
use super::{ImageSourceKey, ImageSourceRevision};

/// Whether a completed graphics transaction can supersede earlier ones.
///
/// The guarantee covers a run of consecutive [`Independent`](Self::Independent) transactions from
/// one screen. A run holds a single source: a later transaction makes every earlier one in the
/// run redundant, so a consumer may keep only the newest. Any other transaction, including a
/// replacement of a different source, ends the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsDependency {
    /// This transaction completely determines everything it changes: either one command
    /// replacing its source without changing its placements, or a whole synchronized update
    /// replacing one image.
    Independent,
    /// Its effects require preceding output; preserve byte order and every transaction.
    DependsOnPrevious,
}

/// Semantic classification of original producer bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOutputClass {
    /// Text, controls, queries, incomplete/chunked or uncertain operations.
    Ordered,
    /// A successfully completed source mutation, or a synchronized update proven to be one
    /// replacement, with explicit dependency information.
    Graphics {
        /// Identity in the authoritative retained screen.
        source: ImageSourceKey,
        /// Source revision after the operation.
        revision: ImageSourceRevision,
        /// Only independent transactions may replace pending earlier transactions.
        dependency: GraphicsDependency,
    },
}

/// Exact original bytes of one ordered segment, completed graphics command or replacement
/// synchronized update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOutputTransaction {
    /// Parser-established semantics; defaults to ordered when equivalence is uncertain.
    pub class: TerminalOutputClass,
    /// Original bytes, unchanged and in producer order, except that a screen relaying out-of-band
    /// images inline records each relayed transmission as the inline one it stands for. A failed
    /// relayed acquisition produces an empty ordered receipt; do not substitute original bytes.
    /// See
    /// [`TerminalScreen::set_image_relay_policy`](crate::widgets::TerminalScreen::set_image_relay_policy).
    pub bytes: Vec<u8>,
}

/// Begin synchronized update (`DECSET 2026`).
pub(super) const BEGIN_SYNC: &[u8] = b"\x1b[?2026h";
/// End synchronized update (`DECRST 2026`).
const END_SYNC: &[u8] = b"\x1b[?2026l";
/// The same bound as one retained APC: larger frames stream as their ordinary transactions.
const FRAME_LIMIT: usize = 1024 * 1024;

/// How a replaceable transaction uses the cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CursorUse {
    Untouched,
    /// Places an image at the cursor without moving it.
    Reads,
    /// Sets the cursor to an absolute position before anything reads it.
    Writes,
}

/// Everything a replaceable transaction's effect depends on or changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Footprint {
    image: u32,
    /// A transmission also advances the screen's next automatic id past the one it names.
    transmitted: bool,
    cursor: CursorUse,
}

impl Footprint {
    pub(super) fn transmission(id: u32, places: bool) -> Self {
        Self {
            image: id,
            transmitted: true,
            cursor: if places {
                CursorUse::Reads
            } else {
                CursorUse::Untouched
            },
        }
    }

    pub(super) fn image(&self) -> u32 {
        self.image
    }
}

/// Replaceable transactions since the last ordered one, which is what a consumer may coalesce.
///
/// A run holds one source, and every transaction in it has the same footprint: one image and,
/// possibly, the cursor. Replaying only the newest therefore differs from replaying all of them
/// in that image alone, which the newest wholly determines. The image budget sees the same
/// sizes, and no transaction in a run evicted, so no eviction can appear or disappear.
/// Transactions for different sources are never reordered: they could change which images
/// the budget evicts.
#[derive(Default)]
pub(super) struct ReplacementRun {
    current: Option<(ImageSourceKey, Footprint)>,
}

impl ReplacementRun {
    /// Whether a transaction keyed by `source` keeps the run coalescible. If not, the caller
    /// must emit it as a dependency and clear the run.
    pub(super) fn admit(&mut self, source: ImageSourceKey, footprint: Footprint) -> bool {
        match self.current {
            Some(current) => current == (source, footprint),
            None => {
                self.current = Some((source, footprint));
                true
            }
        }
    }

    pub(super) fn clear(&mut self) {
        self.current = None;
    }
}

/// What the next text bytes inside an open frame turned out to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FrameText {
    /// Every byte was consumed and the frame is still open.
    Open,
    /// The update ended after this many bytes.
    Ended(usize),
    /// This many bytes leave the provable subset; the frame must stream as recorded.
    Unproven(usize),
}

/// A synchronized update being held until its end shows whether it is one replacement.
///
/// The frame qualifies only when its text is cursor positioning and its graphics commands
/// leave every image they touch in a state that does not depend on what came before. Such a
/// frame applied after any earlier frame with the same footprint gives the same result as
/// applying it alone, so the earlier one may be dropped.
pub(super) struct OpenFrame {
    pending: Vec<TerminalOutputTransaction>,
    bytes: usize,
    escape: Vec<u8>,
    cursor: bool,
    /// Image id to (determined by this frame, transmitted by this frame).
    images: BTreeMap<u32, (bool, bool)>,
    proven: bool,
    evictions: u64,
}

impl OpenFrame {
    /// Start a frame whose begin marker has just been read. `evictions` is the store's count.
    pub(super) fn new(evictions: u64) -> Self {
        Self {
            pending: vec![TerminalOutputTransaction {
                class: TerminalOutputClass::Ordered,
                bytes: BEGIN_SYNC.to_vec(),
            }],
            bytes: BEGIN_SYNC.len(),
            escape: Vec::new(),
            cursor: false,
            images: BTreeMap::new(),
            proven: true,
            evictions,
        }
    }

    pub(super) fn proven(&self) -> bool {
        self.proven && self.bytes <= FRAME_LIMIT
    }

    /// Hold one recorded transaction, merging ordered text as the screen's own list does.
    pub(super) fn push(&mut self, bytes: Vec<u8>, class: TerminalOutputClass) {
        self.bytes = self.bytes.saturating_add(bytes.len());
        if class == TerminalOutputClass::Ordered
            && let Some(previous) = self.pending.last_mut()
            && previous.class == TerminalOutputClass::Ordered
        {
            previous.bytes.extend_from_slice(&bytes);
        } else {
            self.pending
                .push(TerminalOutputTransaction { bytes, class });
        }
    }

    /// Consume text up to the end marker or the first byte outside the provable subset.
    /// Consumed bytes are held as ordered text.
    pub(super) fn text(&mut self, bytes: &[u8]) -> FrameText {
        let mut outcome = FrameText::Open;
        for (at, byte) in bytes.iter().enumerate() {
            self.escape.push(*byte);
            match classify_escape(&self.escape) {
                Escape::Partial => continue,
                Escape::CursorPosition => self.cursor = true,
                Escape::EndSync => outcome = FrameText::Ended(at + 1),
                Escape::Other => {
                    self.proven = false;
                    outcome = FrameText::Unproven(at + 1);
                }
            }
            self.escape.clear();
            if outcome != FrameText::Open {
                break;
            }
        }
        let used = match outcome {
            FrameText::Open => bytes.len(),
            FrameText::Ended(used) | FrameText::Unproven(used) => used,
        };
        self.push(bytes[..used].to_vec(), TerminalOutputClass::Ordered);
        outcome
    }

    /// Account for one applied graphics command. `succeeded` is whether it left the state it
    /// claims: a transmission stored a new revision, a removal left no image.
    pub(super) fn graphics(&mut self, op: Option<FrameOp>, succeeded: bool) {
        match op {
            Some(FrameOp::Replace { id, places }) if succeeded && (self.cursor || !places) => {
                self.images.insert(id, (true, true));
            }
            Some(FrameOp::Remove { id }) if succeeded => {
                let transmitted = self.images.get(&id).is_some_and(|(_, sent)| *sent);
                self.images.insert(id, (true, transmitted));
            }
            Some(FrameOp::Unplace { id }) => {
                self.images.entry(id).or_insert((false, false));
            }
            _ => self.proven = false,
        }
    }

    /// The frame's footprint, if the whole update is one provable replacement of one image.
    /// Several images would let a dropped frame change the sizes a later one meets on its way,
    /// and so what the image budget evicts.
    pub(super) fn footprint(&self, evictions: u64) -> Option<Footprint> {
        let mut images = self.images.iter();
        let (Some((image, (true, transmitted))), None) = (images.next(), images.next()) else {
            return None;
        };
        (self.proven() && evictions == self.evictions).then_some(Footprint {
            image: *image,
            transmitted: *transmitted,
            cursor: if self.cursor {
                CursorUse::Writes
            } else {
                CursorUse::Untouched
            },
        })
    }

    /// The held bytes, in order, for emitting the frame as one transaction.
    pub(super) fn into_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.bytes);
        for transaction in self.pending {
            bytes.extend_from_slice(&transaction.bytes);
        }
        bytes
    }

    /// The held transactions exactly as they would have been recorded without the frame.
    pub(super) fn into_transactions(self) -> Vec<TerminalOutputTransaction> {
        self.pending
    }
}

enum Escape {
    Partial,
    CursorPosition,
    EndSync,
    Other,
}

/// Recognise `CSI Pn ; Pn H` and the end marker; anything else is outside the subset.
fn classify_escape(escape: &[u8]) -> Escape {
    if escape == END_SYNC {
        return Escape::EndSync;
    }
    if END_SYNC.starts_with(escape) {
        return Escape::Partial;
    }
    let Some(params) = escape.strip_prefix(b"\x1b[") else {
        return Escape::Other;
    };
    let numeric = |params: &[u8]| {
        params.len() <= 12
            && params.iter().filter(|byte| **byte == b';').count() <= 1
            && params
                .iter()
                .all(|byte| byte.is_ascii_digit() || *byte == b';')
    };
    match params.split_last() {
        Some((b'H', params)) if numeric(params) => Escape::CursorPosition,
        _ if numeric(params) => Escape::Partial,
        _ => Escape::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{GraphicsMediaPolicy, TerminalScreen};
    use base64::Engine as _;
    use base64::engine::general_purpose::STANDARD as BASE64;

    const SEED: &[u8] = b"\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,C=1,q=2;YWJj\x1b\\";
    const FRAME: &[u8] = b"\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\";

    fn screen() -> TerminalScreen {
        let mut screen = TerminalScreen::new(4, 8, 0);
        screen.process_bytes(SEED);
        screen.set_output_transaction_recording(true);
        screen
    }

    fn independent(class: TerminalOutputClass) -> bool {
        matches!(
            class,
            TerminalOutputClass::Graphics {
                dependency: GraphicsDependency::Independent,
                ..
            }
        )
    }

    #[test]
    fn large_ordered_reads_remain_bounded_and_lossless() {
        let mut screen = screen();
        let bytes = vec![0; 1024 * 1024 + 17];
        screen.process_bytes(&bytes);
        let transactions = screen.take_output_transactions();
        assert_eq!(transactions.len(), 2);
        assert!(
            transactions
                .iter()
                .all(|transaction| transaction.bytes.len() <= 1024 * 1024
                    && transaction.class == TerminalOutputClass::Ordered)
        );
        assert_eq!(
            transactions
                .into_iter()
                .flat_map(|transaction| transaction.bytes)
                .collect::<Vec<_>>(),
            bytes
        );
    }

    #[test]
    fn mixed_output_is_lossless_at_every_split_boundary() {
        let wire = [
            FRAME,
            b"text".as_slice(),
            b"\x1b]2;title\x07tail".as_slice(),
        ]
        .concat();
        for split in 0..=wire.len() {
            let mut screen = screen();
            screen.process_bytes(&wire[..split]);
            let mut transactions = screen.take_output_transactions();
            screen.process_bytes(&wire[split..]);
            transactions.extend(screen.take_output_transactions());
            assert_eq!(
                transactions
                    .iter()
                    .flat_map(|tx| tx.bytes.iter().copied())
                    .collect::<Vec<_>>(),
                wire,
                "split {split}"
            );
            assert_eq!(
                transactions
                    .iter()
                    .filter(|tx| independent(tx.class))
                    .count(),
                1,
                "split {split}"
            );
            let frame = transactions
                .iter()
                .find(|tx| independent(tx.class))
                .unwrap();
            assert_eq!(frame.bytes, FRAME);
        }
    }

    #[test]
    fn incomplete_apc_is_not_a_completed_transaction() {
        let mut screen = screen();
        screen.process_bytes(&FRAME[..8]);
        assert!(screen.take_output_transactions().is_empty());
        screen.process_bytes(&FRAME[8..20]);
        assert!(screen.take_output_transactions().is_empty());
        screen.process_bytes(&FRAME[20..]);
        let transactions = screen.take_output_transactions();
        assert_eq!(transactions.len(), 1);
        assert!(independent(transactions[0].class));
        assert_eq!(transactions[0].bytes, FRAME);
    }

    #[test]
    fn queries_chunked_cursor_creation_and_uncertain_commands_stay_ordered() {
        for wire in [
            b"\x1b_Ga=q,i=7,f=24,s=1,v=1;ZGVm\x1b\\".as_slice(),
            b"\x1b_Ga=T,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=0,f=24,s=1,v=1,q=2;ZGVm\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2,m=1;ZG\x1b\\\x1b_Gm=0;Vm\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2,B=1;ZGVm\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2,x=1;ZGVm\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;@@bad@@\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2,t=t;L25vdC1hLXNvdXJjZQ==\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2,t=s;L25vdC1hLXNvdXJjZQ==\x1b\\".as_slice(),
            // Synchronized, but it places at a cursor the frame did not set.
            b"\x1b[?2026h\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\\x1b[?2026l".as_slice(),
        ] {
            let mut screen = screen();
            screen.process_bytes(wire);
            let transactions = screen.take_output_transactions();
            assert!(
                transactions.iter().all(|tx| !independent(tx.class)),
                "{wire:?}"
            );
            assert_eq!(
                transactions
                    .iter()
                    .flat_map(|tx| tx.bytes.iter().copied())
                    .collect::<Vec<_>>(),
                wire
            );
        }
    }

    #[test]
    fn ignored_and_aborted_apcs_preserve_every_original_byte() {
        for wire in [
            b"a\x1b_nongraphics\x1b\\b".as_slice(),
            b"a\x1b_Gmalformed\x07b".as_slice(),
            b"a\x1b_Gcancel\x18b".as_slice(),
            b"a\x1b_Gcancel\x1ab".as_slice(),
            b"a\x1b_Ginterrupted\x1bXb".as_slice(),
            b"a\x1b\x1b_Gquery\x1b\\b".as_slice(),
        ] {
            for split in 0..=wire.len() {
                let mut screen = screen();
                screen.process_bytes(&wire[..split]);
                let mut tx = screen.take_output_transactions();
                screen.process_bytes(&wire[split..]);
                tx.extend(screen.take_output_transactions());
                assert_eq!(
                    tx.iter()
                        .flat_map(|tx| tx.bytes.iter().copied())
                        .collect::<Vec<_>>(),
                    wire,
                    "split {split}: {wire:?}"
                );
            }
        }
    }

    #[test]
    fn external_boundaries_flush_exact_pending_bytes_and_prevent_replacement() {
        for split in [1, 2, 8, FRAME.len() - 2, FRAME.len() - 1] {
            let mut screen = screen();
            screen.process_bytes(&FRAME[..split]);
            assert!(screen.output_transaction_pending());
            screen.flush_output_transaction_boundary();
            let mut transactions = screen.take_output_transactions();
            screen.process_bytes(&FRAME[split..]);
            transactions.extend(screen.take_output_transactions());
            assert!(!screen.output_transaction_pending());
            assert_eq!(
                transactions
                    .iter()
                    .flat_map(|tx| tx.bytes.iter().copied())
                    .collect::<Vec<_>>(),
                FRAME,
                "split {split}"
            );
            assert!(
                transactions.iter().all(|tx| !independent(tx.class)),
                "split {split}"
            );
            screen.process_bytes(FRAME);
            assert!(
                screen
                    .take_output_transactions()
                    .iter()
                    .any(|tx| independent(tx.class))
            );
        }
    }

    #[test]
    fn retransmission_that_removes_placements_is_not_independent() {
        for wire in [
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\".as_slice(),
            FRAME,
        ] {
            let mut screen = screen();
            screen.process_bytes(b"\x1b_Ga=p,i=7,p=10,C=1,q=2;\x1b\\");
            screen.take_output_transactions();
            screen.process_bytes(wire);
            assert!(
                screen
                    .take_output_transactions()
                    .iter()
                    .all(|tx| !independent(tx.class))
            );
        }
    }

    #[test]
    fn replacement_requires_the_same_displayed_placement_state() {
        for (wire, expected) in [
            (
                b"\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\".as_slice(),
                true,
            ),
            (
                b"\x1b_Ga=T,i=7,p=10,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\".as_slice(),
                false,
            ),
            (
                b"\x1b_Ga=T,i=8,p=9,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\".as_slice(),
                false,
            ),
        ] {
            let mut screen = screen();
            screen.process_bytes(wire);
            assert_eq!(
                screen
                    .take_output_transactions()
                    .iter()
                    .any(|tx| independent(tx.class)),
                expected,
                "{wire:?}"
            );
        }
    }

    #[test]
    fn large_unknown_apcs_use_bounded_ordered_fragments() {
        let mut screen = screen();
        let wire = [
            b"\x1b_unknown".as_slice(),
            &vec![b'x'; 2 * 1024 * 1024],
            b"\x1b\\".as_slice(),
        ]
        .concat();
        let mut transactions = Vec::new();
        for chunk in wire.chunks(64 * 1024) {
            screen.process_bytes(chunk);
            transactions.extend(screen.take_output_transactions());
        }
        assert_eq!(
            transactions
                .iter()
                .flat_map(|tx| tx.bytes.iter().copied())
                .collect::<Vec<_>>(),
            wire
        );
        assert!(
            transactions
                .iter()
                .all(|tx| tx.class == TerminalOutputClass::Ordered)
        );
        assert!(transactions.iter().all(|tx| tx.bytes.len() <= 1024 * 1024));
    }

    /// One frame as terminal-browser's whole-frame presenter writes it.
    fn browser_frame(row: u16, payload: &str) -> Vec<u8> {
        format!(
            "\x1b[?2026h\x1b_Ga=d,d=i,i=1,p=1,q=2\x1b\\\x1b[{row};1H\
             \x1b_Ga=T,f=24,s=1,v=1,i=1,p=1,C=1,z=0,q=2;{payload}\x1b\\\x1b[?2026l"
        )
        .into_bytes()
    }

    fn frames(transactions: &[TerminalOutputTransaction]) -> Vec<(bool, &[u8])> {
        transactions
            .iter()
            .map(|tx| (independent(tx.class), tx.bytes.as_slice()))
            .collect()
    }

    #[test]
    fn whole_frame_updates_are_single_replacements() {
        let mut screen = screen();
        let (first, second) = (browser_frame(1, "YWJj"), browser_frame(2, "ZGVm"));
        screen.process_bytes(&[first.as_slice(), &second].concat());
        let transactions = screen.take_output_transactions();
        assert_eq!(
            frames(&transactions),
            [(true, first.as_slice()), (true, second.as_slice())]
        );
        let sources: Vec<_> = transactions
            .iter()
            .map(|tx| match tx.class {
                TerminalOutputClass::Graphics { source, .. } => source,
                TerminalOutputClass::Ordered => unreachable!(),
            })
            .collect();
        assert_eq!(sources[0], sources[1]);
    }

    #[test]
    fn a_frame_is_held_whole_across_every_split() {
        let wire = [browser_frame(1, "YWJj"), browser_frame(2, "ZGVm")].concat();
        for split in 0..=wire.len() {
            let mut screen = screen();
            screen.process_bytes(&wire[..split]);
            let mut transactions = screen.take_output_transactions();
            screen.process_bytes(&wire[split..]);
            transactions.extend(screen.take_output_transactions());
            assert_eq!(transactions.concat_bytes(), wire, "split {split}");
            assert!(!screen.output_transaction_pending(), "split {split}");
            // A begin marker split across reads is not recognised; that frame stays ordered.
            let replacements = transactions
                .iter()
                .filter(|tx| independent(tx.class))
                .count();
            assert!(replacements >= 1, "split {split}");
        }
    }

    #[test]
    fn replacement_frames_give_the_state_of_the_last_one_alone() {
        let frames = [
            browser_frame(1, "YWJj"),
            browser_frame(3, "ZGVm"),
            browser_frame(2, "Z2hp"),
        ];
        let mut all = screen();
        for frame in &frames {
            all.process_bytes(frame);
        }
        let mut last = screen();
        last.process_bytes(&frames[2]);
        let observe = |screen: &mut TerminalScreen| {
            let snapshot = screen.render_snapshot();
            let images: Vec<_> = snapshot
                .images
                .iter()
                .map(|placement| {
                    let geometry = (placement.row, placement.col, placement.rows, placement.cols);
                    (placement.image_id, placement.image.source_hash(), geometry)
                })
                .collect();
            (images, snapshot.cursor_row, snapshot.cursor_col)
        };
        assert_eq!(observe(&mut all), observe(&mut last));
    }

    #[test]
    fn frames_with_unprovable_content_stream_exactly_as_before() {
        for frame in [
            // Text.
            b"\x1b[?2026h\x1b[1;1Hhello\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\\x1b[?2026l"
                .as_slice(),
            // A relative cursor move.
            b"\x1b[?2026h\x1b[2A\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\\x1b[?2026l"
                .as_slice(),
            // Placement deletes that leave the image's earlier state behind.
            b"\x1b[?2026h\x1b_Ga=d,d=i,i=7,p=9,q=2\x1b\\\x1b[?2026l".as_slice(),
            // Placement by position.
            b"\x1b[?2026h\x1b[1;1H\x1b_Ga=d,d=p,x=1,y=1,q=2\x1b\\\x1b[?2026l".as_slice(),
            // A response.
            b"\x1b[?2026h\x1b[1;1H\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,C=1;ZGVm\x1b\\\x1b[?2026l"
                .as_slice(),
            // A placement that moves the cursor.
            b"\x1b[?2026h\x1b[1;1H\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,q=2;ZGVm\x1b\\\x1b[?2026l"
                .as_slice(),
            // Chunked.
            b"\x1b[?2026h\x1b[1;1H\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,C=1,q=2,m=1;ZG\x1b\\\
              \x1b_Gm=0;Vm\x1b\\\x1b[?2026l"
                .as_slice(),
            // Nothing to replace.
            b"\x1b[?2026h\x1b[1;1H\x1b[?2026l".as_slice(),
        ] {
            let mut screen = screen();
            screen.process_bytes(frame);
            let transactions = screen.take_output_transactions();
            assert!(
                transactions.iter().all(|tx| !independent(tx.class)),
                "{frame:?}"
            );
            assert_eq!(transactions.concat_bytes(), frame);
            assert!(!screen.output_transaction_pending());
        }
    }

    #[test]
    fn an_unproven_frame_is_released_without_waiting_for_its_end() {
        let mut screen = screen();
        screen.process_bytes(b"\x1b[?2026h\x1b[1;1Hpartial");
        assert_eq!(
            screen.take_output_transactions().concat_bytes(),
            b"\x1b[?2026h\x1b[1;1Hpartial"
        );
        assert!(!screen.output_transaction_pending());
    }

    #[test]
    fn an_open_frame_is_pending_until_its_end_or_a_boundary() {
        let frame = browser_frame(1, "YWJj");
        let open = frame.len() - 3;
        let mut screen = screen();
        screen.process_bytes(&frame[..open]);
        assert!(screen.take_output_transactions().is_empty());
        assert!(screen.output_transaction_pending());
        screen.flush_output_transaction_boundary();
        assert!(!screen.output_transaction_pending());
        let mut transactions = screen.take_output_transactions();
        screen.process_bytes(&frame[open..]);
        transactions.extend(screen.take_output_transactions());
        assert_eq!(transactions.concat_bytes(), frame);
        assert!(transactions.iter().all(|tx| !independent(tx.class)));
    }

    #[test]
    fn a_begin_marker_inside_an_open_update_does_not_start_a_frame() {
        let mut screen = screen();
        let wire = [b"\x1b[?2026htext".as_slice(), &browser_frame(1, "YWJj")].concat();
        screen.process_bytes(&wire);
        let transactions = screen.take_output_transactions();
        assert_eq!(transactions.concat_bytes(), wire);
        assert!(transactions.iter().all(|tx| !independent(tx.class)));
    }

    #[test]
    fn replacements_sharing_state_under_different_sources_end_the_run() {
        // The frame owns image 1 and writes the cursor. A lone placement of image 7 reads that
        // cursor, so it must not be moved across a later frame: it becomes a dependency.
        let mut screen = screen();
        let frame = browser_frame(1, "YWJj");
        screen.process_bytes(&frame);
        screen.process_bytes(FRAME);
        screen.process_bytes(&frame);
        let transactions = screen.take_output_transactions();
        assert_eq!(
            transactions
                .iter()
                .map(|tx| independent(tx.class))
                .collect::<Vec<_>>(),
            [true, false, true]
        );

        // Frames writing the cursor under different sources cannot share a run either.
        let mut screen = super::tests::screen();
        let other = String::from_utf8(browser_frame(1, "YWJj"))
            .unwrap()
            .replace("i=1", "i=2")
            .into_bytes();
        screen.process_bytes(&browser_frame(1, "YWJj"));
        screen.process_bytes(&other);
        let transactions = screen.take_output_transactions();
        assert_eq!(
            transactions
                .iter()
                .map(|tx| independent(tx.class))
                .collect::<Vec<_>>(),
            [true, false]
        );
    }

    #[test]
    fn a_run_holds_one_source() {
        // Reordering replacements of different images could change what the budget evicts, so
        // switching sources ends the run and the switching transaction becomes a dependency.
        let mut screen = screen();
        screen.process_bytes(b"\x1b_Ga=T,i=8,p=1,f=24,s=1,v=1,C=1,q=2;YWJj\x1b\\");
        screen.take_output_transactions();
        let other = b"\x1b_Ga=T,i=8,p=1,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\";
        for wire in [FRAME, FRAME, other, other, FRAME] {
            screen.process_bytes(wire);
        }
        assert_eq!(
            screen
                .take_output_transactions()
                .iter()
                .map(|tx| independent(tx.class))
                .collect::<Vec<_>>(),
            [true, true, false, true, false]
        );
    }

    /// What a consumer may do under the documented contract: in each run of replaceable
    /// transactions, keep the newest per source at the position of the first.
    fn coalesce(transactions: Vec<TerminalOutputTransaction>) -> Vec<u8> {
        let mut out: Vec<TerminalOutputTransaction> = Vec::new();
        let mut run: std::collections::HashMap<ImageSourceKey, usize> = Default::default();
        for tx in transactions {
            match tx.class {
                TerminalOutputClass::Graphics {
                    source,
                    dependency: GraphicsDependency::Independent,
                    ..
                } => match run.get(&source) {
                    Some(&at) => out[at] = tx,
                    None => {
                        run.insert(source, out.len());
                        out.push(tx);
                    }
                },
                _ => {
                    run.clear();
                    out.push(tx);
                }
            }
        }
        out.concat_bytes()
    }

    #[test]
    fn coalescing_by_the_contract_preserves_the_final_screen() {
        let pieces: [Vec<u8>; 9] = [
            browser_frame(1, "YWJj"),
            // The frame's own image and placement, replaced alone at the cursor the frame left.
            b"\x1b_Ga=T,i=1,p=1,f=24,s=1,v=1,C=1,z=0,q=2;c3R1\x1b\\".to_vec(),
            browser_frame(3, "ZGVm"),
            String::from_utf8(browser_frame(2, "Z2hp"))
                .unwrap()
                .replace("i=1", "i=2")
                .into_bytes(),
            FRAME.to_vec(),
            b"\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,C=1,q=2;amts\x1b\\".to_vec(),
            b"\x1b_Ga=t,i=8,f=24,s=1,v=1,q=2;bW5v\x1b\\".to_vec(),
            b"\x1b[4;5H".to_vec(),
            b"\x1b[?2026h\x1b[2;2H\x1b_Ga=d,d=I,i=2,q=2\x1b\\\x1b_Ga=T,i=2,p=3,f=24,s=1,v=1,C=1,\
              q=2;cHFy\x1b\\\x1b[?2026l"
                .to_vec(),
        ];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut coalesced = 0;
        for _ in 0..200 {
            let mut wire = Vec::new();
            for _ in 0..12 {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                wire.extend_from_slice(&pieces[(seed % pieces.len() as u64) as usize]);
            }
            let mut recorded = screen();
            recorded.process_bytes(&wire);
            let transactions = recorded.take_output_transactions();
            assert_eq!(transactions.concat_bytes(), wire);
            let kept = coalesce(transactions);
            coalesced += usize::from(kept.len() < wire.len());

            let observe = |bytes: &[u8]| {
                let mut screen = TerminalScreen::new(4, 8, 0);
                screen.process_bytes(SEED);
                screen.process_bytes(bytes);
                let snapshot = screen.render_snapshot();
                let mut images: Vec<_> = snapshot
                    .images
                    .iter()
                    .map(|p| {
                        let geometry = (p.row, p.col, p.rows, p.cols, p.z);
                        (p.image_id, p.image.source_hash(), geometry)
                    })
                    .collect();
                images.sort();
                (images, snapshot.cursor_row, snapshot.cursor_col)
            };
            assert_eq!(observe(&kept), observe(&wire), "{wire:?}");
        }
        assert!(
            coalesced > 50,
            "only {coalesced} streams coalesced anything"
        );
    }

    /// A stored, unplaced image of `units` × 12 bytes (three RGBA pixels per unit).
    fn stored(id: u32, units: usize) -> Vec<u8> {
        format!(
            "\x1b_Ga=t,i={id},f=32,s={},v=1,q=2;{}\x1b\\",
            units * 3,
            "AAAA".repeat(units * 4)
        )
        .into_bytes()
    }

    /// Images left after `seed` then `stream`, with a budget of 100 units.
    fn surviving(seed: &[u8], stream: &[u8]) -> Vec<u32> {
        let mut screen = TerminalScreen::new(4, 8, 0);
        screen.set_image_budget(1200);
        screen.process_bytes(seed);
        screen.process_bytes(stream);
        (1..=9).filter(|id| screen.has_stored_image(*id)).collect()
    }

    fn assert_coalescing_keeps_budget_effects(seed: &[u8], stream: &[u8]) {
        let mut recorder = TerminalScreen::new(4, 8, 0);
        recorder.set_image_budget(1200);
        recorder.process_bytes(seed);
        recorder.set_output_transaction_recording(true);
        recorder.process_bytes(stream);
        let kept = coalesce(recorder.take_output_transactions());
        assert_eq!(surviving(seed, &kept), surviving(seed, stream));
    }

    #[test]
    fn coalescing_across_sources_cannot_cause_an_eviction() {
        // C=20, B=60, A=10, then A→10, B→10, A→60 never exceeds 100. Moving the last A to the
        // first one's position would reach 140 and evict C, which nothing recreates.
        let seed = [stored(3, 20), stored(2, 60), stored(1, 10)].concat();
        let stream = [stored(1, 10), stored(2, 10), stored(1, 60)].concat();
        assert_eq!(surviving(&seed, &stream), [1, 2, 3]);
        assert_coalescing_keeps_budget_effects(&seed, &stream);
    }

    #[test]
    fn a_replacement_that_evicted_is_not_dropped() {
        // X→90 evicts C; dropping it in favour of X→10 would leave C stored.
        let seed = [stored(3, 20), stored(1, 10)].concat();
        let stream = [stored(1, 90), stored(1, 10)].concat();
        assert_eq!(surviving(&seed, &stream), [1]);
        assert_coalescing_keeps_budget_effects(&seed, &stream);
    }

    #[test]
    fn a_frame_owning_several_images_is_not_a_replacement() {
        // An earlier frame can shrink image 2 so that a later one fits only in its original
        // position; dropping the earlier frame would let the later one evict.
        let mut screen = screen();
        screen.process_bytes(
            b"\x1b[?2026h\x1b_Ga=t,i=1,f=24,s=1,v=1,q=2;YWJj\x1b\\\
              \x1b_Ga=t,i=2,f=24,s=1,v=1,q=2;ZGVm\x1b\\\x1b[?2026l",
        );
        assert!(
            screen
                .take_output_transactions()
                .iter()
                .all(|tx| !independent(tx.class))
        );
    }

    #[test]
    fn coalescing_under_budget_pressure_keeps_the_same_images() {
        let seed = [stored(3, 20), stored(2, 30), stored(1, 10)].concat();
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        let mut coalesced = 0;
        for _ in 0..300 {
            let mut stream = Vec::new();
            for _ in 0..8 {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                let id = 1 + (state % 3) as u32;
                let units = [5, 10, 30, 60, 90][((state >> 8) % 5) as usize];
                stream.extend_from_slice(&stored(id, units));
            }
            let mut recorder = TerminalScreen::new(4, 8, 0);
            recorder.set_image_budget(1200);
            recorder.process_bytes(&seed);
            recorder.set_output_transaction_recording(true);
            recorder.process_bytes(&stream);
            let kept = coalesce(recorder.take_output_transactions());
            coalesced += usize::from(kept.len() < stream.len());
            assert_eq!(
                surviving(&seed, &kept),
                surviving(&seed, &stream),
                "{stream:?}"
            );
        }
        assert!(
            coalesced > 50,
            "only {coalesced} streams coalesced anything"
        );
    }

    /// A server-side screen that reads out-of-band sources and relays them inline.
    fn relaying_recorder() -> TerminalScreen {
        let mut recorder = TerminalScreen::new(4, 8, 0);
        recorder.set_image_media_policy(GraphicsMediaPolicy::default());
        recorder.set_image_relay_policy(crate::GraphicsRelayPolicy::All);
        recorder.set_output_transaction_recording(true);
        recorder
    }

    /// The pixels a parser that refuses every out-of-band medium stores from `bytes`.
    fn received_pixels(bytes: &[u8]) -> Vec<u8> {
        let mut receiver = TerminalScreen::new(4, 8, 0);
        receiver.set_image_media_policy(GraphicsMediaPolicy::NONE);
        receiver.process_bytes(bytes);
        let snapshot = receiver.render_snapshot();
        let image = snapshot.images.first().expect("receiver stored no image");
        image
            .image
            .pixels()
            .expect("receiver could not decode")
            .to_rgb8()
            .into_raw()
    }

    fn scratch_file(name: &str, contents: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "tty-graphics-protocol-relay-{}-{name}",
            std::process::id()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    fn named(path: &std::path::Path) -> String {
        BASE64.encode(path.as_os_str().as_encoded_bytes())
    }

    #[test]
    fn a_relayed_file_frame_reaches_a_receiver_that_cannot_read_files() {
        let pixels: Vec<u8> = (0u8..12).collect();
        let path = scratch_file("file", &pixels);
        let mut recorder = relaying_recorder();
        recorder.process_bytes(
            format!(
                "\x1b_Ga=T,f=24,s=2,v=2,t=f,i=5,p=1,C=1,q=2;{}\x1b\\",
                named(&path)
            )
            .as_bytes(),
        );
        let relayed = recorder.take_output_transactions().concat_bytes();
        let text = String::from_utf8_lossy(&relayed);
        assert!(
            !text.contains("t=f"),
            "the source's name was forwarded: {text}"
        );
        assert!(text.contains("a=T") && text.contains("i=5") && text.contains("p=1"));
        assert!(text.contains("o=z"), "raw pixels travel compressed: {text}");
        assert_eq!(received_pixels(&relayed), pixels);
        // The file is left for its owner, as `t=f` requires.
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_metadata_only_screen_still_relays_a_file_frame() {
        let pixels: Vec<u8> = (50u8..62).collect();
        let path = scratch_file("metadata", &pixels);
        let mut recorder = relaying_recorder();
        recorder.set_image_storage_enabled(false);
        recorder.process_bytes(
            format!("\x1b_Ga=T,f=24,s=2,v=2,t=f,i=7,q=2;{}\x1b\\", named(&path)).as_bytes(),
        );
        let relayed = recorder.take_output_transactions().concat_bytes();
        assert!(!String::from_utf8_lossy(&relayed).contains("t=f"));
        assert_eq!(received_pixels(&relayed), pixels);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_consumed_temp_file_is_read_once_and_still_reaches_the_receiver() {
        let pixels: Vec<u8> = (100u8..112).collect();
        let path = scratch_file("temp", &pixels);
        let mut recorder = relaying_recorder();
        recorder.process_bytes(
            format!("\x1b_Ga=T,f=24,s=2,v=2,t=t,i=6,q=2;{}\x1b\\", named(&path)).as_bytes(),
        );
        assert!(
            !path.exists(),
            "the server claims a temporary file by reading it"
        );
        let relayed = recorder.take_output_transactions().concat_bytes();
        assert!(!String::from_utf8_lossy(&relayed).contains("t=t"));
        assert_eq!(received_pixels(&relayed), pixels);
    }

    #[test]
    fn a_large_relayed_frame_is_split_into_protocol_chunks() {
        // Noise does not compress, so the inline payload spans many 4096-byte chunks.
        let mut state = 0x2545_f491_4f6c_dd1du64;
        let pixels: Vec<u8> = (0..64 * 64 * 3)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        let path = scratch_file("large", &pixels);
        let mut recorder = relaying_recorder();
        recorder.process_bytes(
            format!(
                "\x1b_Ga=T,f=24,s=64,v=64,t=f,i=7,q=2;{}\x1b\\",
                named(&path)
            )
            .as_bytes(),
        );
        let relayed = recorder.take_output_transactions().concat_bytes();
        let text = String::from_utf8_lossy(&relayed);
        assert!(text.matches("m=1").count() > 2, "expected several chunks");
        assert_eq!(text.matches("m=0").count(), 1);
        assert_eq!(received_pixels(&relayed), pixels);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn without_relaying_the_original_escape_is_forwarded() {
        let path = scratch_file("plain", &(0u8..12).collect::<Vec<_>>());
        let wire = format!("\x1b_Ga=T,f=24,s=2,v=2,t=f,i=8,q=2;{}\x1b\\", named(&path));
        let mut recorder = TerminalScreen::new(4, 8, 0);
        recorder.set_image_media_policy(GraphicsMediaPolicy::default());
        recorder.set_output_transaction_recording(true);
        recorder.process_bytes(wire.as_bytes());
        assert_eq!(
            recorder.take_output_transactions().concat_bytes(),
            wire.as_bytes()
        );
        let _ = std::fs::remove_file(path);
    }

    trait ConcatBytes {
        fn concat_bytes(&self) -> Vec<u8>;
    }

    impl ConcatBytes for Vec<TerminalOutputTransaction> {
        fn concat_bytes(&self) -> Vec<u8> {
            self.iter()
                .flat_map(|tx| tx.bytes.iter().copied())
                .collect()
        }
    }
}
