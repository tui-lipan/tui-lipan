//! Built-in theme presets.
//!
//! Each preset defines a complete color scheme from a [`ThemePalette`], then
//! overrides the domain-specific palettes (file icons, git status) with
//! colors that match the original theme specification.

use super::{
    Color, DiffPalette, DocumentPalette, DocumentViewPalette, FileIconPalette, GitStatusPalette,
    HexAreaPalette, InputPalette, SyntaxPalette, TerminalPalette, TextAreaPalette, Theme,
    ThemePalette,
};

impl Theme {
    /// ANSI theme.
    ///
    /// A terminal-native palette built from ANSI named colors for classic,
    /// portable styling across terminals.
    pub fn ansi() -> Self {
        Self {
            primary: super::Style::new().fg(Color::Gray).bg(Color::Black),
            accent: super::Style::new().fg(Color::LightCyan),
            selection: super::Style::new().fg(Color::Black).bg(Color::LightCyan),
            text_selection: super::Style::new().fg(Color::Black).bg(Color::LightCyan),
            focus: super::Style::new().fg(Color::LightCyan),
            focus_decoration: true,
            hover: super::Style::default(),
            border: super::Style::new().fg(Color::DarkGray),
            muted: super::Style::new().fg(Color::DarkGray),
            surface: super::SurfacePalette {
                panel: Color::Black,
                element: Color::Black,
                menu: Color::Black,
                backdrop: Color::Black,
            },
            status: super::StatusPalette {
                success: Color::LightGreen,
                warning: Color::Yellow,
                error: Color::LightRed,
                info: Color::LightBlue,
            },
            border_active: Color::LightCyan,
            file_icons: FileIconPalette {
                azure: Color::LightCyan,
                blue: Color::LightBlue,
                cyan: Color::Cyan,
                green: Color::LightGreen,
                grey: Color::Gray,
                orange: Color::LightYellow,
                purple: Color::LightMagenta,
                red: Color::LightRed,
                yellow: Color::Yellow,
            },
            git_status: GitStatusPalette {
                modified: Color::Yellow,
                added: Color::LightGreen,
                deleted: Color::LightRed,
                renamed: Color::LightBlue,
                untracked: Color::LightMagenta,
                conflicted: Color::LightRed,
            },
            diff: DiffPalette {
                context: super::Style::default(),
                added: super::Style::new().bg(Color::Green),
                removed: super::Style::new().bg(Color::Red),
                empty: super::Style::new().dim(),
                added_word: super::Style::new().bg(Color::Green),
                removed_word: super::Style::new().bg(Color::Red),
                added_marker: super::Style::new().fg(Color::LightGreen),
                removed_marker: super::Style::new().fg(Color::LightRed),
                context_line_number: super::Style::new().fg(Color::DarkGray),
                added_line_number: super::Style::default(),
                removed_line_number: super::Style::default(),
                context_separator_style: super::Style::new().fg(Color::DarkGray).dim(),
                patch_header: super::Style::new().fg(Color::Cyan).bold().dim(),
            },
            document: DocumentPalette {
                heading_styles: [
                    super::Style::new().bold().fg(Color::LightCyan),
                    super::Style::new().bold().fg(Color::LightBlue),
                    super::Style::new().bold().fg(Color::Cyan),
                    super::Style::new().bold().fg(Color::Gray),
                    super::Style::new().bold().fg(Color::Gray),
                    super::Style::new().bold().fg(Color::DarkGray).dim(),
                ],
                code_inline: super::Style::new().fg(Color::LightGreen),
                code_block: super::Style::default(),
                emphasis: super::Style::new().italic(),
                strong: super::Style::new().bold(),
                strikethrough: super::Style::new().strikethrough(),
                link: super::Style::new().fg(Color::LightCyan).underline(),
                blockquote_bar: super::Style::new().fg(Color::DarkGray).dim(),
                table_border: super::Style::new().fg(Color::DarkGray).dim(),
                table_header: super::Style::new().bold(),
                hr: super::Style::new().fg(Color::DarkGray).dim(),
                list_item: super::Style::new().fg(Color::LightBlue).bold(),
                list_enumeration: super::Style::new().fg(Color::LightBlue).bold(),
                diagram_node_fill_style: super::Style::new().bg(Color::Black),
                diagram_node_border_style: super::Style::new().fg(Color::LightCyan),
                diagram_node_label_style: super::Style::new().fg(Color::Gray),
                diagram_edge_style: super::Style::new().fg(Color::LightBlue),
                diagram_muted_style: super::Style::new().fg(Color::DarkGray).dim(),
            },
            syntax: SyntaxPalette {
                comment: super::Style::new().fg(Color::DarkGray).italic().dim(),
                keyword: super::Style::new().fg(Color::Magenta),
                string: super::Style::new().fg(Color::Green),
                number: super::Style::new().fg(Color::Yellow),
                constant: super::Style::new().fg(Color::Yellow).italic(),
                function: super::Style::new().fg(Color::Cyan),
                builtin: super::Style::new().fg(Color::Cyan).italic(),
                type_name: super::Style::new().fg(Color::Cyan),
                variable: super::Style::new().fg(Color::Gray),
                parameter: super::Style::new().fg(Color::DarkGray).italic(),
                operator: super::Style::new().fg(Color::DarkGray),
            },
            input: InputPalette::default(),
            text_area: TextAreaPalette::default(),
            document_view: DocumentViewPalette::default(),
            hex_area: HexAreaPalette {
                focus: super::Style::default(),
                cursor: super::Style::new().fg(Color::LightCyan),
            },
            terminal: TerminalPalette::default(),
            scrollbar: super::ScrollbarPalette {
                track: Some(Color::DarkGray),
                thumb: Color::Gray,
                thumb_focus: Some(Color::LightCyan),
            },
            splitter: super::SplitterPalette {
                hover: Color::Blue,
                active: Color::LightBlue,
            },
            extensions: Default::default(),
        }
    }

