//! Forward natural-size patches as ordinary Kitty placements. The host blends pixel overlap;
//! source images remain in the parser for captures, replay and reconnects.
use super::*;
use crate::widgets::{TerminalImageCrop, TerminalImagePlacement};

thread_local! {
    static NATIVE_LAYERS_SUPPORTED: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub(super) fn terminal_image_rows_can_batch() -> bool {
    NATIVE_LAYERS_SUPPORTED.with(|slot| slot.get())
}

pub(crate) struct NativeLayerScope(bool);

pub(crate) fn native_layer_scope(supported: bool) -> NativeLayerScope {
    NativeLayerScope(NATIVE_LAYERS_SUPPORTED.with(|slot| slot.replace(supported && slot.get())))
}

impl Drop for NativeLayerScope {
    fn drop(&mut self) {
        NATIVE_LAYERS_SUPPORTED.with(|slot| slot.set(self.0));
    }
}

static NEXT_NATIVE_ID: AtomicU64 = AtomicU64::new(1 << 31);
type RetiredNativeIds = HashMap<std::thread::ThreadId, Vec<u32>>;
fn retired_ids() -> &'static Mutex<RetiredNativeIds> {
    static RETIRED: OnceLock<Mutex<RetiredNativeIds>> = OnceLock::new();
    RETIRED.get_or_init(Mutex::default)
}

#[derive(Default)]
pub(crate) struct NativeImageLease {
    allocation: OnceLock<Option<Arc<NativeImageAllocation>>>,
    #[cfg(test)]
    protocol: Mutex<Option<Arc<EncodedProtocol>>>,
}

impl NativeImageLease {
    /// New immutable source revision sharing the logical source's host allocation.
    pub(crate) fn next_revision(&self) -> Self {
        let allocation = OnceLock::new();
        if let Some(existing) = self.allocation.get().cloned().flatten() {
            let _ = allocation.set(Some(existing));
        }
        Self {
            allocation,
            #[cfg(test)]
            protocol: Mutex::default(),
        }
    }

    pub(super) fn id(&self) -> Option<u32> {
        self.allocation().map(|allocation| allocation.id)
    }
    #[cfg(test)]
    pub(super) fn discard_unattempted(&self) {
        // Cache cleanup already holds the encoder lock. Never wait on a source whose
        // resolver may be holding its protocol lock while waiting for that encoder.
        if let Ok(mut canonical) = self.protocol.try_lock() {
            let unused = canonical.as_ref().is_some_and(|protocol| {
                Arc::strong_count(protocol) == 1 && matches!(&**protocol, EncodedProtocol::CompressedKitty(image)
                    if !image.upload.attempted.load(Ordering::Relaxed) && Arc::strong_count(&image.upload) == 1)
            });
            if unused {
                *canonical = None;
            }
        }
    }
    pub(super) fn backend_owned() -> Self {
        let allocation = OnceLock::new();
        let _ = allocation.set(allocate_native_id().map(|id| {
            Arc::new(NativeImageAllocation {
                id,
                owner: std::thread::current().id(),
                retire_on_drop: false,
            })
        }));
        Self {
            allocation,
            #[cfg(test)]
            protocol: Mutex::default(),
        }
    }

    fn allocation(&self) -> Option<Arc<NativeImageAllocation>> {
        self.allocation
            .get_or_init(|| {
                allocate_native_id().map(|id| {
                    Arc::new(NativeImageAllocation {
                        id,
                        owner: std::thread::current().id(),
                        retire_on_drop: true,
                    })
                })
            })
            .clone()
    }
}

fn allocate_native_id() -> Option<u32> {
    let mut id = NEXT_NATIVE_ID.load(Ordering::Relaxed);
    while id < u64::from(u32::MAX) {
        match NEXT_NATIVE_ID.compare_exchange_weak(id, id + 1, Ordering::Relaxed, Ordering::Relaxed)
        {
            Ok(_) => return Some(id as u32),
            Err(current) => id = current,
        }
    }
    None
}

struct NativeImageAllocation {
    retire_on_drop: bool,
    id: u32,
    owner: std::thread::ThreadId,
}

pub(super) struct NativeImageOwnership {
    _allocation: Arc<NativeImageAllocation>,
    pub(super) source: std::sync::Weak<NativeImageLease>,
}

impl Drop for NativeImageAllocation {
    fn drop(&mut self) {
        if !self.retire_on_drop {
            return;
        }
        retired_ids()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(self.owner)
            .or_default()
            .push(self.id);
    }
}

pub(crate) fn take_retired_native_ids() -> Vec<u32> {
    retired_ids()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .remove(&std::thread::current().id())
        .unwrap_or_default()
}

#[derive(Clone, Copy, Debug)]
struct NativeGeometry {
    origin: (i64, i64),
    source: TerminalImageCrop,
    cell: (u16, u16),
    z: i32,
}

