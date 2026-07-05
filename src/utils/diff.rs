use std::collections::{HashMap, HashSet};

pub(crate) fn reuse_plan<I, K, New, KeyOld, KeyNew, IsValid, Matches>(
    old: &[I],
    new: &[New],
    key_old: KeyOld,
    key_new: KeyNew,
    is_valid: IsValid,
    matches: Matches,
) -> Vec<Option<I>>
where
    I: Copy + Eq + std::hash::Hash,
    K: Eq + std::hash::Hash + Clone,
    KeyOld: Fn(&I) -> Option<K>,
    KeyNew: Fn(&New) -> Option<K>,
    IsValid: Fn(&I) -> bool,
    Matches: Fn(&I, &New) -> bool,
{
    // Fast path: nothing to match against - skip HashMap/HashSet allocations.
    if new.is_empty() {
        return Vec::new();
    }
    if old.is_empty() {
        return vec![None; new.len()];
    }

    // Fast path: when ALL children are unkeyed (the overwhelmingly common case),
    // skip HashMap/HashSet allocations entirely and do positional matching.
    let all_unkeyed = new.iter().all(|c| key_new(c).is_none())
        && old.iter().all(|c| !is_valid(c) || key_old(c).is_none());

    if all_unkeyed {
        return reuse_plan_unkeyed(old, new, is_valid, matches);
    }

    let mut keyed: HashMap<K, Vec<I>> = HashMap::new();
    for &id in old {
        if !is_valid(&id) {
            continue;
        }
        if let Some(key) = key_old(&id) {
            keyed.entry(key).or_default().push(id);
        }
    }

    let mut used: HashSet<I> = HashSet::new();
    let mut cursor = 0usize;
    let mut plan = Vec::with_capacity(new.len());

    // Pass 1: forward cursor matching (preserves order for positionally stable children).
    for item in new.iter() {
        if let Some(key) = key_new(item) {
            let mut reused = None;
            if let Some(list) = keyed.get_mut(&key)
                && let Some(pos) = list.iter().position(|id| is_valid(id) && matches(id, item))
            {
                let id = list.remove(pos);
                used.insert(id);
                reused = Some(id);
            }
            plan.push(reused);
            continue;
        }

        let mut reused = None;
        while cursor < old.len() {
            let candidate = old[cursor];
            cursor += 1;

            if used.contains(&candidate) || !is_valid(&candidate) {
                continue;
            }

            if key_old(&candidate).is_some() {
                continue;
            }

            if matches(&candidate, item) {
                used.insert(candidate);
                reused = Some(candidate);
                break;
            }
        }

        plan.push(reused);
    }

    // Pass 2: salvage unmatched new items by scanning remaining unmatched old
    // children. This handles insertions at the beginning of unkeyed containers
    // (e.g. a DraggableTabBar inserted before a TextArea), where the forward
    // cursor skipped past viable reuse candidates.
    //
    // Build an index of remaining unkeyed, unused old items to avoid O(N²) scanning.
    let mut remaining_unkeyed: Vec<I> = old
        .iter()
        .copied()
        .filter(|id| !used.contains(id) && is_valid(id) && key_old(id).is_none())
        .collect();

    for (i, item) in new.iter().enumerate() {
        if plan[i].is_some() || key_new(item).is_some() {
            continue;
        }
        if let Some(pos) = remaining_unkeyed.iter().position(|id| matches(id, item)) {
            let candidate = remaining_unkeyed.remove(pos);
            used.insert(candidate);
            plan[i] = Some(candidate);
        }
    }

    plan
}

