use std::borrow::Cow;
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
/// `HOME` is the POSIX one, and a deliberate override wherever it is set. `USERPROFILE` is where
/// Windows keeps it, and its absence here is why the abbreviation never fired there: Windows does
/// not set `HOME`, and the shells that do (Git Bash) set it to a POSIX path that cannot
/// prefix-match a Windows one. Whichever the path is actually under wins, so a `HOME` that does not
/// fit does not mask a `USERPROFILE` that does.
const HOME_VARS: [&str; 2] = ["HOME", "USERPROFILE"];

/// A path as a person should read it: no Win32 verbatim prefix, home directory as `~`.
///
/// [`canonicalize_plain`] means stored paths have no prefix to drop by the time they arrive here.
/// The strip stays as the backstop for the paths that never went through it - a root the app passed
/// already spelled verbatim, which is kept as given when it cannot be canonicalized.
pub(crate) fn path_to_display(path: &str) -> String {
    let homes = HOME_VARS.map(|name| std::env::var(name).unwrap_or_default());
    abbreviate_home(&strip_verbatim_prefix(path), &homes)
}

/// Replace whichever of `homes` the path lies under with `~`.
///
/// The match has to land on a path component: a plain prefix test abbreviates `/home/adamant/src`
/// to `~ant/src` when `HOME` is `/home/adam`, naming a directory the user does not have. A trailing
/// separator on the variable itself (`HOME=/home/adam/`) is not part of the name either.
fn abbreviate_home(path: &str, homes: &[String]) -> String {
    for home in homes {
        let home = home.trim_end_matches(SEPARATORS);
        if home.is_empty() {
            continue;
        }
        if let Some(rest) = path.strip_prefix(home)
            && (rest.is_empty() || rest.starts_with(SEPARATORS))
        {
            return format!("~{rest}");
        }
    }
    path.to_string()
}

/// Both separators everywhere rather than the platform's own: the paths themselves come in either
/// shape, and a `cfg(windows)` rule would sit where CI - Linux-only - cannot run it. The cost is
/// that a POSIX file named with a literal backslash right after the home directory reads as though
/// it were a subdirectory, which is a cosmetic label on a pathological name.
const SEPARATORS: &[char] = &['/', '\\'];

/// Windows' extended-length path prefix, and the spelling of a UNC share behind it.
const VERBATIM_PREFIX: &str = r"\\?\";
const VERBATIM_UNC_PREFIX: &str = r"\\?\UNC\";

/// `fs::canonicalize`, spelled the way the rest of the widget spells paths.
///
/// On Windows canonicalize always returns the extended-length form, so this is the one door the
/// prefix can come through. Dropping it here rather than at the point of display keeps every path
/// the widget stores, keys git decorations by, matches item styles against, and hands to app code
/// in an event in the same spelling an app would build for itself.
///
/// Losing the prefix does not lose access to long paths: `std::fs` re-adds it, converting a path
/// past the legacy limit to the verbatim form before it reaches Win32.
pub(crate) fn canonicalize_plain(path: &Path) -> io::Result<PathBuf> {
    fs::canonicalize(path).map(|canonical| plain_path(&canonical))
}

/// The plain spelling of a path, where it has one.
fn plain_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if !text.starts_with(VERBATIM_PREFIX) {
        // Returned as it came, so a path that is not valid UTF-8 keeps every byte it had rather
        // than being round-tripped through a lossy conversion for nothing.
        return path.to_path_buf();
    }
    PathBuf::from(strip_verbatim_prefix(&text).as_ref())
}

/// Drop Windows' extended-length path prefix.
///
/// The prefix is a Win32 API detail - it opts a path out of the normalization every other path goes
/// through, and out of the `MAX_PATH` limit with it - and means nothing to a person reading a file
/// tree, or to an app comparing paths.
fn strip_verbatim_prefix(path: &str) -> Cow<'_, str> {
    // `\\?\UNC\server\share` is the verbatim spelling of `\\server\share`, so this one is replaced
    // rather than removed: cutting it off would leave a path rooted at the server name.
    if let Some(rest) = path.strip_prefix(VERBATIM_UNC_PREFIX) {
        return Cow::Owned(format!(r"\\{rest}"));
    }
    match path.strip_prefix(VERBATIM_PREFIX) {
        Some(rest) if is_drive_path(rest) => Cow::Borrowed(rest),
        // Anything else behind the prefix - a volume GUID, say - has no plain spelling, so it keeps
        // the one it has rather than being shortened into a path that names nothing.
        _ => Cow::Borrowed(path),
    }
}

