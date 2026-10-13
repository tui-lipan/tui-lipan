//! Backend-owned, transactional native Kitty presentation state.
use super::*;
use crate::core::node::NodeId;
use crate::widgets::{
    ImagePlacementKey, ImageSourceKey, ImageSourceRevision, TerminalImagePlacement,
};

#[derive(Debug, thiserror::Error)]
#[error("native source encoding unavailable")]
pub(crate) struct NativeSourceUnavailable;

pub(crate) fn native_source_unavailable(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .is_some_and(|inner| inner.is::<NativeSourceUnavailable>())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct PlacementKey {
    owner: NodeId,
    logical: ImagePlacementKey,
    region: usize,
}

#[derive(Clone)]
pub(super) struct DesiredPlacement {
    key: PlacementKey,
    source: TerminalImagePlacement,
    geometry: native_patches::NativeRegion,
}

#[derive(Default)]
pub(crate) struct DesiredScene {
    full: bool,
    owners: HashSet<NodeId>,
    placements: Vec<DesiredPlacement>,
    plans: HashMap<NodeId, native_patches::TerminalGraphicsPlan>,
}

thread_local! {
    // Only frame staging lives here. Committed resources belong to HostBackend.
    static DESIRED: RefCell<Option<DesiredScene>> = const { RefCell::new(None) };
    static CACHED_PLANS: RefCell<Option<HashMap<NodeId, native_patches::TerminalGraphicsPlan>>> = const { RefCell::new(None) };
    static OWNER: Cell<NodeId> = const { Cell::new(NodeId::INVALID) };
}

pub(crate) struct NativePlanScope(Option<HashMap<NodeId, native_patches::TerminalGraphicsPlan>>);

impl Drop for NativePlanScope {
    fn drop(&mut self) {
        CACHED_PLANS.with(|slot| {
            slot.replace(self.0.take());
        });
    }
}

pub(super) fn cached_plan() -> Option<native_patches::TerminalGraphicsPlan> {
    CACHED_PLANS.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|plans| plans.get(&OWNER.get()))
            .cloned()
    })
}

pub(crate) fn begin(full: bool) {
    DESIRED.with(|slot| {
        *slot.borrow_mut() = Some(DesiredScene {
            full,
            ..Default::default()
        })
    });
}

pub(crate) fn take_desired() -> Option<DesiredScene> {
    DESIRED.with(|slot| slot.borrow_mut().take())
}

pub(crate) struct OwnerScope(NodeId);

pub(crate) fn owner_scope(owner: NodeId) -> OwnerScope {
    if !capturing_images() {
        DESIRED.with(|slot| {
            if let Some(scene) = slot.borrow_mut().as_mut() {
                scene.owners.insert(owner);
            }
        });
    }
    OwnerScope(OWNER.with(|slot| slot.replace(owner)))
}

impl Drop for OwnerScope {
    fn drop(&mut self) {
        OWNER.with(|slot| slot.set(self.0));
    }
}

pub(super) fn stage_plan(plan: native_patches::TerminalGraphicsPlan) {
    DESIRED.with(|slot| {
        if let Some(scene) = slot.borrow_mut().as_mut() {
            scene.plans.insert(OWNER.get(), plan);
        }
    });
}

pub(super) fn stage(source: &TerminalImagePlacement, regions: &[native_patches::NativeRegion]) {
    DESIRED.with(|slot| {
        if let Some(scene) = slot.borrow_mut().as_mut() {
            for (region, geometry) in regions.iter().enumerate() {
                scene.placements.push(DesiredPlacement {
                    key: PlacementKey {
                        owner: OWNER.get(),
                        logical: source.placement_key(),
                        region,
                    },
                    source: source.clone(),
                    geometry: *geometry,
                });
            }
        }
    });
}

#[derive(Clone)]
struct SourceState {
    source: TerminalImagePlacement,
    revision: ImageSourceRevision,
    id: u32,
    lease: Arc<NativeImageLease>,
    protocol: Arc<EncodedProtocol>,
}

#[derive(Clone)]
struct PlacementState {
    id: u32,
    source: ImageSourceKey,
    geometry: native_patches::NativeRegion,
}

/// A successful host flush is the only operation that replaces this state.
#[derive(Clone, Default)]
pub(crate) struct HostGraphicsScene {
    sources: HashMap<ImageSourceKey, SourceState>,
    placements: HashMap<PlacementKey, PlacementState>,
    next_placement: u32,
    retired_sources: HashMap<u32, SourceState>,
    plans: HashMap<NodeId, native_patches::TerminalGraphicsPlan>,
}

