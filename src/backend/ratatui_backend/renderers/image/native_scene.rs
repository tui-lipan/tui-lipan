//! Backend-owned, transactional native Kitty presentation state.
use std::rc::Rc;

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
    alive_owners: Option<HashSet<NodeId>>,
    owners: HashSet<NodeId>,
    placements: Vec<DesiredPlacement>,
    plans: HashMap<NodeId, native_patches::TerminalGraphicsPlan>,
}

thread_local! {
    // Only frame staging lives here. Committed resources belong to HostBackend.
    static DESIRED: RefCell<Option<DesiredScene>> = const { RefCell::new(None) };
    static CACHED_PLANS: RefCell<Option<HashMap<NodeId, native_patches::TerminalGraphicsPlan>>> = const { RefCell::new(None) };
    // A scoped snapshot of backend-confirmed owners, independent of cached plan availability.
    static COMMITTED_OWNERS: RefCell<HashSet<NodeId>> = RefCell::new(HashSet::new());
    static ALIVE_OWNERS: RefCell<Option<HashSet<NodeId>>> = const { RefCell::new(None) };
    static OWNER: Cell<NodeId> = const { Cell::new(NodeId::INVALID) };
}

pub(crate) struct NativePlanScope {
    plans: Option<HashMap<NodeId, native_patches::TerminalGraphicsPlan>>,
    owners: HashSet<NodeId>,
}

impl Drop for NativePlanScope {
    fn drop(&mut self) {
        CACHED_PLANS.with(|slot| {
            slot.replace(self.plans.take());
        });
        COMMITTED_OWNERS.with(|slot| slot.replace(std::mem::take(&mut self.owners)));
    }
}

pub(super) fn committed_owner() -> bool {
    COMMITTED_OWNERS.with(|slot| slot.borrow().contains(&OWNER.get()))
}

pub(super) fn partial_frame() -> bool {
    DESIRED.with(|slot| slot.borrow().as_ref().is_some_and(|scene| !scene.full))
}

/// A partial paint may only replace an owner's scene when it can describe all visible regions.
/// Otherwise leave the committed scene and its source revision intact until a complete paint.
pub(super) fn defer_owner() {
    DESIRED.with(|slot| {
        if let Some(scene) = slot.borrow_mut().as_mut() {
            let owner = OWNER.get();
            scene.owners.remove(&owner);
            scene.plans.remove(&owner);
            scene
                .placements
                .retain(|placement| placement.key.owner != owner);
        }
    });
}

pub(super) fn cached_plan() -> Option<native_patches::TerminalGraphicsPlan> {
    CACHED_PLANS.with(|slot| {
        slot.borrow()
            .as_ref()
            .and_then(|plans| plans.get(&OWNER.get()))
            .cloned()
    })
}

pub(crate) struct NativeOwnersScope(Option<HashSet<NodeId>>);

pub(crate) fn native_owners_scope(owners: HashSet<NodeId>) -> NativeOwnersScope {
    NativeOwnersScope(ALIVE_OWNERS.with(|slot| slot.replace(Some(owners))))
}

impl Drop for NativeOwnersScope {
    fn drop(&mut self) {
        ALIVE_OWNERS.with(|slot| slot.replace(self.0.take()));
    }
}

