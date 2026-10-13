//! Which viewport rows a terminal changed since the last frame.
//!
//! The emulator already tracks this - `alacritty_terminal` reports damaged line ranges precisely so
//! a front end can avoid redrawing what did not move. Rendering discarded that and rebuilt the
//! whole visible grid for any change, so a one-character spinner cost a full-window frame.
//!
//! Damage accumulates here between paints, because several writes can arrive before the runner
//! draws, and is taken once per frame.

/// The horizontal extent of one damaged viewport row, in columns, inclusive on both ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DamagedRow {
    /// Viewport row, 0 at the top of the visible area.
    pub row: u16,
    /// Leftmost damaged column.
    pub left: u16,
    /// Rightmost damaged column, inclusive.
    pub right: u16,
}

/// What changed in a terminal's viewport since the last time damage was taken.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum TerminalDamage {
    /// Nothing changed.
    #[default]
    None,
    /// Everything must be treated as changed: a resize, a screen swap, a scroll, or simply more
    /// rows damaged than tracking them individually is worth.
    Full,
    /// These rows changed, ascending by row and non-overlapping.
    Rows(Vec<DamagedRow>),
}

/// Accumulates damage between paints.
///
/// Rows are kept in a slot per viewport row rather than a list, so merging repeated writes to the
/// same row is a compare rather than a search, and taking the set is one pass.
#[derive(Clone, Debug, Default)]
pub(crate) struct DamageAccumulator {
    full: bool,
    /// `Some((left, right))` for a damaged row, indexed by viewport row.
    rows: Vec<Option<(u16, u16)>>,
}

impl DamageAccumulator {
    /// Forget everything and treat the next frame as fully damaged.
    pub(crate) fn mark_full(&mut self) {
        self.full = true;
        self.rows.clear();
    }

    /// Widen this row's damaged span, or record it if the row was clean.
    pub(crate) fn add_row(&mut self, row: usize, left: usize, right: usize, viewport_rows: usize) {
        if self.full || row >= viewport_rows {
            return;
        }
        if self.rows.len() < viewport_rows {
            self.rows.resize(viewport_rows, None);
        }
        let left = left.min(u16::MAX as usize) as u16;
        let right = right.min(u16::MAX as usize) as u16;
        let slot = &mut self.rows[row];
        *slot = Some(match *slot {
            Some((old_left, old_right)) => (old_left.min(left), old_right.max(right)),
            None => (left, right),
        });
    }

    /// Take the accumulated damage, leaving the terminal clean.
    pub(crate) fn take(&mut self) -> TerminalDamage {
        if self.full {
            self.full = false;
            self.rows.clear();
            return TerminalDamage::Full;
        }
        let rows: Vec<DamagedRow> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(row, span)| {
                span.map(|(left, right)| DamagedRow {
                    row: row as u16,
                    left,
                    right,
                })
            })
            .collect();
        self.rows.clear();
        if rows.is_empty() {
            TerminalDamage::None
        } else {
            TerminalDamage::Rows(rows)
        }
    }
}

