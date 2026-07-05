use std::path::Path;
use std::sync::Arc;

use crate::style::{Color, FileIconPalette, Span};

/// Custom icon override for specific file extensions or names.
#[derive(Clone, Debug, PartialEq)]
pub struct FileIconOverride {
    /// The icon glyph or text.
    pub icon: Arc<str>,
    /// Optional color for the icon.
    pub color: Option<Color>,
}

/// Build a [`Span`] with the file's Nerd Font icon, colored from `palette`.
///
/// Convenience wrapper over [`file_icon`] yielding ready-to-use list content:
///
/// ```
/// use tui_lipan::prelude::*;
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::file_icon_span;
///
/// let palette = FileIconPalette::default();
/// let _item = ListItem::from_spans([
///     file_icon_span("main.rs", &palette),
///     Span::new(" main.rs"),
/// ]);
/// ```
pub fn file_icon_span(name: &str, palette: &FileIconPalette) -> Span {
    let (glyph, color) = file_icon(name, palette);
    let mut span = Span::new(glyph);
    if let Some(color) = color {
        span = span.fg(color);
    }
    span
}

/// Resolve the Nerd Font glyph and semantic color for a file name or path.
///
/// This is the canonical extension/name → icon mapping; [`FileTree`] and the
/// file-icon tab decorations call it directly, and it is exposed so file icons
/// can be added to a plain [`List`] (or any widget) without reimplementing the
/// table. Accepts either a bare file name or a full path. The color comes from
/// `palette` (typically `theme.file_icons`) so icons match the active theme;
/// `None` means the icon has no semantic color and should inherit the
/// surrounding style.
///
/// ```
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::file_icon;
///
/// let palette = FileIconPalette::default();
/// let (glyph, color) = file_icon("main.rs", &palette);
/// ```
///
/// [`FileTree`]: crate::widgets::FileTree
/// [`List`]: crate::widgets::List
pub fn file_icon(name: &str, palette: &FileIconPalette) -> (&'static str, Option<Color>) {
    let path_obj = Path::new(name);
    let name = path_obj.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name_lower = name.to_lowercase();

    match name_lower.as_str() {
        "dockerfile" => ("󰡨", Some(palette.cyan)),
        "package.json" => ("", Some(palette.green)),
        "package-lock.json" => ("", Some(palette.grey)),
        "readme.md" | "readme" => ("", Some(palette.yellow)),
        "license" | "license.md" | "license.txt" => ("󰘥", Some(palette.cyan)),
        "makefile" | "makefile.am" | "makefile.in" => ("", Some(palette.yellow)),
        "cmakelists.txt" => ("", Some(palette.red)),
        ".gitignore" | ".gitattributes" | ".gitmodules" => ("", Some(palette.red)),
        ".dockerignore" => ("󰡨", Some(palette.orange)),
        ".editorconfig" => ("", Some(palette.grey)),
        ".eslintignore" | ".eslintrc" | ".eslintrc.js" | ".eslintrc.json" => {
            ("", Some(palette.purple))
        }
        ".prettierrc" | ".prettierrc.js" | ".prettierrc.json" | ".prettierignore" => {
            ("", Some(palette.orange))
        }
        _ => {
            let Some(ext) = path_obj.extension().and_then(|e| e.to_str()) else {
                return ("󰈔", None);
            };

            match ext.to_lowercase().as_str() {
                // Rust
                "rs" => ("", Some(palette.red)),
                "toml" => ("", Some(palette.grey)),

                // Python
                "py" | "pyw" | "pyi" => ("", Some(palette.yellow)),
                "pyc" | "pyd" | "pyo" => ("", Some(palette.grey)),

                // JavaScript/TypeScript
                "js" | "mjs" | "cjs" | "jsx" => ("", Some(palette.yellow)),
                "ts" => ("", Some(palette.cyan)),
                "tsx" => ("", Some(palette.cyan)),

                // Web
                "html" | "htm" => ("", Some(palette.orange)),
                "css" => ("", Some(palette.blue)),
                "scss" | "sass" => ("", Some(palette.red)),
                "less" => ("", Some(palette.blue)),

                // JSON/Data
                "json" | "jsonc" => ("", Some(palette.grey)),
                "yaml" | "yml" => ("", Some(palette.grey)),

                // C/C++
                "c" => ("", Some(palette.blue)),
                "cpp" | "cc" | "cxx" => ("", Some(palette.azure)),
                "h" | "hpp" => ("", Some(palette.purple)),

                // Go
                "go" | "mod" | "sum" => ("", Some(palette.cyan)),

                // Ruby
                "rb" | "erb" | "rbw" => ("", Some(palette.red)),
                "gemfile" | "gemfile.lock" => ("", Some(palette.red)),

                // Shell
                "sh" | "bash" | "zsh" | "fish" => ("", Some(palette.green)),
                "ps1" | "psm1" | "psd1" => ("", Some(palette.blue)),

                // Java/Kotlin
                "java" | "jar" => ("", Some(palette.orange)),
                "gradle" => ("", Some(palette.green)),
                "kt" | "kts" => ("", Some(palette.purple)),

                // Documentation
                "md" | "markdown" | "rst" => ("", Some(palette.azure)),
                "txt" => ("", Some(palette.grey)),

                // Images
                "png" => ("󰸭", Some(palette.purple)),
                "jpg" | "jpeg" => ("󰈥", Some(palette.purple)),
                "gif" => ("󰵸", Some(palette.purple)),
                "svg" => ("󰜡", Some(palette.orange)),
                "webp" => ("󰈟", Some(palette.blue)),
                "bmp" | "ico" | "tiff" | "tif" => ("󰈟", Some(palette.purple)),

                // Archives
                "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" => {
                    ("󰗄", Some(palette.blue))
                }

                // Git
                "git" | "gitignore" | "gitattributes" | "gitmodules" => ("", Some(palette.red)),

                // Misc
                "lock" => ("", Some(palette.grey)),
                "sql" => ("", Some(palette.yellow)),
                "db" | "sqlite" | "sqlite3" => ("", Some(palette.grey)),
                "conf" | "cfg" | "config" | "ini" => ("", Some(palette.grey)),
                "log" => ("", Some(palette.grey)),
                "vim" | "nvim" => ("", Some(palette.green)),
                "lua" => ("", Some(palette.blue)),
                "swift" => ("", Some(palette.red)),
                "rsx" => ("󱘗", Some(palette.orange)),
                _ => ("󰈔", None),
            }
        }
    }
}
