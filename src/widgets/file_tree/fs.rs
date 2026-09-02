use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::style::Span;
use crate::utils::file_icons::{directory_icon, file_icon};

/// Icon style for file tree items.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FileIconStyle {
    /// Text labels with bracketed prefixes (e.g. `'[F]'`, `'[D]'`, `'[L]'`).
    #[default]
    Text,
    /// Nerd font icons without colors
    NerdFont,
    /// Nerd font icons with semantic colors (like mini.icons)
    NerdFontColored,
}

/// Filesystem entry kind used by `FileTree` events.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileKind {
    /// Directory entry.
    Directory,
    /// Regular file entry.
    File,
    /// Symlink entry.
    Symlink,
    /// Any other filesystem type.
    Other,
}

impl FileKind {
    pub(crate) fn from_file_type(file_type: &fs::FileType) -> Self {
        if file_type.is_dir() {
            Self::Directory
        } else if file_type.is_file() {
            Self::File
        } else if file_type.is_symlink() {
            Self::Symlink
        } else {
            Self::Other
        }
    }

    pub(crate) fn icon(
        self,
        path: &str,
        expanded: bool,
        is_root: bool,
        props: &super::mod_private::FileTreeProps,
    ) -> Span {
        let palette = &props.icon_palette;

        match self {
            Self::Directory => {
                // Check if there's a custom override for this directory name
                let path_obj = Path::new(path);
                if let Some(name) = path_obj.file_name().and_then(|n| n.to_str())
                    && let Some(override_icon) = props.icon_overrides.get(name)
                {
                    let mut span = Span::new(override_icon.icon.clone());
                    if let Some(color) = override_icon.color {
                        span = span.fg(color);
                    }
                    return span;
                }

                let base = if expanded {
                    &props.opened_directory_icon
                } else {
                    &props.directory_icon
                };

                match props.icon_style {
                    FileIconStyle::Text => Span::new(base.as_ref()),
                    FileIconStyle::NerdFont | FileIconStyle::NerdFontColored => {
                        let (folder_glyph, folder_color) = directory_icon(expanded, palette);
                        let icon = if is_root || !props.show_arrows {
                            folder_glyph.to_string()
                        } else {
                            let arrow = if expanded {
                                "\u{f47c} " // 
                            } else {
                                "\u{f460} " // 
                            };
                            format!("{arrow}{folder_glyph}")
                        };

                        let mut span = Span::new(icon);
                        if props.icon_style == FileIconStyle::NerdFontColored
                            && let Some(c) = folder_color
                        {
                            span = span.fg(c);
                        }
                        span
                    }
                }
            }
            Self::File => {
                // Check if there's a custom override for this file
                let path_obj = Path::new(path);

                // Check by full filename first
                if let Some(name) = path_obj.file_name().and_then(|n| n.to_str())
                    && let Some(override_icon) = props.icon_overrides.get(name)
                {
                    let mut span = Span::new(override_icon.icon.clone());
                    if let Some(color) = override_icon.color {
                        span = span.fg(color);
                    }
                    return span;
                }

                // Then check by extension
                if let Some(ext) = path_obj.extension().and_then(|e| e.to_str())
                    && let Some(override_icon) = props.icon_overrides.get(ext)
                {
                    let mut span = Span::new(override_icon.icon.clone());
                    if let Some(color) = override_icon.color {
                        span = span.fg(color);
                    }
                    return span;
                }

                match props.icon_style {
                    FileIconStyle::Text => Span::new(props.file_icon.clone()),
                    FileIconStyle::NerdFont | FileIconStyle::NerdFontColored => {
                        let (icon, color) = file_icon(path, &props.icon_palette);
                        let mut span = Span::new(icon);
                        if props.icon_style == FileIconStyle::NerdFontColored
                            && let Some(c) = color
                        {
                            span = span.fg(c);
                        }
                        span
                    }
                }
            }
            Self::Symlink => {
                // Check if there's a custom override for symlinks
                let path_obj = Path::new(path);
                if let Some(name) = path_obj.file_name().and_then(|n| n.to_str())
                    && let Some(override_icon) = props.icon_overrides.get(name)
                {
                    let mut span = Span::new(override_icon.icon.clone());
                    if let Some(color) = override_icon.color {
                        span = span.fg(color);
                    }
                    return span;
                }

                match props.icon_style {
                    FileIconStyle::Text => Span::new(props.symlink_icon.clone()),
                    FileIconStyle::NerdFont | FileIconStyle::NerdFontColored => {
                        let mut span = Span::new("󰁔");
                        if props.icon_style == FileIconStyle::NerdFontColored {
                            // Symlinks are typically cyan in mini.icons
                            span = span.fg(palette.cyan);
                        }
                        span
                    }
                }
            }
            Self::Other => {
                // Check if there's a custom override
                let path_obj = Path::new(path);
                if let Some(name) = path_obj.file_name().and_then(|n| n.to_str())
                    && let Some(override_icon) = props.icon_overrides.get(name)
                {
                    let mut span = Span::new(override_icon.icon.clone());
                    if let Some(color) = override_icon.color {
                        span = span.fg(color);
                    }
                    return span;
                }

                match props.icon_style {
                    FileIconStyle::Text => Span::new(props.other_icon.clone()),
                    FileIconStyle::NerdFont | FileIconStyle::NerdFontColored => {
                        let mut span = Span::new("󰈔");
                        if props.icon_style == FileIconStyle::NerdFontColored {
                            span = span.fg(palette.grey);
                        }
                        span
                    }
                }
            }
        }
    }
}