pub(crate) struct SceneTransition {
    pub(crate) next: HostGraphicsScene,
    pub(crate) commands: Vec<String>,
    pub(crate) uploads: Vec<Arc<KittyUpload>>,
}

impl HostGraphicsScene {
    pub(crate) fn graphics_update<'a>(
        &self,
        updates: impl Iterator<Item = (NodeId, &'a [TerminalImagePlacement])>,
    ) -> Option<DesiredScene> {
        let mut desired = DesiredScene::default();
        for (owner, images) in updates {
            let plan = self.plans.get(&owner)?;
            let regions = plan.regions(images)?;
            desired.owners.insert(owner);
            desired.plans.insert(owner, plan.clone());
            for (source, regions) in images.iter().zip(regions) {
                for (region, geometry) in regions.into_iter().enumerate() {
                    desired.placements.push(DesiredPlacement {
                        key: PlacementKey {
                            owner,
                            logical: source.placement_key(),
                            region,
                        },
                        source: source.clone(),
                        geometry,
                    });
                }
            }
        }
        Some(desired)
    }

    pub(crate) fn native_plan_scope(&self) -> NativePlanScope {
        NativePlanScope(CACHED_PLANS.with(|slot| slot.replace(Some(self.plans.clone()))))
    }

    pub(crate) fn native_plan_owners(&self) -> HashSet<NodeId> {
        self.plans.keys().copied().collect()
    }

    pub(crate) fn terminal_visible(&self, owner: NodeId) -> bool {
        self.plans.get(&owner).is_some_and(|plan| plan.visible())
    }

    pub(crate) fn commit_retirements(&mut self) {
        self.retired_sources.clear();
    }

    pub(crate) fn prepare(
        &self,
        desired: DesiredScene,
        reserved: Option<&Self>,
    ) -> std::io::Result<SceneTransition> {
        let mut next = reserved.unwrap_or(self).clone();
        next.plans
            .retain(|owner, _| !(desired.full || desired.owners.contains(owner)));
        next.plans.extend(desired.plans.clone());
        let wanted: HashSet<_> = desired.placements.iter().map(|p| p.key).collect();
        let mut commands = Vec::new();
        next.placements.retain(|key, _| {
            !(desired.full || desired.owners.contains(&key.owner)) || wanted.contains(key)
        });
        let wanted_sources: HashSet<_> = desired
            .placements
            .iter()
            .map(|p| p.source.image.source_key())
            .chain(next.placements.values().map(|p| p.source))
            .collect();
        next.retain_sources(&wanted_sources);
        let mut uploads = Vec::new();
        let mut updated = HashSet::new();
        next.update_sources(
            &desired,
            reserved.is_some(),
            &mut updated,
            &mut commands,
            &mut uploads,
        )?;
        next.recover_sources(
            reserved.is_some(),
            &mut updated,
            &mut commands,
            &mut uploads,
        )?;
        for placement in desired.placements {
            next.place(placement, &updated, &mut commands)?;
        }
        next.refresh_preserved_placements(&wanted, &updated, &mut commands)?;
        next.delete_obsolete_placements(self, reserved, &mut commands);
        next.retire_obsolete_sources(self, reserved, &mut commands);
        Ok(SceneTransition {
            next,
            commands,
            uploads,
        })
    }

    fn retain_sources(&mut self, wanted: &HashSet<ImageSourceKey>) {
        self.sources.retain(|key, source| {
            if wanted.contains(key) {
                true
            } else {
                self.retired_sources.insert(source.id, source.clone());
                false
            }
        });
    }

    fn upload_source(
        &mut self,
        placement: &TerminalImagePlacement,
        retry: bool,
        commands: &mut Vec<String>,
        uploads: &mut Vec<Arc<KittyUpload>>,
    ) -> std::io::Result<()> {
        let source = self.resolve_source(placement, retry)?;
        let EncodedProtocol::CompressedKitty(image) = &*source.protocol else {
            return Err(std::io::Error::other("invalid native image protocol"));
        };
        if let Some(sequence) = image.upload.attempt_sequence()? {
            commands.push(sequence);
            uploads.push(Arc::clone(&image.upload));
        }
        self.sources.insert(placement.image.source_key(), source);
        Ok(())
    }

    fn update_sources(
        &mut self,
        desired: &DesiredScene,
        retry: bool,
        updated: &mut HashSet<ImageSourceKey>,
        commands: &mut Vec<String>,
        uploads: &mut Vec<Arc<KittyUpload>>,
    ) -> std::io::Result<()> {
        for placement in &desired.placements {
            let key = placement.source.image.source_key();
            if updated.contains(&key) {
                continue;
            }
            let revision = placement.source.image.source_revision();
            if !retry
                && self
                    .sources
                    .get(&key)
                    .is_some_and(|source| source.revision == revision)
            {
                continue;
            }
            self.upload_source(&placement.source, retry, commands, uploads)?;
            updated.insert(key);
        }
        Ok(())
    }

