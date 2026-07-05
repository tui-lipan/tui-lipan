use std::hash::{Hash, Hasher};
use std::panic::Location;
use std::sync::Arc;

use crate::core::element::{Element, ElementKind};

/// Stable call-site identity for [`Memo`] entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct MemoCallSite(pub(crate) u64);

/// Internal payload for [`ElementKind::Memo`].
#[derive(Clone)]
pub(crate) struct MemoElement {
    pub(crate) deps_hash: u64,
    pub(crate) call_site: MemoCallSite,
    pub(crate) builder: Arc<dyn Fn() -> Element>,
}

/// In-view subtree memoization wrapper.
///
/// `deps_hash` must include **all values captured by the closure** that can
/// affect the generated subtree. Missing captures can lead to stale UI.
#[derive(Clone, Copy, Debug)]
pub struct Memo {
    deps_hash: u64,
    call_site: MemoCallSite,
}

impl Memo {
    /// Create a memo wrapper with a dependency hash.
    ///
    /// `deps_hash` must include all closure captures used by `build`.
    ///
    /// ## Call-site identity
    ///
    /// [`#[track_caller]`](https://doc.rust-lang.org/reference/attributes/codegen.html#the-track_caller-attribute)
    /// uses the **immediate** caller's source location. If every call goes through a shared
    /// helper (e.g. `fn my_memo() { Memo::new(h).build(...) }`), all calls share one
    /// location and memo cache entries collide. Use [`Memo::with_call_site`](Self::with_call_site)
    /// when wrapping `Memo::new` in helpers.
    #[track_caller]
    pub fn new(deps_hash: u64) -> Self {
        Self {
            deps_hash,
            call_site: stable_memo_call_site(),
        }
    }

    /// Build a memo with an explicit call-site id (for helpers wrapping [`Memo::new`](Self::new)).
    ///
    /// Combine with a per-call-site or per-instance id so memo entries do not collide.
    pub fn with_call_site(deps_hash: u64, call_site: u64) -> Self {
        Self {
            deps_hash,
            call_site: MemoCallSite(call_site),
        }
    }

    /// Build a memoized subtree.
    pub fn build<F, E>(self, builder: F) -> Element
    where
        F: Fn() -> E + 'static,
        E: Into<Element>,
    {
        Element::new(ElementKind::Memo(MemoElement {
            deps_hash: self.deps_hash,
            call_site: self.call_site,
            builder: Arc::new(move || builder().into()),
        }))
    }
}

/// Compute a stable per-call-site identity for memo entries.
#[track_caller]
pub(crate) fn stable_memo_call_site() -> MemoCallSite {
    let location = Location::caller();
    let mut hasher = rustc_hash::FxHasher::default();
    location.file().hash(&mut hasher);
    location.line().hash(&mut hasher);
    location.column().hash(&mut hasher);
    MemoCallSite(hasher.finish())
}