/// Whether this is `C:` or `C:\…`, the only forms that stay valid once the prefix is dropped.
fn is_drive_path(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes.get(2), None | Some(b'\\'))
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
        // The parent path is already canonical. Construct child path directly
        // to avoid an `fs::canonicalize` syscall per entry. Only resolve
        // symlinks where the canonical target matters for consistency.
        let child_path = if matches!(kind, FileKind::Symlink) {
            normalize_path(&child.path())
        } else {
            Arc::from(root.join(name_str).to_string_lossy().as_ref())
        };

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
    use std::path::{Path, PathBuf};

    use super::{abbreviate_home, plain_path, strip_verbatim_prefix};

    /// Windows-shaped strings, run everywhere. The functions under test are pure string handling,
    /// and CI is Linux-only - gating these behind `cfg(windows)` would mean nothing ran them.
    #[test]
    fn a_verbatim_drive_prefix_is_not_shown() {
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\Users"), r"C:\Users");
        assert_eq!(strip_verbatim_prefix(r"\\?\C:\"), r"C:\");
        assert_eq!(strip_verbatim_prefix(r"\\?\C:"), "C:");
    }

    #[test]
    fn a_verbatim_unc_prefix_becomes_its_plain_spelling() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\dir"),
            r"\\server\share\dir",
            "cutting the prefix off would root the path at the server name"
        );
    }

    #[test]
    fn a_verbatim_path_with_no_plain_spelling_is_left_alone() {
        let volume = r"\\?\Volume{b75e2c83-0000-0000-0000-602f00000000}\dir";
        assert_eq!(strip_verbatim_prefix(volume), volume);
    }

    #[test]
    fn ordinary_paths_are_untouched() {
        assert_eq!(strip_verbatim_prefix(r"C:\Users"), r"C:\Users");
        assert_eq!(strip_verbatim_prefix(r"\\server\share"), r"\\server\share");
        assert_eq!(strip_verbatim_prefix("/home/adam/src"), "/home/adam/src");
    }

    #[test]
    fn a_home_that_does_not_fit_does_not_mask_one_that_does() {
        // Git Bash sets `HOME` to a POSIX path while the tree holds Windows ones, which is the
        // shape that has to fall through to `USERPROFILE`.
        let homes = ["/c/Users/adam".to_string(), r"C:\Users\adam".to_string()];
        assert_eq!(abbreviate_home(r"C:\Users\adam\src", &homes), r"~\src");
    }

    #[test]
    fn a_posix_home_is_abbreviated() {
        let homes = ["/home/adam".to_string(), String::new()];
        assert_eq!(abbreviate_home("/home/adam/src", &homes), "~/src");
    }

    #[test]
    fn an_unset_home_leaves_the_path_alone() {
        let homes = [String::new(), String::new()];
        assert_eq!(abbreviate_home(r"C:\Users\adam", &homes), r"C:\Users\adam");
    }

    #[test]
    fn the_home_match_lands_on_a_component_boundary() {
        let homes = ["/home/adam".to_string(), String::new()];
        assert_eq!(
            abbreviate_home("/home/adamant/src", &homes),
            "/home/adamant/src",
            "a plain prefix test would call this `~ant/src`, naming a directory nobody has"
        );
        assert_eq!(
            abbreviate_home("/home/adam", &homes),
            "~",
            "the home itself"
        );
        assert_eq!(abbreviate_home("/home/adam/src", &homes), "~/src");
    }

    #[test]
    fn a_trailing_separator_is_not_part_of_the_home_name() {
        let homes = ["/home/adam/".to_string(), String::new()];
        assert_eq!(abbreviate_home("/home/adam/src", &homes), "~/src");

        let windows = [r"C:\Users\adam\".to_string(), String::new()];
        assert_eq!(abbreviate_home(r"C:\Users\adam\src", &windows), r"~\src");
    }

    #[test]
    fn a_canonicalized_path_is_stored_without_the_prefix() {
        // What `fs::canonicalize` hands back on Windows, and what the widget stores instead - the
        // spelling an app comparing against its own paths would build.
        assert_eq!(
            plain_path(Path::new(r"\\?\C:\Users\adam\src")),
            PathBuf::from(r"C:\Users\adam\src")
        );
        assert_eq!(
            plain_path(Path::new(r"\\?\UNC\server\share\dir")),
            PathBuf::from(r"\\server\share\dir")
        );
    }

    #[test]
    fn a_path_with_no_prefix_is_stored_as_it_came() {
        for path in ["/home/adam/src", r"C:\Users\adam"] {
            assert_eq!(plain_path(Path::new(path)), PathBuf::from(path));
        }
    }
}