impl NativeGeometry {
    fn new(p: &TerminalImagePlacement, content: Rect, cell: (u16, u16)) -> Option<Self> {
        let g = p.image.geometry()?;
        let source = p.source_crop.unwrap_or(TerminalImageCrop {
            x: 0,
            y: 0,
            width: p.image.width(),
            height: p.image.height(),
        });
        if g.cell != cell || g.size != (source.width, source.height) {
            return None;
        }
        if source.x.checked_add(source.width)? > p.image.width()
            || source.y.checked_add(source.height)? > p.image.height()
        {
            return None;
        }
        Some(Self {
            origin: (
                (i64::from(content.x) + i64::from(p.col)) * i64::from(cell.0)
                    + i64::from(g.offset.0),
                (i64::from(content.y) + i64::from(p.row)) * i64::from(cell.1)
                    + i64::from(g.offset.1),
            ),
            source,
            cell,
            z: p.z,
        })
    }

    fn overlaps(self, other: Self) -> bool {
        self.origin.0 < other.origin.0 + i64::from(other.source.width)
            && other.origin.0 < self.origin.0 + i64::from(self.source.width)
            && self.origin.1 < other.origin.1 + i64::from(other.source.height)
            && other.origin.1 < self.origin.1 + i64::from(self.source.height)
    }