/// Where the home directory is kept, in the order they are tried.
///
/// `USERPROFILE` is Windows' own, and its absence here is why the abbreviation never fired there:
/// Windows does not set `HOME`, and the shells that do (Git Bash) set it to a POSIX path that
/// cannot prefix-match a Windows one. `HOME` stays a candidate on both, as the deliberate override.
/// Whichever the path actually lies under wins, so one that does not fit cannot mask one that does.
#[cfg(windows)]
const HOME_VARS: &[&str] = &["USERPROFILE", "HOME"];
#[cfg(not(windows))]
const HOME_VARS: &[&str] = &["HOME"];

/// What separates one path component from the next. Windows accepts either; on POSIX a backslash
/// is an ordinary character in a filename and must not split anything.
#[cfg(windows)]
const SEPARATORS: &[char] = &['\\', '/'];
#[cfg(not(windows))]
const SEPARATORS: &[char] = &['/'];

/// A path as a person should read it: plainly spelled, home directory as `~`.
///
/// [`canonicalize_plain`] means a stored path is already spelled as plainly as it safely can be by
/// the time it arrives here. Simplifying again is the backstop for the paths that never went
/// through it - a root the app passed already spelled verbatim, kept as given because it could not
/// be canonicalized.
pub(crate) fn path_to_display(path: &str) -> String {
    let homes: Vec<String> = HOME_VARS
        .iter()
        .map(|name| std::env::var(name).unwrap_or_default())
        .collect();
    let simplified = dunce::simplified(Path::new(path));
    abbreviate_home(&simplified.to_string_lossy(), &homes, SEPARATORS)
}

