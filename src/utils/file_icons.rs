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

/// Resolve the Nerd Font folder glyph and semantic color.
///
/// Returns the bare folder icon only (no disclosure arrow). Expanded uses
/// `U+E5FE` (``); collapsed uses `U+E5FF` (``). [`FileTree`] prefixes its
/// own expand/collapse arrows when needed.
///
/// ```
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::directory_icon;
///
/// let palette = FileIconPalette::default();
/// let (glyph, color) = directory_icon(false, &palette);
/// assert_eq!(glyph, "\u{e5ff}");
/// assert_eq!(color, Some(palette.blue));
/// ```
///
/// [`FileTree`]: crate::widgets::FileTree
pub fn directory_icon(expanded: bool, palette: &FileIconPalette) -> (&'static str, Option<Color>) {
    let glyph = if expanded {
        "\u{e5fe}" // 
    } else {
        "\u{e5ff}" // 
    };
    (glyph, Some(palette.blue))
}

/// Build a [`Span`] with the directory Nerd Font icon, colored from `palette`.
///
/// Convenience wrapper over [`directory_icon`]:
///
/// ```
/// use tui_lipan::prelude::*;
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::directory_icon_span;
///
/// let palette = FileIconPalette::default();
/// let _item = ListItem::from_spans([
///     directory_icon_span(false, &palette),
///     Span::new(" src"),
/// ]);
/// ```
pub fn directory_icon_span(expanded: bool, palette: &FileIconPalette) -> Span {
    let (glyph, color) = directory_icon(expanded, palette);
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
        "license" | "license.md" | "license.txt" => ("", Some(palette.yellow)),
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
                "txt" => ("󰈙", Some(palette.grey)),

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
                "diff" | "patch" => ("", Some(palette.orange)),
                "xml" => ("󰗀", Some(palette.orange)),
                "zig" => ("", Some(palette.orange)),
                "nix" => ("", Some(palette.cyan)),
                "cs" => ("󰌛", Some(palette.purple)),
                "php" => ("", Some(palette.purple)),
                "ex" | "exs" => ("", Some(palette.purple)),
                "hs" | "lhs" => ("", Some(palette.purple)),
                "csv" => ("", Some(palette.green)),
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

const GENERIC_FILE_ICON: &str = "󰈔";

/// Resolve the Nerd Font glyph and semantic color for a language tag
/// (such as a Markdown fenced code block info string like `"rust"`, `"sh"`,
/// `"typescript"`, or `"diff"`).
///
/// Delegates to [`file_icon`] so code blocks, [`FileTree`], and tabs share one
/// canonical icon table and [`FileIconPalette`]. Returns `None` when `language`
/// is empty or does not match a known language/extension (rather than falling
/// back to a generic file icon).
///
/// ```
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::language_icon;
///
/// let palette = FileIconPalette::default();
/// let (glyph, color) = language_icon("rust", &palette).expect("rust icon");
/// assert_eq!(glyph, "\u{e68b}");
/// assert_eq!(color, Some(palette.red));
/// assert!(language_icon("unknown-lang", &palette).is_none());
/// ```
///
/// [`FileTree`]: crate::widgets::FileTree
pub fn language_icon(
    language: &str,
    palette: &FileIconPalette,
) -> Option<(&'static str, Option<Color>)> {
    let token = language
        .split(|c: char| c.is_whitespace() || c == ',')
        .next()
        .unwrap_or("")
        .trim()
        .trim_start_matches('.');
    if token.is_empty() {
        return None;
    }

    let lower = token.to_ascii_lowercase();
    let sample_name: Option<&'static str> = match lower.as_str() {
        "rust" | "rs" => Some("a.rs"),
        "toml" => Some("a.toml"),
        "python" | "py" | "py3" | "python3" | "pyi" => Some("a.py"),
        "javascript" | "js" | "mjs" | "cjs" | "jsx" => Some("a.js"),
        "typescript" | "ts" => Some("a.ts"),
        "tsx" => Some("a.tsx"),
        "html" | "htm" => Some("a.html"),
        "css" => Some("a.css"),
        "scss" | "sass" => Some("a.scss"),
        "less" => Some("a.less"),
        "json" | "jsonc" | "json5" => Some("a.json"),
        "yaml" | "yml" => Some("a.yaml"),
        "c" => Some("a.c"),
        "cpp" | "c++" | "cc" | "cxx" => Some("a.cpp"),
        "h" | "hpp" => Some("a.h"),
        "go" | "golang" => Some("a.go"),
        "ruby" | "rb" | "erb" => Some("a.rb"),
        "sh" | "bash" | "zsh" | "fish" | "shell" | "console" | "shellsession" => Some("a.sh"),
        "powershell" | "ps1" | "pwsh" => Some("a.ps1"),
        "java" => Some("a.java"),
        "gradle" => Some("a.gradle"),
        "kotlin" | "kt" | "kts" => Some("a.kt"),
        "markdown" | "md" | "rst" => Some("a.md"),
        "text" | "txt" | "plain" | "plaintext" => Some("a.txt"),
        "dockerfile" | "docker" => Some("Dockerfile"),
        "makefile" | "make" => Some("Makefile"),
        "cmake" => Some("CMakeLists.txt"),
        "git" | "gitignore" => Some(".gitignore"),
        "sql" | "sqlite" | "sqlite3" | "postgres" | "postgresql" | "mysql" => Some("a.sql"),
        "ini" | "conf" | "cfg" | "config" => Some("a.ini"),
        "log" => Some("a.log"),
        "vim" | "viml" | "vimscript" | "nvim" => Some("a.vim"),
        "lua" => Some("a.lua"),
        "swift" => Some("a.swift"),
        "rsx" => Some("a.rsx"),
        "diff" | "patch" => Some("a.diff"),
        "xml" => Some("a.xml"),
        "svg" => Some("a.svg"),
        "zig" => Some("a.zig"),
        "nix" => Some("a.nix"),
        "csharp" | "c#" | "cs" => Some("a.cs"),
        "php" => Some("a.php"),
        "elixir" | "ex" | "exs" => Some("a.ex"),
        "haskell" | "hs" | "lhs" => Some("a.hs"),
        "csv" => Some("a.csv"),
        _ => None,
    };

    let resolved = if let Some(name) = sample_name {
        file_icon(name, palette)
    } else if !lower.contains('/') && !lower.contains('\\') {
        let probe = format!("a.{lower}");
        file_icon(&probe, palette)
    } else {
        return None;
    };

    (resolved.0 != GENERIC_FILE_ICON || resolved.1.is_some()).then_some(resolved)
}

