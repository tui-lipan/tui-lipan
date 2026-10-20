//! Stable ordinary placements for composed image rows and tiles.
use super::*;

type SpanKey = (u32, (i32, i32), (u16, u16, u16), (u32, u32, u32, u32));
struct SpanId {
    id: u32,
    upload: std::sync::Weak<KittyUpload>,
    used: bool,
}

#[derive(Default)]
struct PaintExtent {
    partial: bool,
    regions: Vec<ratatui::layout::Rect>,
}

thread_local! {
    static SPAN_IDS: RefCell<HashMap<SpanKey, SpanId>> = RefCell::default();
    static PAINT: RefCell<PaintExtent> = RefCell::default();
}

pub(super) fn begin_frame() {
    PAINT.with(|slot| *slot.borrow_mut() = PaintExtent::default());
    // Full/partial extent is established after begin_frame. Prune unused identities only once
    // a complete paint has staged every visible row; partial paints leave other rows untouched.
    SPAN_IDS.with(|slot| {
        slot.borrow_mut().retain(|_, entry| {
            entry.used = false;
            entry.upload.strong_count() != 0
        });
    });
}

pub(super) fn begin_partial_frame(regions: &[Rect]) {
    PAINT.with(|slot| {
        *slot.borrow_mut() = PaintExtent {
            partial: true,
            regions: regions.iter().copied().map(to_ratatui_rect).collect(),
        };
    });
}

/// Two crops of one image can share an anchor, so the crop is part of the identity too.
fn span_id(
    upload: &Arc<KittyUpload>,
    image: u32,
    origin: (i32, i32),
    span: (u16, u16, u16),
    crop: crate::widgets::TerminalImageCrop,
) -> u32 {
    SPAN_IDS.with(|slot| {
        let mut ids = slot.borrow_mut();
        let crop = (crop.x, crop.y, crop.width, crop.height);
        let entry = ids
            .entry((image, origin, span, crop))
            .or_insert_with(|| SpanId {
                id: NEXT_KITTY_PLACEMENT.with(|next| {
                    let id = next.get().wrapping_add(1).max(1);
                    next.set(id);
                    id
                }),
                upload: Arc::downgrade(upload),
                used: true,
            });
        entry.used = true;
        entry.upload = Arc::downgrade(upload);
        entry.id
    })
}

/// Preserve the source-to-cell mapping when a partial horizontal paint splits an old row.
#[derive(Clone, Copy)]
pub(super) struct RowGeometry {
    pub image: u32,
    pub origin: (i32, i32),
    /// The source pixels shown, and the cells they are stretched over from `origin`.
    pub crop: crate::widgets::TerminalImageCrop,
    pub cells: (u16, u16),
    pub z: i32,
    pub span: (u16, u16, u16),
}

impl RowGeometry {
    pub(super) fn placement(self, upload: Arc<KittyUpload>) -> KittyPlacement {
        let (left, right, y) = self.span;
        let p = span_id(&upload, self.image, self.origin, self.span, self.crop);
        let (crop, cols, rows) = (
            self.crop,
            u32::from(self.cells.0.max(1)),
            u32::from(self.cells.1.max(1)),
        );
        let x = (i32::from(left) - self.origin.0) as u32;
        let row = (i32::from(y) - self.origin.1) as u32;
        // Kitty reads a zero width or height as the whole image, so a row keeps at least a pixel.
        let sx = crop.x + x * crop.width / cols;
        let sy = crop.y + row * crop.height / rows;
        let ex = (crop.x + (x + u32::from(right - left)) * crop.width / cols).max(sx + 1);
        let ey = (crop.y + (row + 1) * crop.height / rows).max(sy + 1);
        KittyPlacement {
            geometry: Some(self),
            anchor: (left, y),
            command: format!(
                "\x1b_Ga=p,q=2,i={},p={p},x={sx},y={sy},w={},h={},c={},r=1,z={},C=1;\x1b\\",
                self.image,
                ex - sx,
                ey - sy,
                right - left,
                self.z,
            ),
            delete: format!("\x1b_Ga=d,q=2,d=i,i={},p={p};\x1b\\", self.image),
            upload,
        }
    }
}

type Placements = HashMap<String, KittyPlacement>;

/// Only a successful, undiscarded flush confirms a placement. Keep attempted writes too, since a
/// failed frame may already have reached the host and needs cleanup if the desired scene changes.
#[derive(Default)]
pub(crate) struct HostComposedScene {
    confirmed: Placements,
    attempted: Placements,
    pending: Option<(Placements, usize, bool)>,
    /// Whether the pending scene came from a complete paint, which alone can show that an image
    /// has no placement left. A partial paint never stages the rows it did not repaint.
    pending_complete: bool,
}