    fn recover_sources(
        &mut self,
        retry: bool,
        updated: &mut HashSet<ImageSourceKey>,
        commands: &mut Vec<String>,
        uploads: &mut Vec<Arc<KittyUpload>>,
    ) -> std::io::Result<()> {
        if !retry {
            return Ok(());
        }
        // Recover preserved panes too: the host may have consumed only part of a failed frame.
        let recovery: Vec<_> = self
            .sources
            .iter()
            .filter(|(key, _)| !updated.contains(key))
            .map(|(_, source)| source.source.clone())
            .collect();
        for placement in recovery {
            self.upload_source(&placement, true, commands, uploads)?;
            updated.insert(placement.image.source_key());
        }
        Ok(())
    }

    fn refresh_preserved_placements(
        &self,
        wanted: &HashSet<PlacementKey>,
        updated: &HashSet<ImageSourceKey>,
        commands: &mut Vec<String>,
    ) -> std::io::Result<()> {
        for (key, placement) in &self.placements {
            if updated.contains(&placement.source) && !wanted.contains(key) {
                commands.push(self.placement_command(placement)?);
            }
        }
        Ok(())
    }

    fn delete_obsolete_placements(
        &self,
        committed: &Self,
        reserved: Option<&Self>,
        commands: &mut Vec<String>,
    ) {
        let mut deleted = HashSet::new();
        let mut deletes = Vec::new();
        for previous in std::iter::once(committed).chain(reserved) {
            for (key, placement) in &previous.placements {
                if self.placements.contains_key(key) {
                    continue;
                }
                let id = previous.sources[&placement.source].id;
                if deleted.insert((id, placement.id)) {
                    deletes.push(format!(
                        "\x1b_Ga=d,q=2,d=i,i={id},p={};\x1b\\",
                        placement.id
                    ));
                }
            }
        }
        commands.splice(0..0, deletes);
    }

    fn retire_obsolete_sources(
        &mut self,
        committed: &Self,
        reserved: Option<&Self>,
        commands: &mut Vec<String>,
    ) {
        for previous in std::iter::once(committed).chain(reserved) {
            for source in previous.sources.values() {
                self.retired_sources.insert(source.id, source.clone());
            }
        }
        let referenced: HashSet<_> = self.placements.values().map(|p| p.source).collect();
        self.sources.retain(|key, source| {
            if !referenced.contains(key) {
                self.retired_sources.insert(source.id, source.clone());
                false
            } else {
                true
            }
        });
        for source in self.sources.values() {
            self.retired_sources.remove(&source.id);
        }
        for id in self.retired_sources.keys() {
            commands.push(format!("\x1b_Ga=d,q=2,d=I,i={id};\x1b\\"));
        }
    }

    fn resolve_source(
        &self,
        placement: &TerminalImagePlacement,
        retry: bool,
    ) -> std::io::Result<SourceState> {
        let key = placement.image.source_key();
        let revision = placement.image.source_revision();
        let previous = self.sources.get(&key);
        if let Some(previous) = previous
            && previous.revision == revision
            && (!retry
                || matches!(&*previous.protocol,
                EncodedProtocol::CompressedKitty(image) if image.transmission_pending()))
        {
            return Ok(previous.clone());
        }
        let lease = previous.map_or_else(
            || Arc::new(NativeImageLease::backend_owned()),
            |previous| Arc::new(previous.lease.next_revision()),
        );
        let id = lease
            .id()
            .ok_or_else(|| std::io::Error::other("native image id exhaustion"))?;
        let (protocol, _) = native_patches::encode_native_source(placement, id, Arc::clone(&lease))
            .ok_or_else(|| std::io::Error::other(NativeSourceUnavailable))?;
        Ok(SourceState {
            source: placement.clone(),
            revision: placement.image.source_revision(),
            id,
            lease,
            protocol,
        })
    }

