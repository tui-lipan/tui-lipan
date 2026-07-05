use crate::core::element::{Element, Key};
use crate::core::memo::MemoCallSite;
use crate::utils::arena::ArenaId;
use rustc_hash::FxHashMap;
use std::any::{Any, TypeId};
use std::sync::Arc;

/// Internal component instance identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ComponentId {
    pub index: u32,
    pub generation: u32,
}

impl ComponentId {
    pub fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    pub fn index(self) -> usize {
        self.index as usize
    }
}

impl ArenaId for ComponentId {
    const INVALID: Self = Self {
        index: u32::MAX,
        generation: 0,
    };

    fn from_parts(index: u32, generation: u32) -> Self {
        Self::new(index, generation)
    }

    fn index(self) -> usize {
        self.index()
    }

    fn generation(self) -> u32 {
        self.generation
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ContainerTag {
    Root,
    VStack,
    HStack,
    Flow,
    Frame,
    Splitter,
    ScrollView,
    Grid,
    Canvas,
    Group,
    ZStack,
    Center,
    CenterPin,
    CenterPinSlot,
    StatusBarLayout,
    StatusBarLayoutSlot,
    Popover,
    Portal,
    ThemeProvider,
    ContextProvider,
    EffectScope,
    MouseRegion,
    DragSource,
    DropTarget,
    Animated,
    Memo,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MemoPathKey {
    pub(crate) path: ContainerPath,
    pub(crate) call_site: MemoCallSite,
}

#[derive(Clone)]
pub(crate) struct MemoCacheEntry {
    pub(crate) deps_hash: u64,
    pub(crate) expanded_child: Element,
    pub(crate) descendant_ids: Vec<ComponentId>,
}

#[derive(Clone)]
pub(crate) struct ContextProviderRecord {
    pub(crate) type_id: TypeId,
    pub(crate) value: Arc<dyn Any>,
    pub(crate) generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum SegmentId {
    Key(Key),
    Index(usize),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PathSegment {
    pub tag: ContainerTag,
    pub id: SegmentId,
}

pub fn segment_id(key: &Option<Key>, index: usize) -> SegmentId {
    match key {
        Some(k) => SegmentId::Key(k.clone()),
        None => SegmentId::Index(index),
    }
}

pub type ContainerPath = Vec<PathSegment>;

/// Per-component bookkeeping used for nested component reconciliation.
#[derive(Default)]
pub(crate) struct HostState {
    pub slots_prev: FxHashMap<ContainerPath, Vec<ComponentId>>,
    pub slots_next: FxHashMap<ContainerPath, Vec<ComponentId>>,
    pub contexts_prev: FxHashMap<ContainerPath, ContextProviderRecord>,
    pub contexts_next: FxHashMap<ContainerPath, ContextProviderRecord>,
    pub memos_prev: FxHashMap<MemoPathKey, MemoCacheEntry>,
    pub memos_next: FxHashMap<MemoPathKey, MemoCacheEntry>,
}

impl HostState {
    pub(crate) fn begin_render(&mut self) {
        self.slots_next.clear();
        self.contexts_next.clear();
        self.memos_next.clear();
    }

    pub(crate) fn finish_render(&mut self) {
        std::mem::swap(&mut self.slots_prev, &mut self.slots_next);
        self.slots_next.clear();
        std::mem::swap(&mut self.contexts_prev, &mut self.contexts_next);
        self.contexts_next.clear();
        std::mem::swap(&mut self.memos_prev, &mut self.memos_next);
        self.memos_next.clear();
    }

    pub fn prev_ids(&self, path: &ContainerPath) -> &[ComponentId] {
        self.slots_prev
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn set_next_ids(&mut self, path: &ContainerPath, ids: Vec<ComponentId>) {
        if let Some((owned_path, _)) = self.slots_prev.remove_entry(path) {
            self.slots_next.insert(owned_path, ids);
        } else {
            self.slots_next.insert(path.clone(), ids);
        }
    }

    pub fn prev_context(&self, path: &ContainerPath) -> Option<&ContextProviderRecord> {
        self.contexts_prev.get(path)
    }

    pub fn set_next_context(&mut self, path: &ContainerPath, record: ContextProviderRecord) {
        if let Some((owned_path, _)) = self.contexts_prev.remove_entry(path) {
            self.contexts_next.insert(owned_path, record);
        } else {
            self.contexts_next.insert(path.clone(), record);
        }
    }

    pub fn prev_memo(
        &self,
        path: &ContainerPath,
        call_site: MemoCallSite,
    ) -> Option<&MemoCacheEntry> {
        self.memos_prev.get(&MemoPathKey {
            path: path.clone(),
            call_site,
        })
    }

    pub fn set_next_memo(
        &mut self,
        path: &ContainerPath,
        call_site: MemoCallSite,
        entry: MemoCacheEntry,
    ) {
        let key = MemoPathKey {
            path: path.clone(),
            call_site,
        };
        if let Some((owned_key, _)) = self.memos_prev.remove_entry(&key) {
            self.memos_next.insert(owned_key, entry);
        } else {
            self.memos_next.insert(key, entry);
        }
    }
}