impl TerminalDamage {
    /// Fold `other` into `self`, keeping the more conservative of the two.
    pub fn merge(&mut self, other: TerminalDamage) {
        match (&mut *self, other) {
            (TerminalDamage::Full, _) | (_, TerminalDamage::None) => {}
            (slot, TerminalDamage::Full) => *slot = TerminalDamage::Full,
            (TerminalDamage::None, other) => *self = other,
            (TerminalDamage::Rows(existing), TerminalDamage::Rows(incoming)) => {
                for row in incoming {
                    match existing.binary_search_by_key(&row.row, |entry| entry.row) {
                        Ok(index) => {
                            existing[index].left = existing[index].left.min(row.left);
                            existing[index].right = existing[index].right.max(row.right);
                        }
                        Err(index) => existing.insert(index, row),
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::widgets::TerminalScreen;

    #[test]
    fn a_spinner_rewrite_damages_one_row() {
        let mut screen = TerminalScreen::new(24, 80, 1000);
        screen.process_bytes(b"hello\r\n");
        let _ = screen.take_damage();

        screen.process_bytes(b"\r| working");
        match screen.take_damage() {
            super::TerminalDamage::Rows(rows) => {
                assert_eq!(rows.len(), 1, "one row moved, got {rows:?}");
                assert!(
                    rows[0].right - rows[0].left < 20,
                    "a nine-column write should not damage the whole row: {rows:?}"
                );
            }
            other => panic!("expected partial damage, got {other:?}"),
        }
    }

    #[test]
    fn taking_damage_leaves_the_screen_clean() {
        let mut screen = TerminalScreen::new(24, 80, 1000);
        screen.process_bytes(b"x");
        let _ = screen.take_damage();
        std::assert_matches!(screen.take_damage(), super::TerminalDamage::None);
    }
}

#[cfg(test)]
mod cursor_damage_tests {
    use crate::widgets::TerminalScreen;

    /// A cursor move with no text change must still report damage.
    ///
    /// The caret occupies cells, so moving it changes what the screen looks like even though no
    /// character did. If the emulator reported nothing here, a frame carrying only a cursor move
    /// would be rejected as `NothingMoved` and the caret would go stale.
    #[test]
    fn moving_the_cursor_alone_reports_damage() {
        let mut screen = TerminalScreen::new(24, 80, 1000);
        screen.process_bytes(b"hello world");
        let _ = screen.take_damage();

        // Cursor to row 5, column 10. No cell contents change.
        screen.process_bytes(b"\x1b[5;10H");
        let damage = screen.take_damage();
        assert!(
            !matches!(damage, super::TerminalDamage::None),
            "a cursor move reported no damage: {damage:?}"
        );
    }
}

/// Completed changes to the retained graphics scene.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum GraphicsDamage {
    /// No retained graphics changed.
    #[default]
    None,
    /// Image pixels or source definitions changed.
    Sources,
    /// Placement geometry or visibility changed.
    Placements,
    /// Both sources and placements changed.
    SourcesAndPlacements,
    /// The graphics namespace was reset.
    Reset,
}

impl GraphicsDamage {
    /// Accumulate changes without losing a reset or either kind of damage.
    pub fn merge(&mut self, other: Self) {
        *self = match (*self, other) {
            (Self::Reset, _) | (_, Self::Reset) => Self::Reset,
            (Self::None, damage) | (damage, Self::None) => damage,
            (a, b) if a == b => a,
            _ => Self::SourcesAndPlacements,
        };
    }
}

/// Mutations completed by the terminal parser, independent of snapshot consumption.
///
/// Incomplete escape sequences and graphics queries do not report visual damage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalMutation {
    /// Changed viewport cells, including the emulator's cursor damage.
    pub cells: TerminalDamage,
    /// Changes to retained image sources and placements.
    pub graphics: GraphicsDamage,
    /// Cursor position, visibility, or style changed.
    pub cursor_changed: bool,
    /// Window title changed.
    pub title_changed: bool,
    /// Terminal modes changed.
    pub modes_changed: bool,
    /// Semantic metadata or program status changed.
    pub semantic_changed: bool,
    /// Clipboard requests were produced.
    pub clipboard_events: bool,
    /// Protocol responses were produced.
    pub responses: bool,
}

impl TerminalMutation {
    /// Whether a completed operation changed anything or produced an event.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// Whether a completed operation requires visual presentation.
    pub fn is_visual(&self) -> bool {
        self.cells != TerminalDamage::None
            || self.graphics != GraphicsDamage::None
            || self.cursor_changed
    }

    /// Merge parser calls made before a consumer takes pending mutations.
    pub fn merge(&mut self, other: Self) {
        self.cells.merge(other.cells);
        self.graphics.merge(other.graphics);
        self.cursor_changed |= other.cursor_changed;
        self.title_changed |= other.title_changed;
        self.modes_changed |= other.modes_changed;
        self.semantic_changed |= other.semantic_changed;
        self.clipboard_events |= other.clipboard_events;
        self.responses |= other.responses;
    }
}

#[cfg(all(test, feature = "terminal-images"))]
mod mutation_tests {
    use super::{GraphicsDamage, TerminalDamage};
    use crate::widgets::TerminalScreen;

    fn clean_screen() -> TerminalScreen {
        let mut screen = TerminalScreen::new(4, 8, 0);
        screen.process_bytes(b"");
        screen.take_damage();
        screen.take_mutation();
        screen
    }

    #[test]
    fn split_upload_mutates_only_when_completed() {
        let mut screen = clean_screen();
        let generation = screen.mutation_generation();
        assert!(
            screen
                .process_bytes(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;YWJj")
                .is_empty()
        );
        assert_eq!(screen.mutation_generation(), generation);
        let mutation = screen.process_bytes(b"\x1b\\");
        assert_eq!(mutation.graphics, GraphicsDamage::Sources);
        assert_eq!(mutation.cells, TerminalDamage::None);
        assert!(!mutation.cursor_changed);
        assert_eq!(screen.take_mutation(), mutation);
        assert!(screen.take_mutation().is_empty());
    }

    #[test]
    fn query_and_malformed_upload_only_produce_responses() {
        let mut screen = clean_screen();
        for command in [
            b"\x1b_Ga=q,i=7,f=24,s=1,v=1;YWJj\x1b\\".as_slice(),
            b"\x1b_Ga=t,i=7,f=24,s=1,v=1;!\x1b\\".as_slice(),
        ] {
            let mutation = screen.process_bytes(command);
            assert!(!mutation.is_visual(), "{mutation:?}");
            assert!(mutation.responses);
        }
    }

    #[test]
    fn metadata_and_cells_accumulate_independently() {
        let mut screen = clean_screen();
        let title = screen.process_bytes(b"\x1b]2;new title\x07");
        assert!(title.title_changed);
        assert!(!title.is_visual(), "{title:?}");
        let upload = screen.process_bytes(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;YWJj\x1b\\");
        assert_eq!(upload.graphics, GraphicsDamage::Sources);
        screen.process_bytes(b"x");
        let pending = screen.take_mutation();
        assert!(pending.title_changed);
        assert_eq!(pending.graphics, GraphicsDamage::Sources);
        assert_ne!(pending.cells, TerminalDamage::None);
    }

    #[test]
    fn source_replacement_preserves_identity_and_retained_placement() {
        let mut screen = clean_screen();
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,q=2,C=1;YWJj\x1b\\");
        let before = screen.render_snapshot().images[0].image.clone();
        screen.process_bytes(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\");
        let after = screen.render_snapshot().images[0].image.clone();
        assert_eq!(before.source_key(), after.source_key());
        assert_eq!(after.source_revision().0, before.source_revision().0 + 1);
        screen.process_bytes(b"\x1b_Ga=d,d=I,i=7,q=2;\x1b\\");
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,q=2,C=1;YWJj\x1b\\");
        let recreated = screen.render_snapshot().images[0].image.clone();
        assert_ne!(before.source_key(), recreated.source_key());
    }
}