/// Build a [`Span`] with the language's Nerd Font icon, colored from `palette`.
///
/// Convenience wrapper over [`language_icon`]. Returns `None` when no icon is
/// registered for `language`.
///
/// ```
/// use tui_lipan::style::FileIconPalette;
/// use tui_lipan::utils::language_icon_span;
///
/// let palette = FileIconPalette::default();
/// let span = language_icon_span("rust", &palette).expect("rust icon span");
/// assert_eq!(span.content.as_ref(), "\u{e68b}");
/// assert!(language_icon_span("unknown-lang", &palette).is_none());
/// ```
pub fn language_icon_span(language: &str, palette: &FileIconPalette) -> Option<Span> {
    let (glyph, color) = language_icon(language, palette)?;
    let mut span = Span::new(glyph);
    if let Some(color) = color {
        span = span.fg(color);
    }
    Some(span)
}

#[cfg(test)]
mod tests {
    use super::{file_icon, language_icon, language_icon_span};
    use crate::style::FileIconPalette;

    #[test]
    fn language_icon_maps_common_fence_tags_via_file_icon() {
        let palette = FileIconPalette::default();
        assert_eq!(
            language_icon("rust", &palette),
            Some(file_icon("main.rs", &palette))
        );
        assert_eq!(
            language_icon("rs", &palette),
            Some(file_icon("main.rs", &palette))
        );
        assert_eq!(
            language_icon("Rust,no_run", &palette),
            Some(file_icon("main.rs", &palette))
        );
        assert_eq!(
            language_icon("bash", &palette),
            Some(file_icon("run.sh", &palette))
        );
        assert_eq!(
            language_icon("shell", &palette),
            Some(file_icon("run.sh", &palette))
        );
        assert_eq!(
            language_icon("typescript", &palette),
            Some(file_icon("app.ts", &palette))
        );
        assert_eq!(
            language_icon("dockerfile", &palette),
            Some(file_icon("Dockerfile", &palette))
        );
        assert_eq!(
            language_icon("diff", &palette),
            Some(file_icon("changes.diff", &palette))
        );
        assert!(language_icon("", &palette).is_none());
        assert!(language_icon("   ", &palette).is_none());
        assert!(language_icon("not-a-real-language", &palette).is_none());
    }

    #[test]
    fn language_icon_span_colors_from_palette() {
        let palette = FileIconPalette::default();
        let span = language_icon_span("python", &palette).expect("python icon");
        assert_eq!(span.content.as_ref(), file_icon("a.py", &palette).0);
        assert_eq!(span.style.fg.map(|p| p.color()), Some(palette.yellow));
    }
}
