use std::path::PathBuf;

use typed_path::{Utf8TypedPath, Utf8TypedPathBuf};

/// A path from an application-provided tree, parsed independently of the host OS.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ProvidedPath {
    path: Utf8TypedPathBuf,
}

impl ProvidedPath {
    /// Resolve a provided root, retaining its spelling and path flavor.
    pub(crate) fn root(root: &str) -> Self {
        let parsed = Utf8TypedPath::derive(root);
        let path = if parsed.is_absolute() {
            parsed.to_path_buf().normalize()
        } else {
            let absolute = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(root);
            Utf8TypedPath::derive(&absolute.to_string_lossy())
                .to_path_buf()
                .normalize()
        };
        Self { path }
    }

    /// Interpret an already-normalized path using the flavor encoded in its spelling.
    pub(crate) fn from_path(path: &str) -> Self {
        Self {
            path: Utf8TypedPath::derive(path).to_path_buf(),
        }
    }

    pub(crate) fn as_str(&self) -> &str {
        self.path.as_str()
    }

    pub(crate) fn file_name(&self) -> Option<&str> {
        self.path.file_name()
    }

    pub(crate) fn is_windows(&self) -> bool {
        self.path.is_windows()
    }

    /// Resolve a listing/change/control path under this root.
    pub(crate) fn resolve(&self, path: &str) -> Option<Self> {
        let parsed = Utf8TypedPath::derive(path);
        let candidate = if parsed.is_absolute() {
            parsed.to_path_buf().normalize()
        } else {
            self.path.join(self.with_root_flavor(path)).normalize()
        };

        (candidate.is_windows() == self.is_windows() && candidate.starts_with(self.path.as_str()))
            .then_some(Self { path: candidate })
    }

    pub(crate) fn join_name(&self, name: &str) -> Self {
        Self {
            path: self.path.join(self.with_root_flavor(name)).normalize(),
        }
    }

    pub(crate) fn is_valid_name(&self, name: &str) -> bool {
        let mut components = self.with_root_flavor(name).components();
        matches!(components.next(), Some(component) if component.is_normal())
            && components.next().is_none()
    }

    pub(crate) fn starts_with(&self, root: &Self) -> bool {
        self.is_windows() == root.is_windows() && self.path.starts_with(root.as_str())
    }

    pub(crate) fn strip_prefix(&self, root: &Self) -> Option<Self> {
        (self.is_windows() == root.is_windows())
            .then(|| self.path.strip_prefix(root.as_str()).ok())
            .flatten()
            .map(|path| Self {
                path: path.to_path_buf(),
            })
    }

    pub(crate) fn parent(&self) -> Option<Self> {
        self.path.to_path().parent().map(|path| Self {
            path: path.to_path_buf(),
        })
    }

    pub(crate) fn components(&self) -> impl Iterator<Item = typed_path::Utf8TypedComponent<'_>> {
        self.path.components()
    }

    fn with_root_flavor<'a>(&self, path: &'a str) -> Utf8TypedPath<'a> {
        if self.is_windows() {
            Utf8TypedPath::windows(path)
        } else {
            Utf8TypedPath::unix(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProvidedPath;

    #[test]
    fn posix_roots_keep_posix_paths_on_any_host() {
        let root = ProvidedPath::root("/home/user/repo");

        assert!(!root.is_windows());
        assert_eq!(
            root.resolve("src/main.rs").unwrap().as_str(),
            "/home/user/repo/src/main.rs"
        );
        assert_eq!(root.join_name("src").as_str(), "/home/user/repo/src");
    }

    #[test]
    fn windows_roots_keep_windows_paths_on_any_host() {
        let root = ProvidedPath::root(r"C:\home\user\repo");

        assert!(root.is_windows());
        assert_eq!(
            root.resolve("src/main.rs").unwrap().as_str(),
            r"C:\home\user\repo\src\main.rs"
        );
        assert_eq!(root.join_name("src").as_str(), r"C:\home\user\repo\src");
    }

    #[test]
    fn resolution_rejects_traversal_prefix_siblings_and_other_flavors() {
        let root = ProvidedPath::root("/remote/repo");

        assert!(root.resolve("../etc").is_none());
        assert!(root.resolve("src/../../etc").is_none());
        assert!(root.resolve("/remote/repo2/file").is_none());
        assert!(root.resolve(r"C:\remote\repo\file").is_none());
        assert_eq!(
            root.resolve("src/../README.md").unwrap().as_str(),
            "/remote/repo/README.md"
        );
        assert!(root.is_valid_name("main.rs"));
        assert!(!root.is_valid_name("a/b"));

        let windows = ProvidedPath::root(r"C:\remote\repo");
        assert!(windows.resolve(r"C:\remote\repo2\file").is_none());
        assert!(windows.resolve(r"C:\remote\repo\..\etc").is_none());
        assert!(!windows.is_valid_name(r"a\b"));
    }
}