    fn visible_row(self, left: u16, right: u16, y: u16) -> Option<NativeRegion> {
        let x0 = self.origin.0.max(i64::from(left) * i64::from(self.cell.0));
        let y0 = self.origin.1.max(i64::from(y) * i64::from(self.cell.1));
        let x1 = (self.origin.0 + i64::from(self.source.width))
            .min(i64::from(right) * i64::from(self.cell.0));
        let y1 = (self.origin.1 + i64::from(self.source.height))
            .min((i64::from(y) + 1) * i64::from(self.cell.1));
        if x0 >= x1 || y0 >= y1 {
            return None;
        }
        Some(NativeRegion {
            anchor: ((x0 / i64::from(self.cell.0)) as u16, y),
            offset: (
                (x0 % i64::from(self.cell.0)) as u32,
                (y0 % i64::from(self.cell.1)) as u32,
            ),
            z: self.z,
            source: TerminalImageCrop {
                x: self.source.x + (x0 - self.origin.0) as u32,
                y: self.source.y + (y0 - self.origin.1) as u32,
                width: (x1 - x0) as u32,
                height: (y1 - y0) as u32,
            },
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct NativeRegion {
    pub(super) anchor: (u16, u16),
    pub(super) offset: (u32, u32),
    pub(super) source: TerminalImageCrop,
    pub(super) z: i32,
}

impl NativeRegion {
    pub(super) fn command(self, image: &CompressedKitty, p: u32) -> String {
        let s = self.source;
        // No c/r: scaling a partially occupied cell would stretch the patch. X/Y place its first
        // pixel inside the cell; w/h are source pixels at the host's own cell resolution.
        format!(
            "\x1b_Ga=p,q=2,i={},p={p},x={},y={},w={},h={},X={},Y={},z={},C=1;\x1b\\",
            image.id, s.x, s.y, s.width, s.height, self.offset.0, self.offset.1, self.z
        )
    }
}

pub(crate) fn render_native_terminal_images(
    f: &mut ratatui::Frame<'_>,
    placements: &[TerminalImagePlacement],
    content: Rect,
    clip: ratatui::layout::Rect,
) -> bool {
    if !native_scene_supported(clip) {
        return false;
    }
    let Some((width, height)) = image_support::measured_host_cell_size() else {
        return false;
    };
    let mut holes = IMAGE_OCCLUSIONS.with(|slot| slot.borrow().clone());
    holes.extend(super::super::image_effects::pending_image_occlusions());
    let plan = native_scene::cached_plan().unwrap_or(TerminalGraphicsPlan {
        content,
        clip,
        holes,
        cell: (width, height),
    });
    let Some(regions) = plan.regions(placements) else {
        return false;
    };
    native_scene::stage_plan(plan);
    for (placement, regions) in placements.iter().zip(regions) {
        native_scene::stage(placement, &regions);
    }
    let _ = f;

    true
}

/// Geometry and occlusion captured by a successful ordinary render.
#[derive(Clone)]
pub(super) struct TerminalGraphicsPlan {
    content: Rect,
    clip: ratatui::layout::Rect,
    holes: Vec<ratatui::layout::Rect>,
    cell: (u16, u16),
}

impl TerminalGraphicsPlan {
    pub(super) fn visible(&self) -> bool {
        !self.clip.is_empty()
            && (self.clip.y..self.clip.bottom()).any(|row| {
                !uncovered_x_spans(self.clip.x, self.clip.right(), row, &self.holes).is_empty()
            })
    }

    pub(super) fn regions(
        &self,
        placements: &[TerminalImagePlacement],
    ) -> Option<Vec<Vec<NativeRegion>>> {
        if image_support::measured_host_cell_size() != Some(self.cell)
            || !native_scene_supported(self.clip)
        {
            return None;
        }
        let geometry = scene_geometry(placements, self.content, self.cell)?;
        let mut total_regions = 0;
        placements
            .iter()
            .zip(geometry)
            .map(|(p, geometry)| {
                let regions = native_regions(geometry, self.clip, &self.holes);
                total_regions += regions.len();
                if total_regions > MAX_NATIVE_REGIONS
                    || (!regions.is_empty()
                        && p.image.file_pixels().is_none()
                        && p.image.pixels().is_none())
                {
                    return None;
                }
                Some(regions)
            })
            .collect()
    }
}

fn native_scene_supported(clip: ratatui::layout::Rect) -> bool {
    if capturing_images()
        || image_support::image_rendering_suspended()
        || !NATIVE_LAYERS_SUPPORTED.with(|slot| slot.get())
    {
        return false;
    }
    if std::env::var("TUI_LIPAN_TERMINAL_IMAGES").is_ok_and(|mode| mode == "composed") {
        return false;
    }
    let protocol = protocol_type_to_public(image_support::picker_snapshot().protocol_type());
    let area = Rect {
        x: clip.x as i16,
        y: clip.y as i16,
        w: clip.width,
        h: clip.height,
    };
    protocol == ImageProtocol::Kitty && live_backdrop_mask(area, protocol).is_none()
}

fn scene_geometry(
    placements: &[TerminalImagePlacement],
    content: Rect,
    cell: (u16, u16),
) -> Option<Vec<NativeGeometry>> {
    let geometry: Vec<_> = placements
        .iter()
        .map(|p| NativeGeometry::new(p, content, cell))
        .collect::<Option<_>>()?;
    // Host IDs do not preserve the producer's same-z ordering, so those overlaps compose.
    // Exact z values stay intact, including the very low plane below cell backgrounds.
    for (index, p) in placements.iter().enumerate() {
        if placements[index + 1..]
            .iter()
            .zip(&geometry[index + 1..])
            .any(|(other, g)| other.z == p.z && geometry[index].overlaps(*g))
        {
            return None;
        }
    }
    Some(geometry)
}

#[cfg(test)]
fn native_protocol(p: &TerminalImagePlacement) -> Option<Arc<EncodedProtocol>> {
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    (
        "native-terminal-patches",
        p.image.stream_namespace(),
        p.image_id,
    )
        .hash(&mut hash);
    let stream = hash.finish();
    let lease = p.image.native_lease();
    // The source owns one canonical protocol independently of the evictable render cache.
    // Uploads own only the allocation, so this cannot form an Arc cycle with the source.
    let mut canonical = lease.protocol.lock().ok()?;
    if let Some(protocol) = canonical.as_ref() {
        return Some(Arc::clone(protocol));
    }
    let id = lease.id()?;
    id.hash(&mut hash);
    (p.image.source_hash(), p.image.width(), p.image.height()).hash(&mut hash);
    let key = RenderCacheKey {
        source_hash: hash.finish(),
        frame_index: 0,
        width: 1,
        height: 1,
        crop: None,
        background_rgb: None,
        fit: ImageFit::Scale,
        protocol: ImageProtocol::Auto,
        resolved_protocol: ImageProtocol::Kitty,
        z_index: 0,
        backdrop: 0,
    };
    let encoder = async_encoder();
    if let Some(encoded) = encoder.cache_get(&key) {
        *canonical = Some(Arc::clone(&encoded));
        return Some(encoded);
    }
    let (protocol, bytes) = encode_native_source(p, id, Arc::clone(&lease))?;
    let mut inner = encoder.inner.lock().ok()?;
    inner.cache.insert(
        stream,
        key,
        Arc::clone(&protocol),
        bytes,
        CacheRetention::LatestOnly,
    );
    inner
        .cache
        .mark_terminal_owner(&key, p.image.image_lifetime());
    *canonical = Some(Arc::clone(&protocol));
    Some(protocol)
}

pub(super) fn encode_native_source(
    p: &TerminalImagePlacement,
    id: u32,
    lease: Arc<NativeImageLease>,
) -> Option<(Arc<EncodedProtocol>, usize)> {
    let size = ratatui::layout::Size::new(1, 1);
    let file = p
        .image
        .file_pixels()
        .filter(|_| shared_frame::host_reads_shared_memory());
    let encoded = file
        .and_then(|input| {
            CompressedKitty::from_file(input, (p.image.width(), p.image.height()), size, id, 0)
        })
        .or_else(|| CompressedKitty::new_native(p.image.pixels()?, size, id))?;
    let file_backed = encoded.upload.file.lock().ok()?.is_some();
    encoded
        .upload
        .native_lease
        .set(NativeImageOwnership {
            _allocation: lease.allocation()?,
            source: Arc::downgrade(&lease),
        })
        .ok()?;
    let bytes = if file_backed {
        1024
    } else {
        usize::try_from(u64::from(p.image.width()) * u64::from(p.image.height()) * 4).ok()?
    };
    Some((Arc::new(EncodedProtocol::CompressedKitty(encoded)), bytes))
}

/// Bound host commands under pathological widget occlusion. Exceeding the limit
/// selects retained composition for the entire terminal scene.
const MAX_NATIVE_REGIONS: usize = 512;

fn native_regions(
    geometry: NativeGeometry,
    clip: ratatui::layout::Rect,
    holes: &[ratatui::layout::Rect],
) -> Vec<NativeRegion> {
    let mut regions: Vec<NativeRegion> = Vec::new();
    let mut before_merge = 0;
    let mut previous: HashMap<(u16, u32, u32, u32), usize> = HashMap::new();
    for y in clip.y..clip.bottom() {
        let mut current = HashMap::new();
        for (left, right) in uncovered_x_spans(clip.x, clip.right(), y, holes) {
            let Some(region) = geometry.visible_row(left, right, y) else {
                continue;
            };
            before_merge += 1;
            let key = (
                region.anchor.0,
                region.offset.0,
                region.source.x,
                region.source.width,
            );
            let index = match previous.get(&key).copied() {
                Some(index) if vertically_contiguous(regions[index], region, geometry.cell.1) => {
                    regions[index].source.height += region.source.height;
                    index
                }
                _ => {
                    regions.push(region);
                    regions.len() - 1
                }
            };
            current.insert(key, index);
            if regions.len() > MAX_NATIVE_REGIONS {
                return record_region_metrics(before_merge, regions);
            }
        }
        previous = current;
    }
    record_region_metrics(before_merge, regions)
}

fn record_region_metrics(before: u64, regions: Vec<NativeRegion>) -> Vec<NativeRegion> {
    crate::core::terminal_metrics::update(|metrics| {
        metrics.regions_before_merge += before;
        metrics.regions_after_merge += regions.len() as u64;
    });
    regions
}

fn vertically_contiguous(top: NativeRegion, bottom: NativeRegion, cell_height: u16) -> bool {
    top.source.y + top.source.height == bottom.source.y
        && u32::from(top.anchor.1) * u32::from(cell_height) + top.offset.1 + top.source.height
            == u32::from(bottom.anchor.1) * u32::from(cell_height) + bottom.offset.1
}

#[cfg(test)]
fn render_native_regions(
    image: &CompressedKitty,
    f: &mut ratatui::Frame<'_>,
    regions: &[NativeRegion],
) {
    let mut transmit = image.pending_transmission();
    for region in regions {
        let p = NEXT_KITTY_PLACEMENT.with(|next| {
            let p = next.get().wrapping_add(1).max(1);
            next.set(p);
            p
        });
        image.stage_placement(f, region.anchor, region.command(image, p), p, &mut transmit);
    }
}

#[cfg(test)]
fn render_native_image(
    image: &CompressedKitty,
    f: &mut ratatui::Frame<'_>,
    geometry: NativeGeometry,
    clip: ratatui::layout::Rect,
) {
    let holes = IMAGE_OCCLUSIONS.with(|slot| slot.borrow().clone());
    let regions: Vec<_> = (clip.y..clip.bottom())
        .flat_map(|y| {
            uncovered_x_spans(clip.x, clip.right(), y, &holes)
                .into_iter()
                .filter_map(move |(left, right)| geometry.visible_row(left, right, y))
        })
        .collect();
    render_native_regions(image, f, &regions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::{TerminalCellSize, TerminalScreen};
    use ratatui::backend::Backend;

    fn scene(keys: &str, size: (u32, u32)) -> Vec<TerminalImagePlacement> {
        let mut screen = TerminalScreen::new(6, 8, 0);
        screen.set_cell_size(TerminalCellSize::new(10, 20));
        screen.process_bytes(
            format!(
                "\x1b_Ga=T,f=32,q=2,C=1,{keys},s={},v={};{}\x1b\\",
                size.0,
                size.1,
                BASE64.encode([10, 20, 30, 255].repeat((size.0 * size.1) as usize))
            )
            .as_bytes(),
        );
        screen.render_snapshot().images.to_vec()
    }

    #[test]
    fn native_geometry_preserves_pixel_offsets_and_clips_without_scaling() {
        let p = scene("i=1,X=3,Y=5,z=2", (17, 23)).remove(0);
        let content = Rect {
            x: 2,
            y: 3,
            w: 8,
            h: 6,
        };
        let g = NativeGeometry::new(&p, content, (10, 20)).unwrap();
        assert_eq!(g.origin, (23, 65));
        let first = g.visible_row(2, 8, 3).unwrap();
        assert_eq!(first.anchor, (2, 3));
        assert_eq!(first.offset, (3, 5));
        assert_eq!(
            first.source,
            TerminalImageCrop {
                x: 0,
                y: 0,
                width: 17,
                height: 15
            }
        );
        let clipped = g.visible_row(3, 8, 4).unwrap();
        assert_eq!(clipped.anchor, (3, 4));
        assert_eq!(clipped.offset, (0, 0));
        assert_eq!(
            clipped.source,
            TerminalImageCrop {
                x: 7,
                y: 15,
                width: 10,
                height: 8
            }
        );
        assert!(g.visible_row(4, 8, 3).is_none());
        assert!(NativeGeometry::new(&p, content, (11, 20)).is_none());
    }

    #[test]
    fn native_geometry_preserves_an_image_scrolled_above_and_left_of_the_pane() {
        let mut p = scene("i=1,X=3,Y=5,z=2", (27, 43)).remove(0);
        p.col = -1;
        p.row = -1;
        let g = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
        let visible = g.visible_row(0, 8, 0).unwrap();
        assert_eq!(
            visible.source,
            TerminalImageCrop {
                x: 7,
                y: 15,
                width: 20,
                height: 20
            }
        );
        assert_eq!(visible.offset, (0, 0));
    }

    #[test]
    fn scaling_and_same_depth_pixel_overlap_use_composition() {
        let mut p = scene("i=1,X=3,Y=5,z=0", (17, 23)).remove(0);
        let mut other = p.clone();
        other.image_id = 2;
        assert!(scene_geometry(&[p.clone(), other.clone()], Rect::default(), (10, 20)).is_none());
        other.z = 1;
        assert!(scene_geometry(&[p.clone(), other.clone()], Rect::default(), (10, 20)).is_some());
        other.z = 0;
        other.col = 3;
        assert!(scene_geometry(&[p.clone(), other], Rect::default(), (10, 20)).is_some());
        p.source_crop = Some(TerminalImageCrop {
            x: 0,
            y: 0,
            width: 3,
            height: 4,
        });
        assert!(scene_geometry(&[p], Rect::default(), (10, 20)).is_none());
    }

    #[test]
    fn native_rows_survive_lost_upload_anchors_and_are_deleted_when_the_pane_closes() {
        begin_kitty_frame();
        clear_image_occlusions();
        let p = scene("i=1,X=3,Y=5,z=2", (17, 43)).remove(0);
        let geometry = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
        let encoded = CompressedKitty::new(
            p.image.pixels().unwrap(),
            ratatui::layout::Size::new(p.cols, p.rows),
            77,
            2,
        )
        .unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 6)).unwrap();
        terminal
            .draw(|f| render_native_image(&encoded, f, geometry, f.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut output = Vec::new();
        {
            let mut host =
                crate::backend::ratatui_backend::native_terminal::HostBackend::new(&mut output);
            // A later pass removed the first row carrying the upload. Other rows still draw.
            host.draw((1..3).map(|y| (0, y, &buffer[(0, y)]))).unwrap();
            Backend::flush(&mut host).unwrap();
            assert!(!encoded.transmission_pending());
            begin_kitty_frame();
            host.draw(std::iter::empty()).unwrap();
            Backend::flush(&mut host).unwrap();
        }
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("a=T").count(), 1);
        assert_eq!(output.matches("a=p").count(), 2);
        assert_eq!(output.matches("a=d").count(), 2);
        assert!(output.contains("x=0,y=15,w=17,h=20,X=3,Y=0,z=2,C=1"));
        assert!(output.contains("x=0,y=35,w=17,h=8,X=3,Y=0,z=2,C=1"));
        assert!(!output.contains("r=1"));
    }

    #[test]
    fn many_native_anchors_retire_only_surviving_placements_and_share_uploads() {
        begin_kitty_frame();
        clear_image_occlusions();
        let mut p = scene("i=1,z=2", (10, 20)).remove(0);
        let base = CompressedKitty::new(
            p.image.pixels().unwrap(),
            ratatui::layout::Size::new(1, 1),
            77,
            2,
        )
        .unwrap();
        let patch = CompressedKitty::new(
            p.image.pixels().unwrap(),
            ratatui::layout::Size::new(1, 1),
            88,
            3,
        )
        .unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(64, 2)).unwrap();
        terminal
            .draw(|f| {
                for col in 0..64 {
                    p.col = col;
                    let g = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
                    render_native_image(&base, f, g, f.area());
                }
                p.col = 1;
                let mut g = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
                g.z = 3;
                render_native_image(&patch, f, g, f.area());
                // A later UI pass hides half the anchors, including the initial upload row.
                for col in (0..64).step_by(2) {
                    f.buffer_mut()[(col, 0)].set_symbol("overlay");
                }
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let mut output = Vec::new();
        {
            let mut host =
                crate::backend::ratatui_backend::native_terminal::HostBackend::new(&mut output);
            host.draw((0..64).rev().map(|x| (x, 0, &buffer[(x, 0)])))
                .unwrap();
            Backend::flush(&mut host).unwrap();
            begin_kitty_frame();
            host.draw(std::iter::empty()).unwrap();
            Backend::flush(&mut host).unwrap();
        }
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("a=T").count(), 2);
        assert_eq!(output.matches("a=p").count(), 33);
        assert_eq!(output.matches("a=d").count(), 33);
        assert!(!base.transmission_pending());
        assert!(!patch.transmission_pending());
    }

    #[test]
    fn native_rows_cut_holes_without_stretching_pixels() {
        begin_kitty_frame();
        set_image_occlusions(vec![ratatui::layout::Rect::new(1, 0, 1, 2)]);
        let p = scene("i=1,z=-1", (30, 40)).remove(0);
        let g = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
        let encoded = CompressedKitty::new(
            p.image.pixels().unwrap(),
            ratatui::layout::Size::new(3, 2),
            77,
            -1,
        )
        .unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(3, 2)).unwrap();
        terminal
            .draw(|f| render_native_image(&encoded, f, g, f.area()))
            .unwrap();
        let buffer = terminal.backend().buffer();
        for y in 0..2 {
            assert!(!buffer[(1, y)].symbol().contains("a=p"));
            assert!(buffer[(0, y)].symbol().contains("x=0,"));
            assert!(buffer[(2, y)].symbol().contains("x=20,"));
            assert!(buffer[(0, y)].symbol().contains("w=10,h=20,X=0,Y=0,z=-1"));
        }
        clear_image_occlusions();
        begin_kitty_frame();
    }
    #[test]
    fn source_and_staged_upload_keep_the_native_image_alive_until_host_cleanup() {
        let lease = Arc::new(NativeImageLease::default());
        let id = lease.id().unwrap();
        let encoded = CompressedKitty::new(
            &image::DynamicImage::new_rgba8(1, 1),
            ratatui::layout::Size::new(1, 1),
            id,
            0,
        )
        .unwrap();
        encoded
            .upload
            .native_lease
            .set(NativeImageOwnership {
                _allocation: lease.allocation().unwrap(),
                source: Arc::downgrade(&lease),
            })
            .ok()
            .unwrap();
        let upload = Arc::clone(&encoded.upload);
        drop(lease);
        drop(encoded);
        assert!(!take_retired_native_ids().contains(&id));
        drop(upload);
        let mut output = Vec::new();
        {
            let mut host =
                crate::backend::ratatui_backend::native_terminal::HostBackend::new(&mut output);
            host.draw(std::iter::empty()).unwrap();
            Backend::flush(&mut host).unwrap();
        }
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains(&format!("d=I,i={id}"))
        );
    }

    #[test]
    fn identical_payload_replacements_receive_distinct_host_ids() {
        let first = NativeImageLease::default();
        let second = NativeImageLease::default();
        assert_ne!(first.id(), second.id());
        assert_eq!(first.id(), first.id());
        assert!(first.id().unwrap() >= 1 << 31);
    }
    #[test]
    fn fully_covered_and_offscreen_images_have_no_native_regions() {
        let p = scene("i=1", (30, 40)).remove(0);
        let g = NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap();
        let clip = ratatui::layout::Rect::new(0, 0, 8, 6);
        assert!(native_regions(g, clip, &[ratatui::layout::Rect::new(0, 0, 3, 2)]).is_empty());
        assert!(native_regions(g, ratatui::layout::Rect::new(4, 3, 2, 2), &[]).is_empty());
    }

    #[test]
    fn a_worker_drops_the_source_but_only_the_owning_host_retires_it() {
        let lease = NativeImageLease::default();
        let id = lease.id().unwrap();
        std::thread::spawn(move || {
            drop(lease);
            assert!(!take_retired_native_ids().contains(&id));
        })
        .join()
        .unwrap();
        assert!(take_retired_native_ids().contains(&id));
    }

    #[test]
    fn discarded_native_storage_cleanup_retries_after_recovery() {
        let lease = NativeImageLease::default();
        let id = lease.id().unwrap();
        let (mut host, state) =
            crate::backend::ratatui_backend::native_terminal::buffered_test_output();
        drop(lease);
        host.draw(std::iter::empty()).unwrap();
        crate::app::job_control::with_released_terminal(&state, || {
            Backend::flush(&mut host).unwrap()
        });
        assert!(host.test_output().is_empty());
        host.draw(std::iter::empty()).unwrap();
        Backend::flush(&mut host).unwrap();
        let output = String::from_utf8(host.test_output()).unwrap();
        assert_eq!(output.matches(&format!("d=I,i={id};")).count(), 1);
        host.draw(std::iter::empty()).unwrap();
        Backend::flush(&mut host).unwrap();
        assert_eq!(
            String::from_utf8(host.test_output())
                .unwrap()
                .matches(&format!("d=I,i={id};"))
                .count(),
            1
        );
    }

    #[test]
    fn failed_native_storage_cleanup_retries_until_flushed() {
        #[derive(Default)]
        struct State {
            reject: bool,
            output: Vec<u8>,
        }
        struct Writer(std::rc::Rc<std::cell::RefCell<State>>);
        impl std::io::Write for Writer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.borrow_mut().output.extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                if self.0.borrow().reject {
                    Err(std::io::Error::other("failed flush"))
                } else {
                    Ok(())
                }
            }
        }
        let lease = NativeImageLease::default();
        let id = lease.id().unwrap();
        drop(lease);
        let state = std::rc::Rc::new(std::cell::RefCell::new(State {
            reject: true,
            ..Default::default()
        }));
        let mut host = crate::backend::ratatui_backend::native_terminal::HostBackend::new(Writer(
            state.clone(),
        ));
        host.draw(std::iter::empty()).unwrap();
        assert!(Backend::flush(&mut host).is_err());
        state.borrow_mut().reject = false;
        host.draw(std::iter::empty()).unwrap();
        Backend::flush(&mut host).unwrap();
        let output = state.borrow().output.clone();
        assert_eq!(
            String::from_utf8(output.clone())
                .unwrap()
                .matches(&format!("d=I,i={id};"))
                .count(),
            2
        );
        host.draw(std::iter::empty()).unwrap();
        Backend::flush(&mut host).unwrap();
        assert_eq!(
            String::from_utf8(state.borrow().output.clone())
                .unwrap()
                .matches(&format!("d=I,i={id};"))
                .count(),
            2
        );
    }
    #[test]
    fn one_source_upload_serves_placements_at_different_depths_and_offsets() {
        begin_kitty_frame();
        clear_image_occlusions();
        let p = scene("i=1,z=-1", (17, 23)).remove(0);
        let mut upper = p.clone();
        upper.z = 3;
        upper.col = 2;
        upper.cols = 3;
        let first = native_protocol(&p).unwrap();
        let second = native_protocol(&upper).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let EncodedProtocol::CompressedKitty(image) = &*first else {
            panic!("Kitty source")
        };
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 6)).unwrap();
        terminal
            .draw(|f| {
                render_native_image(
                    image,
                    f,
                    NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap(),
                    f.area(),
                );
                render_native_image(
                    image,
                    f,
                    NativeGeometry::new(&upper, Rect::default(), (10, 20)).unwrap(),
                    f.area(),
                );
            })
            .unwrap();
        let mut output = Vec::new();
        {
            let mut host =
                crate::backend::ratatui_backend::native_terminal::HostBackend::new(&mut output);
            let buffer = terminal.backend().buffer();
            host.draw((0..6).flat_map(|y| (0..8).map(move |x| (x, y, &buffer[(x, y)]))))
                .unwrap();
            Backend::flush(&mut host).unwrap();
        }
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("a=T").count(), 1);
        assert_eq!(output.matches("a=p,").count(), 4);
        assert_eq!(output.matches("z=-1,C=1").count(), 2);
        assert_eq!(output.matches("z=3,C=1").count(), 2);
        begin_kitty_frame();
    }
    #[test]
    fn same_anchor_depths_emit_one_upload_before_both_placements() {
        assert_native_upload_lifecycle(false);
    }

    #[test]
    fn cache_eviction_between_resolutions_preserves_one_host_upload() {
        assert_native_upload_lifecycle(true);
    }

    fn assert_native_upload_lifecycle(evict: bool) {
        begin_kitty_frame();
        clear_image_occlusions();
        let p = scene("i=1,z=0", (10, 20)).remove(0);
        let mut upper = p.clone();
        upper.z = 1;
        let first = native_protocol(&p).unwrap();
        if evict {
            let mut inner = async_encoder().inner.lock().unwrap();
            inner.cache.remove_unattempted_native_images();
            while !inner.cache.entries.is_empty() {
                inner.cache.remove_at(0);
            }
            inner.cache.remove_unattempted_native_images();
        }
        let second = native_protocol(&upper).unwrap();
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(2, 1)).unwrap();
        terminal
            .draw(|f| {
                for (protocol, placement) in [(&first, &p), (&second, &upper)] {
                    let EncodedProtocol::CompressedKitty(image) = &**protocol else {
                        panic!("Kitty source")
                    };
                    render_native_image(
                        image,
                        f,
                        NativeGeometry::new(placement, Rect::default(), (10, 20)).unwrap(),
                        f.area(),
                    );
                }
            })
            .unwrap();
        let mut output = Vec::new();
        {
            let mut host =
                crate::backend::ratatui_backend::native_terminal::HostBackend::new(&mut output);
            let buffer = terminal.backend().buffer();
            host.draw((0..2).map(|x| (x, 0, &buffer[(x, 0)]))).unwrap();
            Backend::flush(&mut host).unwrap();
        }
        let output = String::from_utf8(output).unwrap();
        assert_eq!(output.matches("a=T").count(), 1, "{output:?}");
        assert_eq!(output.matches("a=p,").count(), 2);
        assert!(output.find("a=T").unwrap() < output.find("a=p,").unwrap());
        let EncodedProtocol::CompressedKitty(image) = &*first else {
            unreachable!()
        };
        assert!(
            image.pending_transmission().is_none(),
            "only the emitted upload is acknowledged"
        );
        begin_kitty_frame();
    }

    #[test]
    fn discarded_native_upload_retries_after_cache_eviction() {
        begin_kitty_frame();
        let p = scene("i=1,z=0", (10, 20)).remove(0);
        let protocol = native_protocol(&p).unwrap();
        let EncodedProtocol::CompressedKitty(image) = &*protocol else {
            unreachable!()
        };
        let mut frame = ratatui::Terminal::new(ratatui::backend::TestBackend::new(1, 1)).unwrap();
        frame
            .draw(|f| {
                render_native_image(
                    image,
                    f,
                    NativeGeometry::new(&p, Rect::default(), (10, 20)).unwrap(),
                    f.area(),
                )
            })
            .unwrap();
        let cell = &frame.backend().buffer()[(0, 0)];
        let (mut host, state) =
            crate::backend::ratatui_backend::native_terminal::buffered_test_output();
        host.draw(std::iter::once((0, 0, cell))).unwrap();
        crate::app::job_control::with_released_terminal(&state, || {
            Backend::flush(&mut host).unwrap()
        });
        assert!(host.test_output().is_empty());
        assert!(image.pending_transmission().is_some());
        {
            let mut inner = async_encoder().inner.lock().unwrap();
            while !inner.cache.entries.is_empty() {
                inner.cache.remove_at(0);
            }
        }
        assert!(Arc::ptr_eq(&protocol, &native_protocol(&p).unwrap()));
        host.draw(std::iter::once((0, 0, cell))).unwrap();
        Backend::flush(&mut host).unwrap();
        let output = String::from_utf8(host.test_output()).unwrap();
        assert_eq!(output.matches("a=T").count(), 1);
        assert_eq!(output.matches("a=p,").count(), 1);
        assert!(output.find("a=T").unwrap() < output.find("a=p,").unwrap());
        assert!(image.pending_transmission().is_none());
        begin_kitty_frame();
    }

    #[test]
    fn matching_guessed_cells_cannot_enable_native_placements() {
        let p = scene("i=1", (10, 20));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(2, 1)).unwrap();
        for measured in [false, true] {
            let _host = image_support::test_kitty_host((10, 20), measured);
            begin_kitty_frame();
            terminal
                .draw(|f| {
                    assert_eq!(
                        render_native_terminal_images(f, &p, Rect::default(), f.area()),
                        measured
                    );
                })
                .unwrap();
        }
        begin_kitty_frame();
    }

    #[test]
    fn a_large_inline_source_keeps_the_asynchronous_composed_path() {
        assert!(!shared_frame::host_reads_shared_memory());
        let p = scene("i=1", (300, 300)).remove(0);
        assert!(native_protocol(&p).is_none());
        assert!(
            CompressedKitty::new(
                p.image.pixels().unwrap(),
                ratatui::layout::Size::new(30, 15),
                77,
                0
            )
            .is_some()
        );
    }

    #[test]
    fn abandoned_native_preparations_release_their_cache_resources() {
        let lease = Arc::new(NativeImageLease::default());
        let id = lease.id().unwrap();
        let encoded = CompressedKitty::new(
            &image::DynamicImage::new_rgba8(1, 1),
            ratatui::layout::Size::new(1, 1),
            id,
            0,
        )
        .unwrap();
        encoded
            .upload
            .native_lease
            .set(NativeImageOwnership {
                _allocation: lease.allocation().unwrap(),
                source: Arc::downgrade(&lease),
            })
            .ok()
            .unwrap();
        let upload = Arc::downgrade(&encoded.upload);
        let key = RenderCacheKey {
            source_hash: 1,
            frame_index: 0,
            width: 1,
            height: 1,
            crop: None,
            background_rgb: None,
            fit: ImageFit::Scale,
            protocol: ImageProtocol::Auto,
            resolved_protocol: ImageProtocol::Kitty,
            z_index: 0,
            backdrop: 0,
        };
        let mut cache = ImageRenderCache::default();
        cache.insert(
            1,
            key,
            Arc::new(EncodedProtocol::CompressedKitty(encoded)),
            4,
            CacheRetention::LatestOnly,
        );
        assert!(upload.upgrade().is_some());
        cache.remove_unattempted_native_images();
        assert!(upload.upgrade().is_none());
        assert!(cache.entries.is_empty());
    }
}

