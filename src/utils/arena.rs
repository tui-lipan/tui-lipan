pub(crate) trait ArenaId: Copy + Eq {
    const INVALID: Self;

    fn from_parts(index: u32, generation: u32) -> Self;
    fn index(self) -> usize;
    fn generation(self) -> u32;

    fn is_invalid(self) -> bool {
        self == Self::INVALID
    }
}

#[derive(Clone)]
struct ArenaSlot<T> {
    generation: u32,
    active: bool,
    value: T,
}

#[derive(Clone)]
pub(crate) struct Arena<T, I: ArenaId> {
    slots: Vec<ArenaSlot<T>>,
    free: Vec<u32>,
    _marker: std::marker::PhantomData<I>,
}

impl<T, I: ArenaId> Default for Arena<T, I> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T, I: ArenaId> Arena<T, I> {
    pub(crate) fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            _marker: std::marker::PhantomData,
        }
    }

    pub(crate) fn is_valid(&self, id: I) -> bool {
        if id.is_invalid() {
            return false;
        }
        let Some(slot) = self.slots.get(id.index()) else {
            return false;
        };
        slot.active && slot.generation == id.generation()
    }

    pub(crate) fn get(&self, id: I) -> &T {
        debug_assert!(self.is_valid(id), "invalid arena id");
        &self.slots[id.index()].value
    }

    pub(crate) fn get_mut(&mut self, id: I) -> &mut T {
        debug_assert!(self.is_valid(id), "invalid arena id");
        &mut self.slots[id.index()].value
    }

    #[cfg(test)]
    pub(crate) fn iter_active(&self) -> impl Iterator<Item = &T> {
        self.slots
            .iter()
            .filter(|slot| slot.active)
            .map(|slot| &slot.value)
    }

    #[cfg(test)]
    pub(crate) fn free_with<R>(&mut self, id: I, reset: R)
    where
        R: FnOnce(&mut T),
    {
        if !self.is_valid(id) {
            return;
        }
        let index = id.index() as u32;
        let slot = &mut self.slots[index as usize];
        slot.active = false;
        reset(&mut slot.value);
        self.free.push(index);
    }

    pub(crate) fn alloc_with<F, R>(&mut self, create: F, reset: R) -> I
    where
        F: FnOnce(I) -> T,
        R: FnOnce(&mut T, I),
    {
        if let Some(index) = self.free.pop() {
            let slot = &mut self.slots[index as usize];
            slot.generation = slot.generation.wrapping_add(1).max(1);
            slot.active = true;

            let id = I::from_parts(index, slot.generation);
            reset(&mut slot.value, id);
            id
        } else {
            let index = self.slots.len() as u32;
            let id = I::from_parts(index, 0);
            let value = create(id);
            self.slots.push(ArenaSlot {
                generation: 0,
                active: true,
                value,
            });
            id
        }
    }

    pub(crate) fn sweep<F, R>(&mut self, mut should_free: F, mut reset: R)
    where
        F: FnMut(&T) -> bool,
        R: FnMut(&mut T),
    {
        for index in 0..self.slots.len() {
            if !self.slots[index].active {
                continue;
            }
            if !should_free(&self.slots[index].value) {
                continue;
            }
            self.free_index(index as u32, &mut reset);
        }
    }

    fn free_index<R>(&mut self, index: u32, reset: &mut R)
    where
        R: FnMut(&mut T),
    {
        let slot = &mut self.slots[index as usize];
        slot.active = false;
        reset(&mut slot.value);
        self.free.push(index);
    }
}

#[cfg(test)]
mod tests {
    use super::{Arena, ArenaId};

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestId {
        index: u32,
        generation: u32,
    }

    impl ArenaId for TestId {
        const INVALID: Self = Self {
            index: u32::MAX,
            generation: 0,
        };

        fn from_parts(index: u32, generation: u32) -> Self {
            Self { index, generation }
        }

        fn index(self) -> usize {
            self.index as usize
        }

        fn generation(self) -> u32 {
            self.generation
        }
    }

    #[test]
    fn arena_reuses_slots_with_bumped_generation() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        let id1 = arena.alloc_with(
            |id| id.generation(),
            |value, id| {
                *value = id.generation();
            },
        );

        arena.free_with(id1, |value| {
            *value = 0;
        });

        let id2 = arena.alloc_with(
            |id| id.generation(),
            |value, id| {
                *value = id.generation();
            },
        );