    /// One Dark theme (Atom-inspired).
    ///
    /// A popular dark theme with muted colors and good contrast.
    pub fn one_dark() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xABB2BF),
            Color::hex_u24(0x282C34),
            Color::hex_u24(0x61AFEF),
        )
        .success(Color::hex_u24(0x98C379))
        .warning(Color::hex_u24(0xE5C07B))
        .error(Color::hex_u24(0xE06C75))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x61AFEF),
            blue: Color::hex_u24(0x4175E6),
            cyan: Color::hex_u24(0x56B6C2),
            green: Color::hex_u24(0x98C379),
            grey: Color::hex_u24(0xABB2BF),
            orange: Color::hex_u24(0xD19A66),
            purple: Color::hex_u24(0xC678DD),
            red: Color::hex_u24(0xE06C75),
            yellow: Color::hex_u24(0xE5C07B),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xE5B767),
            added: Color::hex_u24(0x7EC699),
            deleted: Color::hex_u24(0xE57E7E),
            renamed: Color::hex_u24(0x76C5E5),
            untracked: Color::hex_u24(0xC59AE5),
            conflicted: Color::hex_u24(0xE57E7E),
        };
        theme
    }

    /// Dracula theme.
    ///
    /// A dark theme with vibrant purple accents.
    pub fn dracula() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xF8F8F2),
            Color::hex_u24(0x282A36),
            Color::hex_u24(0xBD93F9),
        )
        .success(Color::hex_u24(0x50FA7B))
        .warning(Color::hex_u24(0xF1FA8C))
        .error(Color::hex_u24(0xFF5555))
        .info(Color::hex_u24(0x8BE9FD))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x8BE9FD),
            blue: Color::hex_u24(0x6BE5FD),
            cyan: Color::hex_u24(0x8BE9FD),
            green: Color::hex_u24(0x50FA7B),
            grey: Color::hex_u24(0x6C7186),
            orange: Color::hex_u24(0xFFB86C),
            purple: Color::hex_u24(0xBD93F9),
            red: Color::hex_u24(0xFF5555),
            yellow: Color::hex_u24(0xF1FA8C),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xFFB86C),
            added: Color::hex_u24(0x50FA7B),
            deleted: Color::hex_u24(0xFF5555),
            renamed: Color::hex_u24(0x8BE9FD),
            untracked: Color::hex_u24(0xBD93F9),
            conflicted: Color::hex_u24(0xFF5555),
        };

        theme
    }

    /// Nord theme.
    ///
    /// An arctic, north-bluish color palette with cool tones.
    pub fn nord() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xD8DEE9),
            Color::hex_u24(0x2E3440),
            Color::hex_u24(0x88C0D0),
        )
        .success(Color::hex_u24(0xA3BE8C))
        .warning(Color::hex_u24(0xEBCB8B))
        .error(Color::hex_u24(0xBF616A))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x88C0D0),
            blue: Color::hex_u24(0x5E81AC),
            cyan: Color::hex_u24(0x8FBCBB),
            green: Color::hex_u24(0xA3BE8C),
            grey: Color::hex_u24(0x676E7D),
            orange: Color::hex_u24(0xD08770),
            purple: Color::hex_u24(0xB48EAD),
            red: Color::hex_u24(0xBF616A),
            yellow: Color::hex_u24(0xEBCB8B),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xEBCB8B),
            added: Color::hex_u24(0xA3BE8C),
            deleted: Color::hex_u24(0xBF616A),
            renamed: Color::hex_u24(0x88C0D0),
            untracked: Color::hex_u24(0xB48EAD),
            conflicted: Color::hex_u24(0xBF616A),
        };

        theme
    }

    /// Gruvbox theme (dark variant).
    ///
    /// A retro groove color scheme with warm earthy tones.
    pub fn gruvbox() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xEBDBB2),
            Color::hex_u24(0x282828),
            Color::hex_u24(0xFE8019),
        )
        .success(Color::hex_u24(0xB8BB26))
        .warning(Color::hex_u24(0xFABD2F))
        .error(Color::hex_u24(0xFB4934))
        .info(Color::hex_u24(0x83A598))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x83A598),
            blue: Color::hex_u24(0x458588),
            cyan: Color::hex_u24(0x8EC07C),
            green: Color::hex_u24(0xB8BB26),
            grey: Color::hex_u24(0x928374),
            orange: Color::hex_u24(0xFE8019),
            purple: Color::hex_u24(0xD3869B),
            red: Color::hex_u24(0xFB4934),
            yellow: Color::hex_u24(0xFABD2F),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xFABD2F),
            added: Color::hex_u24(0xB8BB26),
            deleted: Color::hex_u24(0xFB4934),
            renamed: Color::hex_u24(0x83A598),
            untracked: Color::hex_u24(0xD3869B),
            conflicted: Color::hex_u24(0xFB4934),
        };

        theme
    }

    /// Catppuccin Mocha theme.
    ///
    /// A soothing pastel theme with good contrast and soft colors.
    pub fn catppuccin() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xCDD6F4),
            Color::hex_u24(0x1E1E2E),
            Color::hex_u24(0xCBA6F7),
        )
        .success(Color::hex_u24(0xA6E3A1))
        .warning(Color::hex_u24(0xF9E2AF))
        .error(Color::hex_u24(0xF38BA8))
        .info(Color::hex_u24(0x89B4FA))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x89DCEB),
            blue: Color::hex_u24(0x89B4FA),
            cyan: Color::hex_u24(0x94E2D5),
            green: Color::hex_u24(0xA6E3A1),
            grey: Color::hex_u24(0x6C7086),
            orange: Color::hex_u24(0xFAB387),
            purple: Color::hex_u24(0xCBA6F7),
            red: Color::hex_u24(0xF38BA8),
            yellow: Color::hex_u24(0xF9E2AF),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xF9E2AF),
            added: Color::hex_u24(0xA6E3A1),
            deleted: Color::hex_u24(0xF38BA8),
            renamed: Color::hex_u24(0x89B4FA),
            untracked: Color::hex_u24(0xCBA6F7),
            conflicted: Color::hex_u24(0xF38BA8),
        };

        theme
    }

    /// Tokyo Night theme.
    ///
    /// A clean dark theme inspired by Tokyo city lights.
    pub fn tokyo_night() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xC0CAF5),
            Color::hex_u24(0x1A1B26),
            Color::hex_u24(0x7AA2F7),
        )
        .success(Color::hex_u24(0x9ECE6A))
        .warning(Color::hex_u24(0xE0AF68))
        .error(Color::hex_u24(0xF7768E))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x7DCFFF),
            blue: Color::hex_u24(0x7AA2F7),
            cyan: Color::hex_u24(0x7DCFFF),
            green: Color::hex_u24(0x9ECE6A),
            grey: Color::hex_u24(0x565F89),
            orange: Color::hex_u24(0xFF9E64),
            purple: Color::hex_u24(0xBB9AF7),
            red: Color::hex_u24(0xF7768E),
            yellow: Color::hex_u24(0xE0AF68),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xE0AF68),
            added: Color::hex_u24(0x9ECE6A),
            deleted: Color::hex_u24(0xF7768E),
            renamed: Color::hex_u24(0x7DCFFF),
            untracked: Color::hex_u24(0xBB9AF7),
            conflicted: Color::hex_u24(0xF7768E),
        };

        theme
    }

    /// Solarized Dark theme.
    ///
    /// A precision color scheme with both dark and light variants.
    pub fn solarized_dark() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0x93A1A1),
            Color::hex_u24(0x002B36),
            Color::hex_u24(0x268BD2),
        )
        .success(Color::hex_u24(0x859900))
        .warning(Color::hex_u24(0xB58900))
        .error(Color::hex_u24(0xDC322F))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x268BD2),
            blue: Color::hex_u24(0x268BD2),
            cyan: Color::hex_u24(0x2AA198),
            green: Color::hex_u24(0x859900),
            grey: Color::hex_u24(0x657B83),
            orange: Color::hex_u24(0xCB4B16),
            purple: Color::hex_u24(0x6C71C4),
            red: Color::hex_u24(0xDC322F),
            yellow: Color::hex_u24(0xB58900),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xB58900),
            added: Color::hex_u24(0x859900),
            deleted: Color::hex_u24(0xDC322F),
            renamed: Color::hex_u24(0x268BD2),
            untracked: Color::hex_u24(0x6C71C4),
            conflicted: Color::hex_u24(0xDC322F),
        };

        theme
    }

    /// Monokai Pro theme.
    ///
    /// A modern take on the classic Monokai theme.
    pub fn monokai() -> Self {
        let mut theme: Self = ThemePalette::new(
            Color::hex_u24(0xF8F8F2),
            Color::hex_u24(0x2D2A2E),
            Color::hex_u24(0xFFD866),
        )
        .success(Color::hex_u24(0xA6E22E))
        .warning(Color::hex_u24(0xFD971F))
        .error(Color::hex_u24(0xF92672))
        .info(Color::hex_u24(0x66D9EF))
        .into();

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x66D9EF),
            blue: Color::hex_u24(0x66D9EF),
            cyan: Color::hex_u24(0x66D9EF),
            green: Color::hex_u24(0xA6E22E),
            grey: Color::hex_u24(0x75715E),
            orange: Color::hex_u24(0xFD971F),
            purple: Color::hex_u24(0xAE81FF),
            red: Color::hex_u24(0xF92672),
            yellow: Color::hex_u24(0xFFD866),
        };
        theme.git_status = GitStatusPalette {
            modified: Color::hex_u24(0xFFD866),
            added: Color::hex_u24(0xA6E22E),
            deleted: Color::hex_u24(0xF92672),
            renamed: Color::hex_u24(0x66D9EF),
            untracked: Color::hex_u24(0xAE81FF),
            conflicted: Color::hex_u24(0xF92672),
        };

        theme
    }

    /// The signature **tui-lipan** theme.
    ///
    /// A deep, near-black cool background with a soft cyan-grey foreground and a
    /// violet accent (`#C084FC`), accented by a richer purple (`#9333EA`) for
    /// selection. Surfaces carry a faint teal-navy tint so panels and popovers
    /// layer cleanly. This is the project's "business card" look.
    pub fn lipan() -> Self {
        let text = Color::hex_u24(0xA8CDD8);
        let background = Color::hex_u24(0x04090D);
        let accent = Color::hex_u24(0xC084FC);
        let deep_purple = Color::hex_u24(0x9333EA);

        let success = Color::hex_u24(0x4ADE80);
        let warning = Color::hex_u24(0xFBBF24);
        let error = Color::hex_u24(0xFB7185);
        let info = Color::hex_u24(0x67E8F9);

        let mut theme: Self = ThemePalette::new(text, background, accent)
            .success(success)
            .warning(warning)
            .error(error)
            .info(info)
            .into();

        // Signature selection: a deep violet field with a light violet glyph so
        // selected rows read as the brand moment without sacrificing legibility.
        let selection_bg = background.blend_toward(deep_purple, 0.30);
        theme.selection = super::Style::new()
            .fg(Color::hex_u24(0xEBD9FF))
            .bg(selection_bg);
        theme.text_selection = super::Style::new()
            .fg(Color::hex_u24(0xEBD9FF))
            .bg(background.blend_toward(deep_purple, 0.24));

        // Teal-navy tinted surfaces (anchored on the brand `#081217`).
        theme.surface = super::SurfacePalette {
            backdrop: background,
            element: Color::hex_u24(0x060E13),
            panel: Color::hex_u24(0x081217),
            menu: Color::hex_u24(0x0C1B24),
        };

        // A cool, slightly violet-leaning structural border.
        theme.border = super::Style::new().fg(Color::hex_u24(0x324553));

        theme.scrollbar = super::ScrollbarPalette {
            track: Some(Color::hex_u24(0x0C1B24)),
            thumb: Color::hex_u24(0x33424E),
            thumb_focus: Some(accent),
        };
        theme.splitter = super::SplitterPalette {
            hover: accent,
            active: Color::hex_u24(0xD8B4FE),
        };

        theme.file_icons = FileIconPalette {
            azure: Color::hex_u24(0x7DD3FC),
            blue: Color::hex_u24(0x818CF8),
            cyan: info,
            green: success,
            grey: Color::hex_u24(0x64798A),
            orange: Color::hex_u24(0xF6A96B),
            purple: accent,
            red: error,
            yellow: warning,
        };
        theme.git_status = GitStatusPalette {
            modified: warning,
            added: success,
            deleted: error,
            renamed: info,
            untracked: accent,
            conflicted: error,
        };

        theme
    }
}

