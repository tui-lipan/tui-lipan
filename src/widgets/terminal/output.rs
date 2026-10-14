//! Lossless output transactions classified by the retained parser.
use super::{ImageSourceKey, ImageSourceRevision};

/// Whether a completed graphics operation can supersede an earlier one independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsDependency {
    /// This transaction completely replaces its logical source without changing its placements.
    Independent,
    /// Its effects require preceding output; preserve byte order and every transaction.
    DependsOnPrevious,
}

/// Semantic classification of original producer bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalOutputClass {
    /// Text, controls, queries, incomplete/chunked or uncertain operations.
    Ordered,
    /// A successfully completed source mutation with explicit dependency information.
    Graphics {
        /// Identity in the authoritative retained screen.
        source: ImageSourceKey,
        /// Source revision after the operation.
        revision: ImageSourceRevision,
        /// Only independent transactions may replace pending earlier transactions.
        dependency: GraphicsDependency,
    },
}

/// Exact original bytes of one ordered segment or completed graphics command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalOutputTransaction {
    /// Parser-established semantics; defaults to ordered when equivalence is uncertain.
    pub class: TerminalOutputClass,
    /// Original bytes, unchanged and in producer order.
    pub bytes: Vec<u8>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::TerminalScreen;

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
            b"\x1b[?2026h\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\\x1b[?2026l".as_slice(),
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
}