        assert_eq!(id1.index, id2.index);
        assert!(id2.generation > id1.generation);
        assert!(arena.is_valid(id2));
        assert!(!arena.is_valid(id1));
    }

    #[test]
    fn sequential_allocation_increments_index() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        let id0 = arena.alloc_with(|_| 10, |_, _| {});
        let id1 = arena.alloc_with(|_| 20, |_, _| {});
        let id2 = arena.alloc_with(|_| 30, |_, _| {});

        assert_eq!(id0.index, 0);
        assert_eq!(id1.index, 1);
        assert_eq!(id2.index, 2);

        assert_eq!(*arena.get(id0), 10);
        assert_eq!(*arena.get(id1), 20);
        assert_eq!(*arena.get(id2), 30);
    }

    #[test]
    fn stale_generation_fails_is_valid() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        let old_id = arena.alloc_with(|_| 1, |_, _| {});
        assert!(arena.is_valid(old_id));

        // Free the slot and reallocate - generation bumps
        arena.free_with(old_id, |v| *v = 0);
        let new_id = arena.alloc_with(|_| 2, |v, _| *v = 2);

        // Same index, different generation
        assert_eq!(old_id.index, new_id.index);
        assert!(!arena.is_valid(old_id));
        assert!(arena.is_valid(new_id));
        assert_eq!(*arena.get(new_id), 2);
    }

    #[test]
    fn out_of_bounds_index_fails_is_valid() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        arena.alloc_with(|_| 42, |_, _| {});

        let bogus = TestId::from_parts(999, 0);
        assert!(!arena.is_valid(bogus));

        // INVALID sentinel should also fail
        assert!(!arena.is_valid(TestId::INVALID));
    }

    #[test]
    fn sweep_frees_matching_preserves_others() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        let id_a = arena.alloc_with(|_| 1, |_, _| {});
        let id_b = arena.alloc_with(|_| 2, |_, _| {});
        let id_c = arena.alloc_with(|_| 3, |_, _| {});

        // Sweep away the even value
        arena.sweep(|v| *v == 2, |v| *v = 0);

        assert!(arena.is_valid(id_a));
        assert!(!arena.is_valid(id_b));
        assert!(arena.is_valid(id_c));

        let active: Vec<&u32> = arena.iter_active().collect();
        assert_eq!(active.len(), 2);
        assert!(active.contains(&&1));
        assert!(active.contains(&&3));
    }

    #[test]
    fn free_all_then_reallocate() {
        let mut arena: Arena<u32, TestId> = Arena::new();
        let id0 = arena.alloc_with(|_| 10, |_, _| {});
        let id1 = arena.alloc_with(|_| 20, |_, _| {});
        let id2 = arena.alloc_with(|_| 30, |_, _| {});

        // Free all (order: 0, 1, 2 pushed onto free stack)
        arena.free_with(id0, |v| *v = 0);
        arena.free_with(id1, |v| *v = 0);
        arena.free_with(id2, |v| *v = 0);

        // Reallocate 3 - should pop from free stack in LIFO order (2, 1, 0)
        let new_a = arena.alloc_with(|_| 100, |v, _| *v = 100);
        let new_b = arena.alloc_with(|_| 200, |v, _| *v = 200);
        let new_c = arena.alloc_with(|_| 300, |v, _| *v = 300);

        assert_eq!(new_a.index, 2); // LIFO: last freed is first reused
        assert_eq!(new_b.index, 1);
        assert_eq!(new_c.index, 0);

        // All old IDs must be stale (generation bumped)
        assert!(!arena.is_valid(id0));
        assert!(!arena.is_valid(id1));
        assert!(!arena.is_valid(id2));

        // New IDs are valid with correct values
        assert_eq!(*arena.get(new_a), 100);
        assert_eq!(*arena.get(new_b), 200);
        assert_eq!(*arena.get(new_c), 300);
    }

    #[test]
    fn generation_wrapping() {
        let mut arena: Arena<u32, TestId> = Arena::new();

        // Allocate a slot (generation = 0, index = 0)
        let id = arena.alloc_with(|_| 1, |_, _| {});
        assert_eq!(id.generation, 0);

        // Free and reallocate once to get generation = 1
        arena.free_with(id, |v| *v = 0);
        let id1 = arena.alloc_with(|_| 2, |v, _| *v = 2);
        assert_eq!(id1.generation, 1);
        assert_eq!(id1.index, 0);

        // Manually set the slot generation to u32::MAX to test wrapping.
        // We need to free and then poke the internal generation before realloc.
        arena.free_with(id1, |v| *v = 0);

        // The slot is now at generation 1. We'll cheat by freeing and reallocating
        // in a loop... but that's slow. Instead, access internals directly via
        // the fact that free_with doesn't bump generation - only alloc_with does.
        // So we directly set the slot's generation to u32::MAX.
        arena.slots[0].generation = u32::MAX;

        // Now reallocate - wrapping_add(1) on u32::MAX = 0, then .max(1) = 1
        let id_wrapped = arena.alloc_with(|_| 99, |v, _| *v = 99);
        assert_eq!(id_wrapped.index, 0);
        assert_eq!(id_wrapped.generation, 1); // u32::MAX wraps to 0, clamped to 1
        assert!(arena.is_valid(id_wrapped));
        assert_eq!(*arena.get(id_wrapped), 99);

        // Also verify that generation 0 is skipped (the .max(1) guard), meaning
        // a stale ID with generation=0 can never accidentally match.
        let stale = TestId::from_parts(0, 0);
        assert!(!arena.is_valid(stale));
    }
}