fn normalize_name(name: &str) -> String {
    name.trim()
        .chars()
        .filter(|c| !matches!(c, '-' | '_' | ' '))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Return a built-in theme preset by normalized name.
///
/// Name matching is case-insensitive and ignores `-`, `_`, and spaces.
pub fn preset_by_name(name: &str) -> Option<Theme> {
    match normalize_name(name).as_str() {
        "onedark" => Some(Theme::one_dark()),
        "dracula" => Some(Theme::dracula()),
        "nord" => Some(Theme::nord()),
        "gruvbox" => Some(Theme::gruvbox()),
        "catppuccin" => Some(Theme::catppuccin()),
        "ansi" => Some(Theme::ansi()),
        "tokyonight" => Some(Theme::tokyo_night()),
        "solarizeddark" => Some(Theme::solarized_dark()),
        "monokai" => Some(Theme::monokai()),
        "lipan" => Some(Theme::lipan()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Color, Theme, preset_by_name};

    #[test]
    fn preset_by_name_resolves_lipan() {
        assert_eq!(preset_by_name("lipan"), Some(Theme::lipan()));
        assert_eq!(preset_by_name("Lipan"), Some(Theme::lipan()));
    }

    #[test]
    fn preset_by_name_does_not_resolve_system() {
        assert_eq!(preset_by_name("system"), None);
    }

    #[test]
    fn lipan_uses_the_brand_tokens() {
        let theme = Theme::lipan();
        assert_eq!(
            theme.primary.fg.map(|p| p.color()),
            Some(Color::hex_u24(0xA8CDD8))
        );
        assert_eq!(
            theme.primary.bg.map(|p| p.color()),
            Some(Color::hex_u24(0x04090D))
        );
        assert_eq!(
            theme.accent.fg.map(|p| p.color()),
            Some(Color::hex_u24(0xC084FC))
        );
    }

    #[test]
    fn lipan_surfaces_are_distinct_and_elevated() {
        let s = Theme::lipan().surface;
        // Each elevation step is a strictly different, lighter surface than the backdrop.
        assert!(s.backdrop != s.element && s.element != s.panel && s.panel != s.menu);
        assert!(s.backdrop.luminance() < s.element.luminance());
        assert!(s.element.luminance() < s.panel.luminance());
        assert!(s.panel.luminance() < s.menu.luminance());
    }

    #[test]
    fn derived_surfaces_elevate_on_light_backgrounds() {
        // A light-background palette must darken surfaces (panel below backdrop
        // luminance), not lighten into invisibility.
        let theme = Theme::custom(
            Color::hex_u24(0x24292F),
            Color::hex_u24(0xFFFFFF),
            Color::hex_u24(0x0969DA),
        );
        let s = theme.surface;
        assert!(s.panel.luminance() < s.backdrop.luminance());
        assert!(s.menu.luminance() < s.panel.luminance());
    }
}
