//! Stable ordinary placements for composed image rows and tiles.
use super::*;

type SpanKey = (u32, (i32, i32), (u16, u16, u16));
struct SpanId {
    id: u32,
    upload: std::sync::Weak<KittyUpload>,
    used: bool,
}

thread_local! {
    static SPAN_IDS: RefCell<HashMap<SpanKey, SpanId>> = RefCell::default();
}

pub(super) fn begin_frame() {
    // Keep identities across adjacent paints without accumulating every past scroll/clip position.
    SPAN_IDS.with(|slot| {
        slot.borrow_mut().retain(|_, entry| {
            let keep = entry.used && entry.upload.strong_count() != 0;
            entry.used = false;
            keep
        });
    });
}

pub(super) fn span_id(image: &CompressedKitty, origin: (i32, i32), span: (u16, u16, u16)) -> u32 {
    SPAN_IDS.with(|slot| {
        let mut ids = slot.borrow_mut();
        let entry = ids
            .entry((image.id, origin, span))
            .or_insert_with(|| SpanId {
                id: NEXT_KITTY_PLACEMENT.with(|next| {
                    let id = next.get().wrapping_add(1).max(1);
                    next.set(id);
                    id
                }),
                upload: Arc::downgrade(&image.upload),
                used: true,
            });
        entry.used = true;
        entry.upload = Arc::downgrade(&image.upload);
        entry.id
    })
}

type Placements = HashMap<String, KittyPlacement>;

/// Only a successful, undiscarded flush confirms a placement. Keep attempted writes too, since a
/// failed frame may already have reached the host and needs cleanup if the desired scene changes.
#[derive(Default)]
pub(crate) struct HostComposedScene {
    confirmed: Placements,
    attempted: Placements,
    pending: Option<(Placements, usize, bool)>,
}

impl HostComposedScene {
    pub(crate) fn prepare(
        &mut self,
        draw: &mut PreparedKittyDraw<'_>,
        epoch: usize,
        known: &HostKittyUploads,
    ) -> Vec<String> {
        let active = self
            .pending
            .as_ref()
            .filter(|(_, e, ready)| *e == epoch && *ready)
            .map_or(&self.confirmed, |(placements, _, _)| placements);
        let next = surviving_placements(draw, active);
        suppress_unchanged(draw, active, known);
        let deletes = self
            .confirmed
            .iter()
            .chain(&self.attempted)
            .filter(|(key, _)| !next.contains_key(*key))
            .map(|(key, _)| key.clone())
            .collect::<HashSet<_>>();
        self.attempted.extend(next.clone());
        self.pending = Some((next, epoch, false));
        deletes.into_iter().collect()
    }

    pub(crate) fn drawn(&mut self) {
        if let Some((_, _, ready)) = &mut self.pending {
            *ready = true;
        }
    }

    pub(crate) fn invalidate(&mut self) {
        self.attempted.extend(self.confirmed.drain());
        self.pending = None;
    }

    pub(crate) fn flushed(&mut self, success: bool, epoch: usize) {
        let Some((next, written_epoch, ready)) = self.pending.take() else {
            return;
        };
        if success && ready && epoch == written_epoch {
            self.confirmed = next;
            self.attempted.clear();
        }
    }
}

fn surviving_placements(draw: &PreparedKittyDraw<'_>, active: &Placements) -> Placements {
    let outgoing: HashMap<_, _> = draw
        .cells
        .iter()
        .map(|(x, y, cell)| ((*x, *y), cell.symbol()))
        .collect();
    PENDING_KITTY_PLACEMENTS.with(|slot| {
        slot.borrow()
            .values()
            .flatten()
            .filter(|placement| match outgoing.get(&placement.anchor) {
                Some(symbol) => symbol.contains(&placement.command),
                None => active.get(&placement.delete).is_some_and(|old| {
                    old.anchor == placement.anchor && old.command == placement.command
                }),
            })
            .map(|placement| (placement.delete.clone(), placement.clone()))
            .collect()
    })
}

fn suppress_unchanged(
    draw: &mut PreparedKittyDraw<'_>,
    active: &Placements,
    known: &HostKittyUploads,
) {
    for placement in &draw.placements {
        if (known.contains(&placement.upload)
            || placement
                .upload
                .transmit
                .lock()
                .is_ok_and(|upload| upload.is_none()))
            && active.get(&placement.delete).is_some_and(|old| {
                old.anchor == placement.anchor && old.command == placement.command
            })
            && let Some((_, _, cell)) = draw
                .cells
                .iter_mut()
                .find(|(x, y, _)| (*x, *y) == placement.anchor)
        {
            let symbol = cell.symbol().replace(&placement.command, "");
            cell.to_mut().set_symbol(&symbol);
        }
    }
}
