use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::style::Span;
use crate::utils::file_icons::file_icon;

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
                        let folder_icon = if expanded { " " } else { " " };
                        let icon = if is_root || !props.show_arrows {
                            // Remove the arrow for root or when arrows are disabled
                            folder_icon.chars().skip(2).collect::<String>()
                        } else {
                            folder_icon.to_string()
                        };

                        let mut span = Span::new(icon);
                        if props.icon_style == FileIconStyle::NerdFontColored {
                            // Directories are typically blue in mini.icons
                            span = span.fg(palette.blue);
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

pub(crate) fn path_to_display(path: &str) -> String {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() && path.starts_with(&home) {
        return path.replacen(&home, "~", 1);
    }
    path.to_string()
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
    if let Ok(canonical) = fs::canonicalize(path) {
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