#[cfg(test)]
mod rectangle_tests {
    use super::*;

    fn geometry() -> NativeGeometry {
        NativeGeometry {
            origin: (0, 0),
            source: TerminalImageCrop {
                x: 0,
                y: 0,
                width: 800,
                height: 1000,
            },
            cell: (10, 20),
            z: 1,
        }
    }

    #[test]
    fn region_bound_applies_to_the_complete_terminal_scene() {
        let _host = image_support::test_kitty_host((10, 20), true);
        let mut source = crate::widgets::TerminalScreen::new(1, 1024, 0);
        source.set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        source.process_bytes(b"\x1b_Ga=T,i=1,C=1,f=24,s=1,v=1,q=2;YWJj\x1b\\");
        let base = source.render_snapshot().images[0].clone();
        let placements: Vec<_> = (0..=MAX_NATIVE_REGIONS)
            .map(|col| {
                let mut placement = base.clone();
                placement.col = col as i32;
                placement
            })
            .collect();
        let plan = TerminalGraphicsPlan {
            content: Rect {
                x: 0,
                y: 0,
                w: 1024,
                h: 1,
            },
            clip: ratatui::layout::Rect::new(0, 0, 1024, 1),
            holes: Vec::new(),
            cell: (10, 20),
        };
        assert!(plan.regions(&placements[..MAX_NATIVE_REGIONS]).is_some());
        assert!(plan.regions(&placements).is_none());
    }