    fn place(
        &mut self,
        desired: DesiredPlacement,
        updated: &HashSet<ImageSourceKey>,
        commands: &mut Vec<String>,
    ) -> std::io::Result<()> {
        let source = desired.source.image.source_key();
        let previous = self.placements.get(&desired.key);
        if previous.is_some_and(|p| p.source == source && p.geometry == desired.geometry)
            && !updated.contains(&source)
        {
            return Ok(());
        }
        let id = if let Some(previous) = previous {
            previous.id
        } else {
            self.next_placement = self
                .next_placement
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("native placement id exhaustion"))?;
            self.next_placement
        };
        let state = PlacementState {
            id,
            source,
            geometry: desired.geometry,
        };
        commands.push(self.placement_command(&state)?);
        self.placements.insert(desired.key, state);
        Ok(())
    }

    fn placement_command(&self, placement: &PlacementState) -> std::io::Result<String> {
        let EncodedProtocol::CompressedKitty(image) = &*self.sources[&placement.source].protocol
        else {
            return Err(std::io::Error::other("invalid native image protocol"));
        };
        let (x, y) = placement.geometry.anchor;
        Ok(format!(
            "\x1b[s\x1b[{};{}H{}\x1b[u",
            y + 1,
            x + 1,
            placement.geometry.command(image, placement.id)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::ratatui_backend::native_terminal::HostBackend;
    use ratatui::backend::Backend;
    use std::io;

    #[derive(Clone, Default)]
    struct Output {
        bytes: std::rc::Rc<RefCell<Vec<u8>>>,
        fail_flush: std::rc::Rc<Cell<bool>>,
    }

    impl io::Write for Output {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.bytes.borrow_mut().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_flush.replace(false) {
                Err(io::Error::other("injected flush failure"))
            } else {
                Ok(())
            }
        }
    }

    impl Output {
        fn take(&self) -> String {
            String::from_utf8(std::mem::take(&mut *self.bytes.borrow_mut())).unwrap()
        }
    }

    fn screen() -> crate::widgets::TerminalScreen {
        let mut screen = crate::widgets::TerminalScreen::new(4, 8, 0);
        screen.process_bytes(b"\x1b_Ga=T,i=7,p=9,f=24,s=1,v=1,q=2,C=1;YWJj\x1b\\");
        screen
    }

    fn stage_frame(screen: &mut crate::widgets::TerminalScreen) {
        begin(true);
        let _owner = owner_scope(NodeId::new(1, 1));
        let placement = &screen.render_snapshot().images[0];
        stage(
            placement,
            &[native_patches::NativeRegion {
                anchor: (0, 0),
                offset: (0, 0),
                z: 0,
                source: crate::widgets::TerminalImageCrop {
                    x: 0,
                    y: 0,
                    width: 1,
                    height: 1,
                },
            }],
        );
    }

    fn draw(host: &mut HostBackend<Output>) -> io::Result<()> {
        host.draw(std::iter::empty())?;
        Backend::flush(host)
    }

    fn id(wire: &str, key: &str) -> u32 {
        wire.split_once(key)
            .unwrap()
            .1
            .split([',', ';'])
            .next()
            .unwrap()
            .parse()
            .unwrap()
    }

    #[test]
    fn nested_capture_preserves_live_frame_staging() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        let _ = record_capture_images(|| {
            begin_kitty_frame();
            begin_partial_native_scene();
            let _owner = owner_scope(NodeId::new(2, 1));
        });
        draw(&mut host).unwrap();
        assert_eq!(output.take().matches("a=p").count(), 1);
        begin(false);
        let _ = record_capture_images(|| {
            let _owner = owner_scope(NodeId::new(1, 1));
        });
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn unchanged_and_unrelated_frames_emit_no_graphics() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let first = output.take();
        assert_eq!(first.matches("a=p").count(), 1);
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
        begin(false);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn replacements_reuse_source_and_placement_ids() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let first = output.take();
        screen.process_bytes(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\");
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let update = output.take();
        assert_eq!(id(&first, "i="), id(&update, "i="));
        assert_eq!(id(&first, "p="), id(&update, "p="));
        assert_eq!(update.matches("a=p").count(), 1);
        assert!(!update.contains("a=d"));
    }

    #[test]
    fn failed_flush_retries_without_committing_or_reallocating() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        output.fail_flush.set(true);
        assert!(draw(&mut host).is_err());
        let first = output.take();
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let retry = output.take();
        assert_eq!(id(&first, "i="), id(&retry, "i="));
        assert_eq!(id(&first, "p="), id(&retry, "p="));
        assert!(retry.contains("a=p"));
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn backend_lifetimes_upload_independently() {
        let mut screen = screen();
        let mut wires = Vec::new();
        for _ in 0..2 {
            let output = Output::default();
            let mut host = HostBackend::new(output.clone());
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            wires.push(output.take());
        }
        assert!(wires.iter().all(|wire| wire.contains("a=p")));
        assert_ne!(id(&wires[0], "i="), id(&wires[1], "i="));
    }

    #[test]
    fn composition_transition_retires_native_placements_and_sources() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        output.take();
        begin(true);
        draw(&mut host).unwrap();
        let removal = output.take();
        assert_eq!(removal.matches("a=d").count(), 2);
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert_eq!(output.take().matches("a=p").count(), 1);
        begin(false);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }
}