impl HostComposedScene {
    pub(crate) fn prepare(
        &mut self,
        draw: &mut PreparedKittyDraw<'_>,
        epoch: usize,
        known: &HostKittyUploads,
    ) -> Vec<String> {
        let recovering = !self.attempted.is_empty()
            && self
                .pending
                .as_ref()
                .is_none_or(|(_, e, ready)| *e != epoch || !ready);
        let active = self
            .pending
            .as_ref()
            .filter(|(_, e, ready)| *e == epoch && *ready)
            .map_or(&self.confirmed, |(placements, _, _)| placements);
        let mut next = surviving_placements(draw, active);
        PAINT.with(|slot| {
            let paint = slot.borrow();
            self.pending_complete = !paint.partial;
            if paint.partial {
                preserve_unpainted(draw, active, &mut next, &paint.regions);
            } else {
                SPAN_IDS.with(|ids| ids.borrow_mut().retain(|_, entry| entry.used));
            }
        });
        restore_retained(draw, active, &next, known, recovering);
        if !recovering {
            suppress_unchanged(draw, active, known);
        }
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

    /// Returns the images that no longer have a confirmed placement. A host may evict such an
    /// image to stay within its storage limit, so the next placement must upload it again.
    pub(crate) fn flushed(&mut self, success: bool, epoch: usize) -> Vec<u32> {
        let Some((next, written_epoch, ready)) = self.pending.take() else {
            return Vec::new();
        };
        if !(success && ready && epoch == written_epoch) {
            return Vec::new();
        }
        let placed = placed_images(&next);
        let unplaced = if self.pending_complete {
            placed_images(&self.confirmed)
                .difference(&placed)
                .copied()
                .collect()
        } else {
            Vec::new()
        };
        self.confirmed = next;
        self.attempted.clear();
        unplaced
    }
}

fn placed_images(placements: &Placements) -> HashSet<u32> {
    placements
        .values()
        .filter_map(|placement| placement.upload.cache_key.get().map(|(id, _)| *id))
        .collect()
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

fn preserve_unpainted(
    draw: &PreparedKittyDraw<'_>,
    active: &Placements,
    next: &mut Placements,
    regions: &[ratatui::layout::Rect],
) {
    let mut damage = regions.to_vec();
    // Explicit regions include repainted-but-unchanged cells. Outgoing cells and staged spans
    // also cover direct backend users and row draws without an explicit region list.
    damage.extend(
        draw.cells
            .iter()
            .map(|(x, y, _)| ratatui::layout::Rect::new(*x, *y, 1, 1)),
    );
    damage.extend(next.values().filter_map(|p| p.geometry).map(|geometry| {
        let (left, right, y) = geometry.span;
        ratatui::layout::Rect::new(left, y, right - left, 1)
    }));
    for (key, placement) in active {
        if next.contains_key(key) {
            continue;
        }
        let Some(geometry) = placement.geometry else {
            continue;
        };
        let (left, right, y) = geometry.span;
        for (start, end) in uncovered_x_spans(left, right, y, &damage) {
            let retained = if (start, end) == (left, right) {
                placement.clone()
            } else {
                RowGeometry {
                    span: (start, end, y),
                    ..geometry
                }
                .placement(Arc::clone(&placement.upload))
            };
            next.insert(retained.delete.clone(), retained);
        }
    }
}

fn restore_retained(
    draw: &mut PreparedKittyDraw<'_>,
    active: &Placements,
    next: &Placements,
    known: &HostKittyUploads,
    recovering: bool,
) {
    let uploaded: HashSet<_> = draw
        .placements
        .iter()
        .filter(|placement| {
            !known.contains(&placement.upload)
                && draw
                    .uploads
                    .iter()
                    .any(|upload| Arc::ptr_eq(upload, &placement.upload))
        })
        .filter_map(|placement| placement.geometry.map(|geometry| geometry.image))
        .collect();
    let staged: HashSet<_> = draw
        .placements
        .iter()
        .map(|placement| &placement.delete)
        .collect();
    for (key, placement) in next {
        if !staged.contains(key)
            && (recovering
                || !active.contains_key(key)
                || placement
                    .geometry
                    .is_some_and(|geometry| uploaded.contains(&geometry.image)))
        {
            // Uploads replace all placements of that image ID. Restore preserved rows only after
            // drawing the cells containing those uploads, with an absolute cursor position.
            let (x, y) = placement.anchor;
            draw.restored_placements.push(format!(
                "\x1b[s\x1b[{};{}H{}\x1b[u",
                y + 1,
                x + 1,
                placement.command,
            ));
        }
    }
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