    #[test]
    fn unobstructed_pane_compacts_to_one_rectangle() {
        let regions = native_regions(geometry(), ratatui::layout::Rect::new(0, 0, 80, 50), &[]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].source, geometry().source);
    }

    #[test]
    fn rectangular_modal_produces_four_rectangles() {
        let regions = native_regions(
            geometry(),
            ratatui::layout::Rect::new(0, 0, 80, 50),
            &[ratatui::layout::Rect::new(20, 10, 40, 20)],
        );
        assert_eq!(regions.len(), 4);
        let area: u32 = regions
            .iter()
            .map(|r| r.source.width * r.source.height)
            .sum();
        assert_eq!(area, 800 * 1000 - 400 * 400);
    }

    #[test]
    fn merging_preserves_partial_cells_and_source_crop() {
        let geometry = NativeGeometry {
            origin: (3, 7),
            source: TerminalImageCrop {
                x: 11,
                y: 13,
                width: 45,
                height: 62,
            },
            ..geometry()
        };
        let regions = native_regions(geometry, ratatui::layout::Rect::new(0, 0, 80, 50), &[]);
        assert_eq!(regions.len(), 1);
        assert_eq!(regions[0].offset, (3, 7));
        assert_eq!(regions[0].source, geometry.source);
        let clipped = native_regions(geometry, ratatui::layout::Rect::new(1, 1, 2, 2), &[]);
        assert_eq!(clipped.len(), 1);
        assert_eq!(
            clipped[0].source,
            TerminalImageCrop {
                x: 18,
                y: 26,
                width: 20,
                height: 40
            }
        );
    }

    #[test]
    fn pathological_occlusion_stops_at_region_limit() {
        let holes: Vec<_> = (0..50)
            .flat_map(|y| (0..40).map(move |x| ratatui::layout::Rect::new(2 * x + y % 2, y, 1, 1)))
            .collect();
        let regions = native_regions(geometry(), ratatui::layout::Rect::new(0, 0, 80, 50), &holes);
        assert_eq!(regions.len(), MAX_NATIVE_REGIONS + 1);
    }
}