/// Replace whichever of `homes` the path lies under with `~`.
///
/// The match has to land on a component boundary: a plain prefix test abbreviates
/// `/home/adamant/src` to `~ant/src` when `HOME` is `/home/adam`, naming a directory the user does
/// not have. A trailing separator on the variable itself (`HOME=/home/adam/`) is not part of the
/// name either.
///
/// `separators` is a parameter rather than the constant so the Windows rule can be exercised on any
/// platform: the production values are [`SEPARATORS`], which differ per platform, and the tests
/// below pass both shapes explicitly.
fn abbreviate_home(path: &str, homes: &[String], separators: &[char]) -> String {
    for home in homes {
        let home = home.trim_end_matches(separators);
        if home.is_empty() {
            continue;
        }
        if let Some(rest) = path.strip_prefix(home)
            && (rest.is_empty() || rest.starts_with(separators))
        {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// `fs::canonicalize`, spelled the way the rest of the widget spells paths.
///
/// On Windows canonicalize always returns the extended-length `\?\` form, so this is the one door
/// that spelling can come through. Simplifying it here rather than at the point of display keeps
/// every path the widget stores, keys git decorations by, matches item styles against, and hands to
/// app code in an event in the same spelling an app would build for itself.
///
/// The contract is *plain wherever plain means the same thing*, not *never verbatim*. `\\?\` does
/// more than lift the `MAX_PATH` limit: it also turns off the Win32 parsing that would rewrite the
/// path on the way to the filesystem. A name ending in a dot or space, a component that collides
/// with a reserved DOS device (`NUL`, `COM1`), or a path over 260 characters therefore has no plain
/// spelling that still refers to the same file, and keeps the verbatim one. `dunce` draws exactly
/// that line, and draws it against real Win32 rules rather than a prefix test; it is also why a
/// long path handed to `git -C` stays in the form that works, since it is never shortened.
pub(crate) fn canonicalize_plain(path: &Path) -> io::Result<PathBuf> {
    dunce::canonicalize(path)
}

#[derive(Clone, Debug)]
pub(crate) struct FsNode {
    pub(crate) name: Arc<str>,
    pub(crate) path: Arc<str>,
    pub(crate) kind: FileKind,
    pub(crate) loaded: bool,
    pub(crate) loading: bool,
    pub(crate) error: Option<Arc<str>>,
    pub(crate) children: Vec<FsNode>,
}

impl FsNode {
    pub(crate) fn is_dir(&self) -> bool {
        matches!(self.kind, FileKind::Directory)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct LoadedEntry {
    pub(crate) name: Arc<str>,
    pub(crate) path: Arc<str>,
    pub(crate) kind: FileKind,
}

#[derive(Clone, Debug)]
pub(crate) struct DirectoryLoadResult {
    pub(crate) entries: Vec<LoadedEntry>,
    pub(crate) omitted: usize,
    pub(crate) error: Option<Arc<str>>,
}

pub(crate) fn read_directory(
    path: &str,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> DirectoryLoadResult {
    let mut entries = Vec::new();
    let mut omitted = 0usize;
    let root = PathBuf::from(path);

    let read_dir = match fs::read_dir(&root) {
        Ok(read_dir) => read_dir,
        Err(err) => {
            return DirectoryLoadResult {
                entries,
                omitted,
                error: Some(err.to_string().into()),
            };
        }
    };

    for child in read_dir {
        let Ok(child) = child else {
            continue;
        };
        let name = child.file_name();
        let Some(name_str) = name.to_str() else {
            continue;
        };
        if !show_hidden && is_hidden_name(name_str) {
            continue;
        }

        let Ok(file_type) = child.file_type() else {
            continue;
        };

        if entries.len() >= max_entries_per_dir {
            omitted = omitted.saturating_add(1);
            continue;
        }

        let kind = FileKind::from_file_type(&file_type);
        // The parent path is already canonical, so a child is built from it directly rather than
        // paying an `fs::canonicalize` syscall per entry.
        //
        // A symlink is spelled the same way, by its own name rather than its target's. The path is
        // what identifies a row - selection, expansion, git decorations, and the path an activation
        // hands the application are all keyed by it - and resolving the link collapses two rows onto
        // one identity: a repository where `CLAUDE.md` links to `AGENTS.md` gets two rows claiming
        // to be `AGENTS.md`, so selecting one lands on the other. Git says the same thing: it
        // reports the link's own path, never the target's.
        let child_path = Arc::<str>::from(root.join(name_str).to_string_lossy().as_ref());

        entries.push(LoadedEntry {
            name: Arc::from(name_str),
            path: child_path,
            kind,
        });
    }

    entries.sort_by(|left, right| {
        let left_dir = matches!(left.kind, FileKind::Directory);
        let right_dir = matches!(right.kind, FileKind::Directory);
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    DirectoryLoadResult {
        entries,
        omitted,
        error: None,
    }
}

pub(crate) fn normalize_path(path: &Path) -> Arc<str> {
    if let Ok(canonical) = canonicalize_plain(path) {
        return Arc::<str>::from(canonical.to_string_lossy().as_ref());
    }
    Arc::<str>::from(path.to_string_lossy().as_ref())
}

fn is_hidden_name(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

pub(crate) fn root_node(root: &Arc<str>) -> FsNode {
    let path = PathBuf::from(root.as_ref());
    let name = display_name(&path);

    match fs::symlink_metadata(&path) {
        Ok(meta) => {
            let kind = FileKind::from_file_type(&meta.file_type());
            FsNode {
                name,
                path: normalize_path(&path),
                kind,
                loaded: !matches!(kind, FileKind::Directory),
                loading: false,
                error: None,
                children: Vec::new(),
            }
        }
        Err(err) => FsNode {
            name,
            path: normalize_path(&path),
            kind: FileKind::Other,
            loaded: true,
            loading: false,
            error: Some(err.to_string().into()),
            children: Vec::new(),
        },
    }
}

fn display_name(path: &Path) -> Arc<str> {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(Arc::from)
        .unwrap_or_else(|| Arc::<str>::from(path.to_string_lossy().as_ref()))
}

#[cfg(test)]
mod tests {
    use super::abbreviate_home;
    #[cfg(unix)]
    use super::{canonicalize_plain, fs, read_directory};

    /// The Windows separator set, passed explicitly so the Windows rule is exercised on the Linux
    /// CI that runs these. `SEPARATORS` itself is per-platform; this is the shape it takes there.
    const WINDOWS: &[char] = &['\\', '/'];
    const POSIX: &[char] = &['/'];

    /// A symlink is a row of its own. Storing its target's path instead gave a repository where
    /// `CLAUDE.md` links to `AGENTS.md` two rows with one identity, and everything keyed by path -
    /// selection above all - could then only pick one of them.
    #[cfg(unix)]
    #[test]
    fn a_symlink_keeps_its_own_path_rather_than_its_targets() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-symlink-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).expect("temp dir");
        fs::write(dir.join("AGENTS.md"), "guide").expect("target file");
        std::os::unix::fs::symlink("AGENTS.md", dir.join("CLAUDE.md")).expect("symlink");

        let root = canonicalize_plain(&dir).expect("temp dir canonicalizes");
        let loaded = read_directory(&root.to_string_lossy(), false, 2_000);

        let path_of = |name: &str| {
            loaded
                .entries
                .iter()
                .find(|entry| entry.name.as_ref() == name)
                .map(|entry| entry.path.to_string())
                .unwrap_or_else(|| panic!("{name} listed"))
        };
        assert_eq!(
            path_of("CLAUDE.md"),
            root.join("CLAUDE.md").to_string_lossy(),
            "the link is spelled by its own name"
        );
        assert_ne!(
            path_of("CLAUDE.md"),
            path_of("AGENTS.md"),
            "two rows, two identities"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_home_that_does_not_fit_does_not_mask_one_that_does() {
        // Git Bash sets `HOME` to a POSIX path while the tree holds Windows ones, which is the
        // shape that has to fall through to the next candidate.
        let homes = [r"C:\Users\adam".to_string(), "/c/Users/adam".to_string()];
        assert_eq!(
            abbreviate_home(r"C:\Users\adam\src", &homes, WINDOWS),
            r"~\src"
        );

        let reversed = ["/c/Users/adam".to_string(), r"C:\Users\adam".to_string()];
        assert_eq!(
            abbreviate_home(r"C:\Users\adam\src", &reversed, WINDOWS),
            r"~\src",
            "order decides nothing when only one candidate can match"
        );
    }

    #[test]
    fn a_posix_home_is_abbreviated() {
        let homes = ["/home/adam".to_string()];
        assert_eq!(abbreviate_home("/home/adam/src", &homes, POSIX), "~/src");
    }

    #[test]
    fn an_unset_home_leaves_the_path_alone() {
        let homes = [String::new(), String::new()];
        assert_eq!(
            abbreviate_home(r"C:\Users\adam", &homes, WINDOWS),
            r"C:\Users\adam"
        );
    }

    #[test]
    fn the_home_match_lands_on_a_component_boundary() {
        let homes = ["/home/adam".to_string()];
        assert_eq!(
            abbreviate_home("/home/adamant/src", &homes, POSIX),
            "/home/adamant/src",
            "a plain prefix test would call this `~ant/src`, naming a directory nobody has"
        );
        assert_eq!(
            abbreviate_home("/home/adam", &homes, POSIX),
            "~",
            "the home itself"
        );
        assert_eq!(abbreviate_home("/home/adam/src", &homes, POSIX), "~/src");
    }

    #[test]
    fn a_trailing_separator_is_not_part_of_the_home_name() {
        let posix = ["/home/adam/".to_string()];
        assert_eq!(abbreviate_home("/home/adam/src", &posix, POSIX), "~/src");

        let windows = [r"C:\Users\adam\".to_string()];
        assert_eq!(
            abbreviate_home(r"C:\Users\adam\src", &windows, WINDOWS),
            r"~\src"
        );
    }

    #[test]
    fn a_backslash_is_an_ordinary_character_in_a_posix_name() {
        // The same input the Windows set would split. On POSIX `adam\x` is one directory name, and
        // a file inside a differently named directory must not be abbreviated as though it were in
        // the home.
        let homes = ["/home/adam".to_string()];
        assert_eq!(
            abbreviate_home(r"/home/adam\x", &homes, POSIX),
            r"/home/adam\x"
        );
        assert_eq!(abbreviate_home(r"/home/adam\x", &homes, WINDOWS), r"~\x");
    }

    /// The plain-spelling contract itself belongs to `dunce`, which decides it against real Win32
    /// rules and can only do so on Windows - `dunce::simplified` is a no-op everywhere else. So
    /// this runs only where it means something, and needs a Windows CI job to run at all.
    #[cfg(windows)]
    #[test]
    fn an_ordinary_canonical_path_is_stored_plainly() {
        let temporary = std::env::temp_dir();
        let canonical = super::canonicalize_plain(&temporary).expect("temp dir canonicalizes");
        assert!(
            !canonical.to_string_lossy().starts_with(r"\\?\"),
            "an ordinary short path has a plain spelling: {canonical:?}"
        );
        assert!(canonical.is_dir(), "and still names the same directory");
    }
}