pub(crate) fn begin(full: bool) {
    DESIRED.with(|slot| {
        *slot.borrow_mut() = Some(DesiredScene {
            full,
            alive_owners: full
                .then(|| ALIVE_OWNERS.with(|slot| slot.borrow().clone()))
                .flatten(),
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
            scene.owners.insert(OWNER.get());
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
    revision: ImageSourceRevision,
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
    identities: Rc<RefCell<SceneIdentities>>,
}

#[derive(Default)]
struct SceneIdentities {
    sources: HashMap<ImageSourceKey, (std::sync::Weak<()>, Arc<NativeImageLease>)>,
    placements: HashMap<PlacementKey, (std::sync::Weak<()>, u32)>,
}

pub(crate) struct SceneTransition {
    pub(crate) next: HostGraphicsScene,
    pub(crate) commands: Vec<String>,
    pub(crate) uploads: Vec<Arc<KittyUpload>>,
    pub(crate) metrics: crate::widgets::TerminalGraphicsMetrics,
}

impl SceneTransition {
    pub(crate) fn continue_with(&self, desired: DesiredScene) -> std::io::Result<Self> {
        let mut base = self.next.clone();
        // These deletions were already written in this output batch. Keep their leases in the
        // resulting transition, but do not emit them again for each repainted terminal row.
        base.commit_retirements();
        let mut next = base.prepare(desired, None)?;
        next.next
            .retired_sources
            .extend(self.next.retired_sources.clone());
        next.uploads.extend(self.uploads.iter().cloned());
        next.metrics.sources_created += self.metrics.sources_created;
        next.metrics.sources_uploaded += self.metrics.sources_uploaded;
        next.metrics.sources_unchanged += self.metrics.sources_unchanged;
        next.metrics.sources_updated += self.metrics.sources_updated;
        next.metrics.sources_retired += self.metrics.sources_retired;
        next.metrics.placements_desired += self.metrics.placements_desired;
        next.metrics.placements_created += self.metrics.placements_created;
        next.metrics.placements_unchanged += self.metrics.placements_unchanged;
        next.metrics.placements_updated += self.metrics.placements_updated;
        next.metrics.placements_deleted += self.metrics.placements_deleted;
        next.metrics.upload_bytes += self.metrics.upload_bytes;
        next.metrics.placement_bytes += self.metrics.placement_bytes;
        next.metrics.delete_bytes += self.metrics.delete_bytes;
        Ok(next)
    }

    pub(crate) fn record_metrics(&self) {
        crate::core::terminal_metrics::update(|metrics| {
            metrics.sources_created += self.metrics.sources_created;
            metrics.sources_uploaded += self.metrics.sources_uploaded;
            metrics.sources_unchanged += self.metrics.sources_unchanged;
            metrics.sources_updated += self.metrics.sources_updated;
            metrics.sources_retired += self.metrics.sources_retired;
            metrics.placements_desired += self.metrics.placements_desired;
            metrics.placements_created += self.metrics.placements_created;
            metrics.placements_unchanged += self.metrics.placements_unchanged;
            metrics.placements_updated += self.metrics.placements_updated;
            metrics.placements_deleted += self.metrics.placements_deleted;
            metrics.upload_bytes += self.metrics.upload_bytes;
            metrics.placement_bytes += self.metrics.placement_bytes;
            metrics.delete_bytes += self.metrics.delete_bytes;
        });
    }
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
        NativePlanScope {
            plans: CACHED_PLANS.with(|slot| slot.replace(Some(self.plans.clone()))),
            owners: COMMITTED_OWNERS.with(|slot| slot.replace(self.native_plan_owners())),
        }
    }

    pub(crate) fn native_plan_owners(&self) -> HashSet<NodeId> {
        self.plans.keys().copied().collect()
    }

    pub(crate) fn terminal_visible(&self, owner: NodeId) -> bool {
        self.plans.get(&owner).is_some_and(|plan| plan.visible())
    }

    pub(crate) fn invalidate(&mut self) {
        self.plans.clear();
        for source in self.sources.values_mut() {
            source.revision = ImageSourceRevision::default();
        }
    }

    fn cleanup_ids(&self, pending: Option<&Self>) -> std::collections::BTreeSet<u32> {
        let mut ids = std::collections::BTreeSet::new();
        for scene in std::iter::once(self).chain(pending) {
            ids.extend(scene.sources.values().map(|source| source.id));
            ids.extend(scene.retired_sources.keys().copied());
        }
        ids
    }

    pub(crate) fn cleanup_commands(&self, pending: Option<&Self>) -> String {
        self.cleanup_ids(pending)
            .into_iter()
            .map(|id| format!("\x1b_Ga=d,q=2,d=I,i={id};\x1b\\"))
            .collect()
    }

    pub(crate) fn defer_cleanup(&self, pending: Option<&Self>) {
        native_patches::defer_backend_cleanup(self.cleanup_ids(pending));
    }

    pub(crate) fn commit_retirements(&mut self) {
        self.retired_sources.clear();
    }

    pub(crate) fn prepare(
        &self,
        mut desired: DesiredScene,
        reserved: Option<&Self>,
    ) -> std::io::Result<SceneTransition> {
        {
            let mut identities = self.identities.borrow_mut();
            identities
                .sources
                .retain(|_, (owner, _)| owner.strong_count() != 0);
            identities.placements.retain(|key, (owner, _)| {
                owner.strong_count() != 0
                    && desired
                        .alive_owners
                        .as_ref()
                        .is_none_or(|owners| owners.contains(&key.owner))
            });
        }
        self.match_regions(&mut desired, reserved);
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
        next.validate_revisions(&desired, &wanted)?;
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
        let metrics =
            self.transition_metrics(&next, &wanted, &wanted_sources, &commands, uploads.len());
        Ok(SceneTransition {
            metrics,
            next,
            commands,
            uploads,
        })
    }

    fn validate_revisions(
        &self,
        desired: &DesiredScene,
        wanted: &HashSet<PlacementKey>,
    ) -> std::io::Result<()> {
        let mut revisions = HashMap::new();
        let desired_versions = desired.placements.iter().map(|placement| {
            (
                placement.source.image.source_key(),
                placement.source.image.source_revision(),
            )
        });
        let preserved_versions = self
            .placements
            .iter()
            .filter(|(key, _)| !wanted.contains(key))
            .map(|(_, placement)| (placement.source, placement.revision));
        for (key, revision) in desired_versions.chain(preserved_versions) {
            if revisions
                .insert(key, revision)
                .is_some_and(|previous| previous != revision)
            {
                // Frozen snapshots may coexist with a live screen at a newer source revision.
                // A single Kitty image ID cannot display both; retained composition can.
                return Err(std::io::Error::other(NativeSourceUnavailable));
            }
        }
        Ok(())
    }

    fn transition_metrics(
        &self,
        next: &Self,
        wanted: &HashSet<PlacementKey>,
        sources: &HashSet<ImageSourceKey>,
        commands: &[String],
        uploads: usize,
    ) -> crate::widgets::TerminalGraphicsMetrics {
        let mut metrics = crate::widgets::TerminalGraphicsMetrics {
            placements_desired: wanted.len() as u64,
            sources_uploaded: uploads as u64,
            ..Default::default()
        };
        for key in sources {
            let Some(source) = next.sources.get(key) else {
                continue;
            };
            match self.sources.get(key) {
                None => metrics.sources_created += 1,
                Some(previous) if previous.revision != source.revision => {
                    metrics.sources_updated += 1
                }
                Some(_) => metrics.sources_unchanged += 1,
            }
        }
        metrics.sources_retired = self
            .sources
            .keys()
            .filter(|key| !next.sources.contains_key(key))
            .count() as u64;
        metrics.placements_created = wanted
            .iter()
            .filter(|key| !self.placements.contains_key(key))
            .count() as u64;
        for command in commands {
            if command.contains("\x1b_Ga=p,") {
                metrics.placement_bytes += command.len() as u64;
                metrics.placements_updated += 1;
            } else if command.starts_with("\x1b_Ga=d,") {
                metrics.delete_bytes += command.len() as u64;
                if command.contains(",p=") {
                    metrics.placements_deleted += 1;
                }
            } else {
                metrics.upload_bytes += command.len() as u64;
            }
        }
        metrics.placements_updated = metrics
            .placements_updated
            .saturating_sub(metrics.placements_created);
        metrics.placements_unchanged = metrics
            .placements_desired
            .saturating_sub(metrics.placements_created + metrics.placements_updated);
        metrics
    }

    fn match_regions(&self, desired: &mut DesiredScene, reserved: Option<&Self>) {
        let mut geometry = HashMap::new();
        let mut free: HashMap<(NodeId, ImagePlacementKey), std::collections::BTreeSet<usize>> =
            HashMap::new();
        for previous in std::iter::once(reserved)
            .flatten()
            .chain(std::iter::once(self))
        {
            for (key, state) in &previous.placements {
                geometry.insert((key.owner, key.logical, state.geometry), key.region);
                free.entry((key.owner, key.logical))
                    .or_default()
                    .insert(key.region);
            }
        }
        let mut matched = HashSet::new();
        let mut used: HashMap<(NodeId, ImagePlacementKey), HashSet<usize>> = HashMap::new();
        for (index, placement) in desired.placements.iter_mut().enumerate() {
            let group = (placement.key.owner, placement.key.logical);
            if let Some(&region) = geometry.get(&(group.0, group.1, placement.geometry))
                && used.entry(group).or_default().insert(region)
            {
                placement.key.region = region;
                free.entry(group).or_default().remove(&region);
                matched.insert(index);
            }
        }
        for (index, placement) in desired.placements.iter_mut().enumerate() {
            if matched.contains(&index) {
                continue;
            }
            let group = (placement.key.owner, placement.key.logical);
            let regions = used.entry(group).or_default();
            let region = free
                .entry(group)
                .or_default()
                .pop_first()
                .unwrap_or_else(|| (0..).find(|region| !regions.contains(region)).unwrap_or(0));
            regions.insert(region);
            placement.key.region = region;
        }
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
        if let Some(sequence) = image.upload.attempt_native_sequence()? {
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
            || {
                let mut identities = self.identities.borrow_mut();
                let (_, lease) = identities.sources.entry(key).or_insert_with(|| {
                    (
                        placement.image.source_lifetime(),
                        Arc::new(NativeImageLease::backend_owned()),
                    )
                });
                Arc::new(lease.next_revision())
            },
            |previous| Arc::new(previous.lease.next_revision()),
        );
        let id = lease
            .id()
            .ok_or_else(|| std::io::Error::other("native image id exhaustion"))?;
        let (protocol, _) = native_patches::encode_native_source(placement, id, Arc::clone(&lease))
            .ok_or_else(|| std::io::Error::other(NativeSourceUnavailable))?;
        let mut source = placement.clone();
        source.image.detach_logical_lifetimes();
        Ok(SourceState {
            source,
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
        } else if let Some((_, id)) = self.identities.borrow().placements.get(&desired.key) {
            *id
        } else {
            self.next_placement = self
                .next_placement
                .checked_add(1)
                .ok_or_else(|| std::io::Error::other("native placement id exhaustion"))?;
            self.next_placement
        };
        if let Some(owner) = desired.source.image.placement_lifetime() {
            self.identities
                .borrow_mut()
                .placements
                .insert(desired.key, (owner, id));
        }
        let state = PlacementState {
            id,
            source,
            revision: desired.source.image.source_revision(),
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
    fn partial_repaint_upload_restores_the_complete_image() {
        let _host = image_support::test_kitty_host((10, 20), true);
        let mut screen = crate::widgets::TerminalScreen::new(4, 8, 0);
        screen.set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        let mut frame = ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 4)).unwrap();
        let content = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 4,
        };
        let paint = |screen: &mut crate::widgets::TerminalScreen,
                     frame: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
                     clip| {
            let _owner = owner_scope(NodeId::new(1, 1));
            frame
                .draw(|f| {
                    assert!(render_native_terminal_images(
                        f,
                        &screen.render_snapshot().images,
                        content,
                        clip
                    ))
                })
                .unwrap();
        };
        let upload = |screen: &mut crate::widgets::TerminalScreen, color: u8| {
            let payload = BASE64.encode([color, 0, 0].repeat(20 * 60));
            screen.process_bytes(
                format!("\x1b[H\x1b_Ga=T,i=7,p=9,f=24,s=20,v=60,q=2,C=1;{payload}\x1b\\")
                    .as_bytes(),
            );
        };
        upload(&mut screen, 1);
        begin_kitty_frame();
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 0, 8, 4),
        );
        draw(&mut host).unwrap();
        let initial = output.take();
        assert!(initial.contains("w=20,h=60"));

        // A new producer frame is pending when a text-only update repaints one physical row.
        upload(&mut screen, 2);
        begin_partial_image_scene(&[]);
        let _plans = host.native_plan_scope();
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 1, 8, 1),
        );
        draw(&mut host).unwrap();
        let update = output.take();
        assert_eq!(update.matches("a=t,").count(), 1);
        assert_eq!(update.matches("a=p,").count(), 1);
        assert!(
            update.contains("w=20,h=60"),
            "re-upload deletes the old placement, so the replacement must cover all three rows: {update:?}"
        );

        begin_partial_image_scene(&[]);
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 2, 8, 1),
        );
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn partial_repaint_upload_with_changed_holes_restores_the_complete_image() {
        let _host = image_support::test_kitty_host((10, 20), true);
        let mut screen = crate::widgets::TerminalScreen::new(4, 8, 0);
        screen.set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        let mut frame = ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 4)).unwrap();
        let content = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 4,
        };
        let paint = |screen: &mut crate::widgets::TerminalScreen,
                     frame: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
                     clip| {
            let _owner = owner_scope(NodeId::new(1, 1));
            frame
                .draw(|f| {
                    assert!(render_native_terminal_images(
                        f,
                        &screen.render_snapshot().images,
                        content,
                        clip
                    ))
                })
                .unwrap();
        };
        let upload = |screen: &mut crate::widgets::TerminalScreen, color: u8| {
            let payload = BASE64.encode([color, 0, 0].repeat(20 * 60));
            screen.process_bytes(
                format!("\x1b[H\x1b_Ga=T,i=7,p=9,f=24,s=20,v=60,q=2,C=1;{payload}\x1b\\")
                    .as_bytes(),
            );
        };
        upload(&mut screen, 1);
        begin_kitty_frame();
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 0, 8, 4),
        );
        draw(&mut host).unwrap();
        let initial = output.take();
        assert!(initial.contains("w=20,h=60"));

        // A new producer frame is pending when a text-only update repaints one physical row.
        upload(&mut screen, 2);
        // An unrelated overlay outside the image changes the occlusion set.
        crate::backend::ratatui_backend::renderers::image::set_image_occlusions(vec![
            ratatui::layout::Rect::new(6, 3, 2, 1),
        ]);
        begin_partial_image_scene(&[]);
        let _plans = host.native_plan_scope();
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 1, 8, 1),
        );
        draw(&mut host).unwrap();
        let update = output.take();
        assert_eq!(update.matches("a=t,").count(), 1);
        assert_eq!(update.matches("a=p,").count(), 1);
        assert!(
            update.contains("w=20,h=60"),
            "re-upload deletes the old placement, so the replacement must cover all three rows: {update:?}"
        );

        // An overlay that intersects the image must update occlusion across its full height.
        upload(&mut screen, 3);
        set_image_occlusions(vec![ratatui::layout::Rect::new(0, 1, 1, 1)]);
        begin_partial_image_scene(&[]);
        paint(
            &mut screen,
            &mut frame,
            ratatui::layout::Rect::new(0, 1, 8, 1),
        );
        draw(&mut host).unwrap();
        let occluded = output.take();
        assert_eq!(occluded.matches("a=t,").count(), 1);
        assert_eq!(occluded.matches("a=p,").count(), 3);
        assert!(occluded.contains("x=0,y=0,w=20,h=20"));
        assert!(occluded.contains("x=10,y=20,w=10,h=20"));
        assert!(occluded.contains("x=0,y=40,w=20,h=20"));

        crate::backend::ratatui_backend::renderers::image::clear_image_occlusions();
    }

    #[test]
    fn partial_repaint_without_complete_bounds_or_native_support_defers_the_upload() {
        for case in [
            "missing plan",
            "moved content",
            "unsupported layers",
            "unmeasured cells",
            "changed cell size",
        ] {
            let _host = image_support::test_kitty_host((10, 20), true);
            let mut screen = crate::widgets::TerminalScreen::new(4, 8, 0);
            screen.set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
            let output = Output::default();
            let mut host = HostBackend::new(output.clone());
            let mut frame =
                ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 4)).unwrap();
            let content = Rect {
                x: 0,
                y: 0,
                w: 8,
                h: 4,
            };
            let clip = ratatui::layout::Rect::new(0, 0, 8, 4);
            let paint = |screen: &mut crate::widgets::TerminalScreen,
                         frame: &mut ratatui::Terminal<ratatui::backend::TestBackend>,
                         content,
                         clip| {
                let _owner = owner_scope(NodeId::new(1, 1));
                frame
                    .draw(|f| {
                        assert!(render_native_terminal_images(
                            f,
                            &screen.render_snapshot().images,
                            content,
                            clip
                        ))
                    })
                    .unwrap();
            };
            let upload = |screen: &mut crate::widgets::TerminalScreen, color: u8| {
                let payload = BASE64.encode([color, 0, 0].repeat(20 * 60));
                screen.process_bytes(
                    format!("\x1b[H\x1b_Ga=T,i=7,p=9,f=24,s=20,v=60,q=2,C=1;{payload}\x1b\\")
                        .as_bytes(),
                );
            };
            upload(&mut screen, 1);
            begin_kitty_frame();
            paint(&mut screen, &mut frame, content, clip);
            draw(&mut host).unwrap();
            assert!(output.take().contains("w=20,h=60"));

            upload(&mut screen, 2);
            begin_partial_image_scene(&[]);
            {
                let _plans = host.native_plan_scope();
                if case == "missing plan" {
                    CACHED_PLANS.with(|slot| slot.borrow_mut().as_mut().unwrap().clear());
                }
                let _layers = native_layer_scope(case != "unsupported layers");
                let cell = if case == "changed cell size" {
                    (12, 24)
                } else {
                    (10, 20)
                };
                let _cell = image_support::test_kitty_host(cell, case != "unmeasured cells");
                let content = if case == "moved content" {
                    Rect { x: 1, ..content }
                } else {
                    content
                };
                paint(
                    &mut screen,
                    &mut frame,
                    content,
                    ratatui::layout::Rect::new(0, 1, 8, 1),
                );
                draw(&mut host).unwrap();
                assert!(
                    !output.take().contains("\x1b_G"),
                    "{case}: a partial plan must not upload or remove the committed image"
                );
            }
            // The new revision stays pending and is presented in full by the next ordinary paint.
            begin_kitty_frame();
            paint(&mut screen, &mut frame, content, clip);
            draw(&mut host).unwrap();
            let complete = output.take();
            assert_eq!(
                complete.matches("a=t,").count(),
                1,
                "{case}: upload must remain pending"
            );
            assert!(
                complete.contains("w=20,h=60"),
                "{case}: full image must be restored"
            );
        }
    }

    #[test]
    fn partial_paint_without_native_support_falls_back_to_composition() {
        // A terminal whose steady state is composed (native layers unavailable) must keep using
        // the composed fallback in partial frames, or its image cells are never drawn there.
        let _host = image_support::test_kitty_host((10, 20), true);
        let mut screen = crate::widgets::TerminalScreen::new(4, 8, 0);
        screen.set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        let payload = BASE64.encode([1u8, 0, 0].repeat(20 * 60));
        screen.process_bytes(
            format!("\x1b[H\x1b_Ga=T,i=7,p=9,f=24,s=20,v=60,q=2,C=1;{payload}\x1b\\").as_bytes(),
        );
        let mut frame = ratatui::Terminal::new(ratatui::backend::TestBackend::new(8, 4)).unwrap();
        let content = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 4,
        };
        let mut native = |owner, clip| {
            let _owner = owner_scope(owner);
            let mut handled = false;
            frame
                .draw(|f| {
                    handled = render_native_terminal_images(
                        f,
                        &screen.render_snapshot().images,
                        content,
                        clip,
                    )
                })
                .unwrap();
            handled
        };
        // Another terminal uses native rendering on this backend. Its committed ownership
        // must not cause this composed terminal to defer.
        let output = Output::default();
        let mut host = HostBackend::new(output);
        begin_kitty_frame();
        assert!(native(
            NodeId::new(2, 1),
            ratatui::layout::Rect::new(0, 0, 8, 4)
        ));
        draw(&mut host).unwrap();
        let _plans = host.native_plan_scope();
        let _layers = native_layer_scope(false);
        begin_kitty_frame();
        assert!(
            !native(NodeId::new(1, 1), ratatui::layout::Rect::new(0, 0, 8, 4)),
            "full paint composes"
        );
        begin_partial_image_scene(&[]);
        assert!(
            !native(NodeId::new(1, 1), ratatui::layout::Rect::new(0, 1, 8, 1)),
            "partial paint claimed a composed terminal, skipping its composed fallback"
        );
    }

    #[test]
    fn nested_capture_preserves_live_frame_staging() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        let _ = record_capture_images(|| {
            begin_kitty_frame();
            begin_partial_image_scene(&[]);
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
        assert!(first.contains("a=t,"));
        assert!(!first.contains("U=1"));
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
        begin(false);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn source_only_retransmission_removes_host_placements_until_explicitly_placed() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let first = output.take();
        screen.process_bytes(b"\x1b_Ga=t,i=7,f=24,s=1,v=1,q=2;ZGVm\x1b\\");
        assert!(screen.render_snapshot().images.is_empty());
        begin(true);
        {
            let _owner = owner_scope(NodeId::new(1, 1));
        }
        draw(&mut host).unwrap();
        let retired = output.take();
        assert!(retired.contains("a=d"));
        assert!(!retired.contains("a=p"));
        screen.process_bytes(b"\x1b_Ga=p,i=7,p=9,C=1,q=2;\x1b\\");
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let placed = output.take();
        assert_eq!(id(&first, "i="), id(&placed, "i="));
        assert_eq!(placed.matches("a=p").count(), 1);
    }

    #[test]
    fn replacements_reuse_source_ids_and_retire_old_placements() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let first = output.take();
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\");
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let update = output.take();
        assert_eq!(id(&first, "i="), id(&update, "i="));
        assert_ne!(
            id(&first, "p="),
            id(update.split_once("a=p,").unwrap().1, "p=")
        );
        assert_eq!(update.matches("a=p").count(), 1);
        assert!(update.contains("a=d"));
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
        let initial = output.take();
        begin(true);
        draw(&mut host).unwrap();
        let removal = output.take();
        assert_eq!(removal.matches("a=d").count(), 2);
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let restored = output.take();
        assert_eq!(restored.matches("a=p").count(), 1);
        assert_eq!(id(&initial, "i="), id(&restored, "i="));
        assert_eq!(id(&initial, "p="), id(&restored, "p="));
        begin(false);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }
    #[test]
    fn late_frame_failure_and_abandonment_do_not_commit_graphics() {
        for failure in [false, true] {
            let mut screen = screen();
            let output = Output::default();
            let mut host = HostBackend::new(output.clone());
            let frame = host.begin_output_frame().unwrap();
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            let first = output.take();
            if failure {
                output.fail_flush.set(true);
                assert!(host.finish_output_frame(frame).is_err());
            } else {
                drop(frame);
            }
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            let retry = output.take();
            assert!(retry.contains("a=t"));
            assert_eq!(id(&first, "i="), id(&retry, "i="));
            assert_eq!(id(&first, "p="), id(&retry, "p="));
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            assert!(!output.take().contains("\x1b_G"));
        }
    }

    #[test]
    fn backend_retry_pixels_do_not_keep_logical_source_reservations_alive() {
        let mut screen = screen();
        stage_frame(&mut screen);
        let scene = HostGraphicsScene::default();
        let mut committed = scene.prepare(take_desired().unwrap(), None).unwrap().next;
        committed.commit_retirements();
        assert_eq!(committed.identities.borrow().sources.len(), 1);
        assert_eq!(committed.identities.borrow().placements.len(), 1);
        drop(screen);
        let next = committed
            .prepare(
                DesiredScene {
                    full: true,
                    ..Default::default()
                },
                None,
            )
            .unwrap()
            .next;
        assert!(next.identities.borrow().sources.is_empty());
        assert!(next.identities.borrow().placements.is_empty());
    }

    #[test]
    fn inserting_a_region_does_not_renumber_unchanged_regions() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        let placement = screen.render_snapshot().images[0].clone();
        let region = |row| native_patches::NativeRegion {
            anchor: (0, row),
            offset: (0, 0),
            z: 0,
            source: crate::widgets::TerminalImageCrop {
                x: 0,
                y: 0,
                width: 1,
                height: 1,
            },
        };
        for rows in [vec![0, 2], vec![0, 1, 2]] {
            begin(true);
            let _owner = owner_scope(NodeId::new(1, 1));
            stage(
                &placement,
                &rows.iter().map(|row| region(*row)).collect::<Vec<_>>(),
            );
            draw(&mut host).unwrap();
            let commands = output.take();
            assert_eq!(
                commands.matches("a=p").count(),
                if rows.len() == 2 { 2 } else { 1 }
            );
            assert!(!commands.contains("a=d"));
        }
    }
    #[test]
    fn operation_counters_commit_with_the_scene_and_not_intermediate_flushes() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        let before = crate::widgets::terminal_graphics_metrics();
        let frame = host.begin_output_frame().unwrap();
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let intermediate = crate::widgets::terminal_graphics_metrics();
        assert_eq!(intermediate.sources_uploaded, before.sources_uploaded);
        assert_eq!(intermediate.frames_written, before.frames_written);
        host.finish_output_frame(frame).unwrap();
        let committed = crate::widgets::terminal_graphics_metrics();
        assert_eq!(committed.sources_uploaded, before.sources_uploaded + 1);
        assert_eq!(committed.placements_created, before.placements_created + 1);
        assert_eq!(committed.frames_written, before.frames_written + 1);
        assert!(committed.upload_bytes > before.upload_bytes);
        let frame = host.begin_output_frame().unwrap();
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\");
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        drop(frame);
        let abandoned = crate::widgets::terminal_graphics_metrics();
        assert_eq!(abandoned.sources_uploaded, committed.sources_uploaded);
        assert_eq!(abandoned.frames_written, committed.frames_written);
        assert_eq!(abandoned.frames_failed, committed.frames_failed + 1);
    }
    #[test]
    fn title_updates_commit_after_the_complete_frame_and_retry_after_failure() {
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        let frame = host.begin_output_frame().unwrap();
        host.set_title("scene").unwrap();
        draw(&mut host).unwrap();
        host.finish_output_frame(frame).unwrap();
        assert_eq!(output.take().matches("\x1b]2;scene").count(), 1);
        let frame = host.begin_output_frame().unwrap();
        host.set_title("scene").unwrap();
        host.finish_output_frame(frame).unwrap();
        assert!(!output.take().contains("\x1b]2;"));
        let frame = host.begin_output_frame().unwrap();
        host.set_title("replacement").unwrap();
        draw(&mut host).unwrap();
        output.fail_flush.set(true);
        assert!(host.finish_output_frame(frame).is_err());
        output.take();
        let frame = host.begin_output_frame().unwrap();
        host.set_title("replacement").unwrap();
        host.finish_output_frame(frame).unwrap();
        assert!(output.take().contains("\x1b]2;replacement"));
    }

    #[test]
    fn screen_clear_reuploads_unchanged_native_sources_with_reserved_ids() {
        for region in [
            None,
            Some(ratatui::backend::ClearType::All),
            Some(ratatui::backend::ClearType::AfterCursor),
        ] {
            let mut screen = screen();
            let output = Output::default();
            let mut host = HostBackend::new(output.clone());
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            let initial = output.take();

            if let Some(region) = region {
                host.clear_region(region).unwrap();
            } else {
                host.clear().unwrap();
            }
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            let restored = output.take();
            assert!(
                restored.contains("a=t"),
                "a clear can erase the host image data"
            );
            assert_eq!(id(&initial, "i="), id(&restored, "i="));
            assert_eq!(id(&initial, "p="), id(&restored, "p="));

            stage_frame(&mut screen);
            draw(&mut host).unwrap();
            assert!(!output.take().contains("\x1b_G"));
        }
    }

    #[test]
    fn host_recovery_restores_sources_with_reserved_ids_and_backend_drop_cleans_them() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let initial = output.take();
        host.invalidate_host_state();
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let restored = output.take();
        assert!(restored.contains("a=t"));
        assert_eq!(id(&initial, "i="), id(&restored, "i="));
        assert_eq!(id(&initial, "p="), id(&restored, "p="));
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
        drop(host);
        let cleanup = output.take();
        assert!(cleanup.contains("a=d,q=2,d=I"));
        assert_eq!(id(&initial, "i="), id(&cleanup, "i="));
    }
    #[test]
    fn multiple_row_draws_do_not_reupload_an_unrelated_native_source() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        output.take();
        let frame = host.begin_output_frame().unwrap();
        for _ in 0..3 {
            begin(false);
            let _owner = owner_scope(NodeId::new(2, 1));
            draw(&mut host).unwrap();
        }
        host.finish_output_frame(frame).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn repeated_draws_in_one_frame_commit_one_upload_and_keep_its_handoff() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        output.take();
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\");
        let before = crate::terminal_graphics_metrics();
        let frame = host.begin_output_frame().unwrap();
        for _ in 0..3 {
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
        }
        host.finish_output_frame(frame).unwrap();
        let commands = output.take();
        assert_eq!(commands.matches("a=t").count(), 1);
        assert_eq!(commands.matches("a=p").count(), 1);
        assert_eq!(
            crate::terminal_graphics_metrics().sources_uploaded,
            before.sources_uploaded + 1
        );
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        assert!(!output.take().contains("\x1b_G"));
    }

    #[test]
    fn simultaneous_frozen_and_live_source_revisions_require_composition() {
        let mut screen = screen();
        stage_frame(&mut screen);
        let mut desired = take_desired().unwrap();
        let mut frozen = desired.placements[0].clone();
        frozen.key.owner = NodeId::new(2, 1);
        desired.owners.insert(frozen.key.owner);
        desired.placements.push(frozen.clone());
        let committed = HostGraphicsScene::default()
            .prepare(desired, None)
            .unwrap()
            .next;
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\");
        stage_frame(&mut screen);
        let mut changed = take_desired().unwrap();
        changed.full = false;
        match committed.prepare(changed, None) {
            Ok(_) => panic!("different retained source revisions cannot share a Kitty source"),
            Err(error) => assert!(native_source_unavailable(&error)),
        }
    }

    #[test]
    fn placement_reservations_survive_clipping_but_not_owner_removal() {
        let mut screen = screen();
        stage_frame(&mut screen);
        let committed = HostGraphicsScene::default()
            .prepare(take_desired().unwrap(), None)
            .unwrap()
            .next;
        let owner = NodeId::new(1, 1);
        let clipped = DesiredScene {
            full: true,
            alive_owners: Some(HashSet::from([owner])),
            ..Default::default()
        };
        let hidden = committed.prepare(clipped, None).unwrap().next;
        assert_eq!(hidden.identities.borrow().placements.len(), 1);
        let removed = DesiredScene {
            full: true,
            alive_owners: Some(HashSet::new()),
            ..Default::default()
        };
        let detached = hidden.prepare(removed, None).unwrap().next;
        assert!(detached.identities.borrow().placements.is_empty());
        assert_eq!(detached.identities.borrow().sources.len(), 1);
    }

    #[test]
    fn failed_backend_cleanup_is_available_to_the_next_backend() {
        super::native_patches::take_retired_native_ids();
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let initial = output.take();
        output.fail_flush.set(true);
        drop(host);
        let deferred = super::native_patches::take_retired_native_ids();
        assert!(deferred.contains(&id(&initial, "i=")));
    }

    #[test]
    fn failed_multi_draw_frame_retries_one_upload_with_the_same_ids() {
        let mut screen = screen();
        let output = Output::default();
        let mut host = HostBackend::new(output.clone());
        stage_frame(&mut screen);
        draw(&mut host).unwrap();
        let initial = output.take();
        screen.process_bytes(b"\x1b_Ga=T,i=7,f=24,s=1,v=1,C=1,q=2;ZGVm\x1b\\");
        let frame = host.begin_output_frame().unwrap();
        for _ in 0..3 {
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
        }
        output.fail_flush.set(true);
        assert!(host.finish_output_frame(frame).is_err());
        output.take();
        let frame = host.begin_output_frame().unwrap();
        for _ in 0..3 {
            stage_frame(&mut screen);
            draw(&mut host).unwrap();
        }
        host.finish_output_frame(frame).unwrap();
        let retried = output.take();
        assert_eq!(retried.matches("a=t").count(), 1);
        assert_eq!(retried.matches("a=p").count(), 1);
        assert_eq!(id(&initial, "i="), id(&retried, "i="));
        assert_eq!(id(&initial, "p="), id(&retried, "p="));
    }
}