/// Fast path for the all-unkeyed case: positional matching with a salvage pass,
/// avoiding `HashMap` and `HashSet` allocations entirely.
fn reuse_plan_unkeyed<I, New, IsValid, Matches>(
    old: &[I],
    new: &[New],
    is_valid: IsValid,
    matches: Matches,
) -> Vec<Option<I>>
where
    I: Copy + Eq,
    IsValid: Fn(&I) -> bool,
    Matches: Fn(&I, &New) -> bool,
{
    // Track which old indices have been consumed. A bool vec is cheaper than a
    // HashSet for the small, contiguous index ranges typical of child lists.
    let mut used = vec![false; old.len()];
    let mut cursor = 0usize;
    let mut plan = Vec::with_capacity(new.len());

    // Pass 1: forward cursor matching - identical to the keyed path's unkeyed
    // branch but without the key_old / used-HashSet checks.
    for item in new.iter() {
        let mut reused = None;
        while cursor < old.len() {
            let idx = cursor;
            cursor += 1;

            if used[idx] || !is_valid(&old[idx]) {
                continue;
            }

            if matches(&old[idx], item) {
                used[idx] = true;
                reused = Some(old[idx]);
                break;
            }
        }
        plan.push(reused);
    }

    // Pass 2: salvage unmatched new items against remaining unused old items
    // (handles insertions at the beginning that cause the cursor to overshoot).
    let mut remaining: Vec<(usize, I)> = old
        .iter()
        .enumerate()
        .filter(|(idx, id)| !used[*idx] && is_valid(id))
        .map(|(idx, &id)| (idx, id))
        .collect();

    for (i, item) in new.iter().enumerate() {
        if plan[i].is_some() {
            continue;
        }
        if let Some(pos) = remaining.iter().position(|(_, id)| matches(id, item)) {
            let (old_idx, candidate) = remaining.remove(pos);
            used[old_idx] = true;
            plan[i] = Some(candidate);
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy)]
    struct OldItem {
        id: u32,
        key: Option<&'static str>,
        type_tag: &'static str,
        valid: bool,
    }

    #[derive(Debug)]
    struct NewItem {
        key: Option<&'static str>,
        type_tag: &'static str,
    }

    fn make_old(id: u32, key: Option<&'static str>, type_tag: &'static str) -> OldItem {
        OldItem {
            id,
            key,
            type_tag,
            valid: true,
        }
    }

    fn make_new(key: Option<&'static str>, type_tag: &'static str) -> NewItem {
        NewItem { key, type_tag }
    }

    fn run(old: &[OldItem], new: &[NewItem]) -> Vec<Option<u32>> {
        let ids: Vec<u32> = old.iter().map(|o| o.id).collect();
        // Build lookup from id → OldItem for closures.
        let lookup: HashMap<u32, OldItem> = old.iter().map(|o| (o.id, *o)).collect();

        reuse_plan(
            &ids,
            new,
            |id| lookup[id].key,
            |n: &NewItem| n.key,
            |id| lookup[id].valid,
            |id, n: &NewItem| lookup[id].type_tag == n.type_tag,
        )
    }

    #[test]
    fn both_empty() {
        let plan = run(&[], &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn old_empty_new_nonempty() {
        let new = [make_new(Some("a"), "btn"), make_new(None, "txt")];
        let plan = run(&[], &new);
        assert_eq!(plan, vec![None, None]);
    }

    #[test]
    fn new_empty() {
        let old = [make_old(1, Some("a"), "btn"), make_old(2, None, "txt")];
        let plan = run(&old, &[]);
        assert!(plan.is_empty());
    }

    #[test]
    fn all_keyed_same_order() {
        let old = [
            make_old(1, Some("a"), "btn"),
            make_old(2, Some("b"), "btn"),
            make_old(3, Some("c"), "btn"),
        ];
        let new = [
            make_new(Some("a"), "btn"),
            make_new(Some("b"), "btn"),
            make_new(Some("c"), "btn"),
        ];
        let plan = run(&old, &new);
        assert_eq!(plan, vec![Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn all_keyed_reordered() {
        let old = [
            make_old(1, Some("a"), "btn"),
            make_old(2, Some("b"), "btn"),
            make_old(3, Some("c"), "btn"),
        ];
        let new = [
            make_new(Some("c"), "btn"),
            make_new(Some("a"), "btn"),
            make_new(Some("b"), "btn"),
        ];
        let plan = run(&old, &new);
        assert_eq!(plan, vec![Some(3), Some(1), Some(2)]);
    }

    #[test]
    fn keyed_items_added() {
        let old = [make_old(1, Some("a"), "btn")];
        let new = [
            make_new(Some("a"), "btn"),
            make_new(Some("x"), "btn"),
            make_new(Some("y"), "btn"),
        ];
        let plan = run(&old, &new);
        assert_eq!(plan, vec![Some(1), None, None]);
    }

    #[test]
    fn keyed_items_removed() {
        let old = [
            make_old(1, Some("a"), "btn"),
            make_old(2, Some("b"), "btn"),
            make_old(3, Some("c"), "btn"),
        ];
        let new = [make_new(Some("b"), "btn")];
        let plan = run(&old, &new);
        assert_eq!(plan, vec![Some(2)]);
    }

    #[test]
    fn all_unkeyed_same_count() {
        let old = [
            make_old(10, None, "txt"),
            make_old(11, None, "txt"),
            make_old(12, None, "txt"),
        ];
        let new = [
            make_new(None, "txt"),
            make_new(None, "txt"),
            make_new(None, "txt"),
        ];
        let plan = run(&old, &new);
        // Forward cursor matches positionally: 10, 11, 12.
        assert_eq!(plan, vec![Some(10), Some(11), Some(12)]);
    }

    #[test]
    fn unkeyed_insertion_at_beginning() {
        // Old: [txt(10), txt(11)]
        // New: [btn(new), txt, txt]
        // Forward cursor: btn doesn't match txt(10), advances; doesn't match txt(11),
        // advances; cursor exhausted → None. Then txt matches nothing (cursor past end) → None.
        // Salvage pass: the two trailing txt items find txt(10) and txt(11).
        let old = [make_old(10, None, "txt"), make_old(11, None, "txt")];
        let new = [
            make_new(None, "btn"),
            make_new(None, "txt"),
            make_new(None, "txt"),
        ];
        let plan = run(&old, &new);
        // btn gets None (no compatible old item), txt items are salvaged.
        assert_eq!(plan, vec![None, Some(10), Some(11)]);
    }

    #[test]
    fn mixed_keyed_and_unkeyed() {
        let old = [
            make_old(1, Some("header"), "frame"),
            make_old(2, None, "txt"),
            make_old(3, Some("footer"), "frame"),
            make_old(4, None, "txt"),
        ];
        let new = [
            make_new(None, "txt"),
            make_new(Some("footer"), "frame"),
            make_new(None, "txt"),
            make_new(Some("header"), "frame"),
        ];
        let plan = run(&old, &new);
        // Keyed: "footer" → 3, "header" → 1.
        // Unkeyed forward cursor: first unkeyed txt scans past id=1 (keyed, skipped),
        // finds id=2 (unkeyed txt) → match. Second unkeyed txt: cursor continues past
        // id=3 (keyed, skipped), finds id=4 (unkeyed txt) → match.
        assert_eq!(plan, vec![Some(2), Some(3), Some(4), Some(1)]);
    }

    #[test]
    fn is_valid_filters_stale() {
        let mut old = [
            make_old(1, Some("a"), "btn"),
            make_old(2, None, "txt"),
            make_old(3, None, "txt"),
        ];
        old[0].valid = false; // id=1 is stale
        old[1].valid = false; // id=2 is stale

        let new = [make_new(Some("a"), "btn"), make_new(None, "txt")];
        let plan = run(&old, &new);
        // id=1 invalid → keyed lookup skips it. id=2 invalid → cursor skips it.
        // id=3 (valid, unkeyed txt) matches second new item.
        assert_eq!(plan, vec![None, Some(3)]);
    }

    #[test]
    fn matches_rejects_incompatible() {
        let old = [make_old(1, Some("x"), "btn"), make_old(2, None, "btn")];
        let new = [
            make_new(Some("x"), "txt"), // same key but different type_tag
            make_new(None, "txt"),      // different type_tag than old id=2
        ];
        let plan = run(&old, &new);
        // Keyed: key "x" found in old (id=1), but matches() fails (btn != txt) → None.
        // Unkeyed: cursor finds id=2 (btn), matches fails (btn != txt) → None.
        assert_eq!(plan, vec![None, None]);
    }
}
