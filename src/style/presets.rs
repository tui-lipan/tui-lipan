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
        let mut theme = PresetColors {
            text: Color::hex_u24(0xABB2BF),
            bg: Color::hex_u24(0x282C34),
            accent: Color::hex_u24(0x61AFEF),
            success: Color::hex_u24(0x98C379),
            warning: Color::hex_u24(0xE5C07B),
            error: Color::hex_u24(0xE06C75),
            info: Color::hex_u24(0x61AFEF),
            azure: Color::hex_u24(0x61AFEF),
            blue: Color::hex_u24(0x4175E6),
            cyan: Color::hex_u24(0x56B6C2),
            grey: Color::hex_u24(0xABB2BF),
            orange: Color::hex_u24(0xD19A66),
            purple: Color::hex_u24(0xC678DD),
        }
        .into_theme();

        // One Dark softens every git color away from its syntax counterpart
        // rather than reusing the status palette, so all six slots are its own.
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
        let mut theme = PresetColors {
            text: Color::hex_u24(0xF8F8F2),
            bg: Color::hex_u24(0x282A36),
            accent: Color::hex_u24(0xBD93F9),
            success: Color::hex_u24(0x50FA7B),
            warning: Color::hex_u24(0xF1FA8C),
            error: Color::hex_u24(0xFF5555),
            info: Color::hex_u24(0x8BE9FD),
            azure: Color::hex_u24(0x8BE9FD),
            blue: Color::hex_u24(0x6BE5FD),
            cyan: Color::hex_u24(0x8BE9FD),
            grey: Color::hex_u24(0x6C7186),
            orange: Color::hex_u24(0xFFB86C),
            purple: Color::hex_u24(0xBD93F9),
        }
        .into_theme();

        // Dracula's warning is an acid yellow that reads as "new", not
        // "changed"; its orange carries modification.
        theme.git_status.modified = Color::hex_u24(0xFFB86C);

        theme
    }

    /// Nord theme.
    ///
    /// An arctic, north-bluish color palette with cool tones.
    pub fn nord() -> Self {
        PresetColors {
            text: Color::hex_u24(0xD8DEE9),
            bg: Color::hex_u24(0x2E3440),
            accent: Color::hex_u24(0x88C0D0),
            success: Color::hex_u24(0xA3BE8C),
            warning: Color::hex_u24(0xEBCB8B),
            error: Color::hex_u24(0xBF616A),
            info: Color::hex_u24(0x88C0D0),
            azure: Color::hex_u24(0x88C0D0),
            blue: Color::hex_u24(0x5E81AC),
            cyan: Color::hex_u24(0x8FBCBB),
            grey: Color::hex_u24(0x676E7D),
            orange: Color::hex_u24(0xD08770),
            purple: Color::hex_u24(0xB48EAD),
        }
        .into_theme()
    }

    /// Gruvbox theme (dark variant).
    ///
    /// A retro groove color scheme with warm earthy tones.
    pub fn gruvbox() -> Self {
        PresetColors {
            text: Color::hex_u24(0xEBDBB2),
            bg: Color::hex_u24(0x282828),
            accent: Color::hex_u24(0xFE8019),
            success: Color::hex_u24(0xB8BB26),
            warning: Color::hex_u24(0xFABD2F),
            error: Color::hex_u24(0xFB4934),
            info: Color::hex_u24(0x83A598),
            azure: Color::hex_u24(0x83A598),
            blue: Color::hex_u24(0x458588),
            cyan: Color::hex_u24(0x8EC07C),
            grey: Color::hex_u24(0x928374),
            orange: Color::hex_u24(0xFE8019),
            purple: Color::hex_u24(0xD3869B),
        }
        .into_theme()
    }

    /// Catppuccin Mocha theme.
    ///
    /// A soothing pastel theme with good contrast and soft colors.
    pub fn catppuccin() -> Self {
        let mut theme = PresetColors {
            text: Color::hex_u24(0xCDD6F4),
            bg: Color::hex_u24(0x1E1E2E),
            accent: Color::hex_u24(0xCBA6F7),
            success: Color::hex_u24(0xA6E3A1),
            warning: Color::hex_u24(0xF9E2AF),
            error: Color::hex_u24(0xF38BA8),
            info: Color::hex_u24(0x89B4FA),
            azure: Color::hex_u24(0x89DCEB),
            blue: Color::hex_u24(0x89B4FA),
            cyan: Color::hex_u24(0x94E2D5),
            grey: Color::hex_u24(0x6C7086),
            orange: Color::hex_u24(0xFAB387),
            purple: Color::hex_u24(0xCBA6F7),
        }
        .into_theme();

        // Catppuccin renames with its blue rather than its lighter sky.
        theme.git_status.renamed = Color::hex_u24(0x89B4FA);

        theme
    }

    /// Tokyo Night theme.
    ///
    /// A clean dark theme inspired by Tokyo city lights.
    pub fn tokyo_night() -> Self {
        PresetColors {
            text: Color::hex_u24(0xC0CAF5),
            bg: Color::hex_u24(0x1A1B26),
            accent: Color::hex_u24(0x7AA2F7),
            success: Color::hex_u24(0x9ECE6A),
            warning: Color::hex_u24(0xE0AF68),
            error: Color::hex_u24(0xF7768E),
            info: Color::hex_u24(0x7AA2F7),
            azure: Color::hex_u24(0x7DCFFF),
            blue: Color::hex_u24(0x7AA2F7),
            cyan: Color::hex_u24(0x7DCFFF),
            grey: Color::hex_u24(0x565F89),
            orange: Color::hex_u24(0xFF9E64),
            purple: Color::hex_u24(0xBB9AF7),
        }
        .into_theme()
    }

    /// Solarized Dark theme.
    ///
    /// A precision color scheme with both dark and light variants.
    pub fn solarized_dark() -> Self {
        PresetColors {
            text: Color::hex_u24(0x93A1A1),
            bg: Color::hex_u24(0x002B36),
            accent: Color::hex_u24(0x268BD2),
            success: Color::hex_u24(0x859900),
            warning: Color::hex_u24(0xB58900),
            error: Color::hex_u24(0xDC322F),
            info: Color::hex_u24(0x268BD2),
            azure: Color::hex_u24(0x268BD2),
            blue: Color::hex_u24(0x268BD2),
            cyan: Color::hex_u24(0x2AA198),
            grey: Color::hex_u24(0x657B83),
            orange: Color::hex_u24(0xCB4B16),
            purple: Color::hex_u24(0x6C71C4),
        }
        .into_theme()
    }

    /// Monokai Pro theme.
    ///
    /// A modern take on the classic Monokai theme.
    pub fn monokai() -> Self {
        let mut theme = PresetColors {
            text: Color::hex_u24(0xF8F8F2),
            bg: Color::hex_u24(0x2D2A2E),
            accent: Color::hex_u24(0xFFD866),
            success: Color::hex_u24(0xA6E22E),
            warning: Color::hex_u24(0xFD971F),
            error: Color::hex_u24(0xF92672),
            info: Color::hex_u24(0x66D9EF),
            azure: Color::hex_u24(0x66D9EF),
            blue: Color::hex_u24(0x66D9EF),
            cyan: Color::hex_u24(0x66D9EF),
            grey: Color::hex_u24(0x75715E),
            orange: Color::hex_u24(0xFD971F),
            purple: Color::hex_u24(0xAE81FF),
        }
        .into_theme();

        // Monokai is the one preset whose accent *is* its yellow, leaving
        // `warning` as the orange. File icons and git both want the yellow.
        theme.file_icons.yellow = Color::hex_u24(0xFFD866);
        theme.git_status.modified = Color::hex_u24(0xFFD866);

        theme
    }

    /// The signature **tui-lipan** theme.
    ///
    /// A deep, near-black cool background with a soft cyan-grey foreground and a
    /// violet accent (`#C084FC`), accented by a richer purple (`#9333EA`) for
    /// selection. Surfaces carry a faint teal-navy tint so panels and popovers
    /// layer cleanly. This is the project's "business card" look.
    pub fn lipan() -> Self {
        let background = Color::hex_u24(0x04090D);
        let accent = Color::hex_u24(0xC084FC);
        let deep_purple = Color::hex_u24(0x9333EA);
        let info = Color::hex_u24(0x67E8F9);

        let mut theme = PresetColors {
            text: Color::hex_u24(0xA8CDD8),
            bg: background,
            accent,
            success: Color::hex_u24(0x4ADE80),
            warning: Color::hex_u24(0xFBBF24),
            error: Color::hex_u24(0xFB7185),
            info,
            azure: Color::hex_u24(0x7DD3FC),
            blue: Color::hex_u24(0x818CF8),
            cyan: info,
            grey: Color::hex_u24(0x64798A),
            orange: Color::hex_u24(0xF6A96B),
            purple: accent,
        }
        .into_theme();

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

        // Renames read on the brand cyan rather than the lighter sky blue.
        theme.git_status.renamed = info;

        theme
    }

    // ── Light variants of the existing dark presets ─────────────────────────

    /// Solarized Light theme.
    ///
    /// The light half of Ethan Schoonover's Solarized, on the `base3` paper
    /// background.
    pub fn solarized_light() -> Self {
        PresetColors {
            text: Color::hex_u24(0x657B83),
            bg: Color::hex_u24(0xFDF6E3),
            accent: Color::hex_u24(0x268BD2),
            success: Color::hex_u24(0x859900),
            warning: Color::hex_u24(0xB58900),
            error: Color::hex_u24(0xDC322F),
            info: Color::hex_u24(0x2AA198),
            azure: Color::hex_u24(0x268BD2),
            blue: Color::hex_u24(0x268BD2),
            cyan: Color::hex_u24(0x2AA198),
            grey: Color::hex_u24(0x93A1A1),
            orange: Color::hex_u24(0xCB4B16),
            purple: Color::hex_u24(0x6C71C4),
        }
        .into_theme()
    }

    /// Gruvbox Light theme.
    ///
    /// The light background variant of Pavel Pertsev's Gruvbox.
    pub fn gruvbox_light() -> Self {
        PresetColors {
            text: Color::hex_u24(0x3C3836),
            bg: Color::hex_u24(0xFBF1C7),
            accent: Color::hex_u24(0x076678),
            success: Color::hex_u24(0x79740E),
            warning: Color::hex_u24(0xB57614),
            error: Color::hex_u24(0x9D0006),
            info: Color::hex_u24(0x427B58),
            azure: Color::hex_u24(0x427B58),
            blue: Color::hex_u24(0x076678),
            cyan: Color::hex_u24(0x689D6A),
            grey: Color::hex_u24(0x7C6F64),
            orange: Color::hex_u24(0xAF3A03),
            purple: Color::hex_u24(0x8F3F71),
        }
        .into_theme()
    }

    /// Tokyo Night Day theme.
    ///
    /// The light sibling of Tokyo Night.
    pub fn tokyo_night_day() -> Self {
        PresetColors {
            text: Color::hex_u24(0x3760BF),
            bg: Color::hex_u24(0xE1E2E7),
            accent: Color::hex_u24(0x2E7DE9),
            success: Color::hex_u24(0x587539),
            warning: Color::hex_u24(0x8C6C3E),
            error: Color::hex_u24(0xF52A65),
            info: Color::hex_u24(0x007197),
            azure: Color::hex_u24(0x007197),
            blue: Color::hex_u24(0x2E7DE9),
            cyan: Color::hex_u24(0x007197),
            grey: Color::hex_u24(0x848CB5),
            orange: Color::hex_u24(0xB15C00),
            purple: Color::hex_u24(0x9854F1),
        }
        .into_theme()
    }

    // ── Catppuccin flavor parity ────────────────────────────────────────────

    /// Catppuccin Latte theme.
    ///
    /// The light flavor of Catppuccin. See also [`Theme::catppuccin`] (Mocha).
    pub fn catppuccin_latte() -> Self {
        let mut theme = PresetColors {
            text: Color::hex_u24(0x4C4F69),
            bg: Color::hex_u24(0xEFF1F5),
            accent: Color::hex_u24(0x8839EF),
            success: Color::hex_u24(0x40A02B),
            warning: Color::hex_u24(0xDF8E1D),
            error: Color::hex_u24(0xD20F39),
            info: Color::hex_u24(0x1E66F5),
            azure: Color::hex_u24(0x04A5E5),
            blue: Color::hex_u24(0x1E66F5),
            cyan: Color::hex_u24(0x179299),
            grey: Color::hex_u24(0x9CA0B0),
            orange: Color::hex_u24(0xFE640B),
            purple: Color::hex_u24(0x8839EF),
        }
        .into_theme();

        // Catppuccin renames with its blue rather than its lighter sky.
        theme.git_status.renamed = Color::hex_u24(0x1E66F5);

        theme
    }

    /// Catppuccin Frappé theme.
    ///
    /// The mid-dark flavor of Catppuccin.
    pub fn catppuccin_frappe() -> Self {
        let mut theme = PresetColors {
            text: Color::hex_u24(0xC6D0F5),
            bg: Color::hex_u24(0x303446),
            accent: Color::hex_u24(0xCA9EE6),
            success: Color::hex_u24(0xA6D189),
            warning: Color::hex_u24(0xE5C890),
            error: Color::hex_u24(0xE78284),
            info: Color::hex_u24(0x8CAAEE),
            azure: Color::hex_u24(0x99D1DB),
            blue: Color::hex_u24(0x8CAAEE),
            cyan: Color::hex_u24(0x81C8BE),
            grey: Color::hex_u24(0x737994),
            orange: Color::hex_u24(0xEF9F76),
            purple: Color::hex_u24(0xCA9EE6),
        }
        .into_theme();

        // Catppuccin renames with its blue rather than its lighter sky.
        theme.git_status.renamed = Color::hex_u24(0x8CAAEE);

        theme
    }

    /// Catppuccin Macchiato theme.
    ///
    /// The dark flavor of Catppuccin, one step warmer than Mocha.
    pub fn catppuccin_macchiato() -> Self {
        let mut theme = PresetColors {
            text: Color::hex_u24(0xCAD3F5),
            bg: Color::hex_u24(0x24273A),
            accent: Color::hex_u24(0xC6A0F6),
            success: Color::hex_u24(0xA6DA95),
            warning: Color::hex_u24(0xEED49F),
            error: Color::hex_u24(0xED8796),
            info: Color::hex_u24(0x8AADF4),
            azure: Color::hex_u24(0x91D7E3),
            blue: Color::hex_u24(0x8AADF4),
            cyan: Color::hex_u24(0x8BD5CA),
            grey: Color::hex_u24(0x6E738D),
            orange: Color::hex_u24(0xF5A97F),
            purple: Color::hex_u24(0xC6A0F6),
        }
        .into_theme();

        // Catppuccin renames with its blue rather than its lighter sky.
        theme.git_status.renamed = Color::hex_u24(0x8AADF4);

        theme
    }

    // ── Rosé Pine ───────────────────────────────────────────────────────────

    /// Rosé Pine theme.
    ///
    /// The default "main" variant: all natural pine, faux fur and a bit of soho
    /// vibes.
    pub fn rose_pine() -> Self {
        PresetColors {
            text: Color::hex_u24(0xE0DEF4),
            bg: Color::hex_u24(0x191724),
            accent: Color::hex_u24(0xC4A7E7),
            success: Color::hex_u24(0x9CCFD8),
            warning: Color::hex_u24(0xF6C177),
            error: Color::hex_u24(0xEB6F92),
            info: Color::hex_u24(0x31748F),
            azure: Color::hex_u24(0x9CCFD8),
            blue: Color::hex_u24(0x31748F),
            cyan: Color::hex_u24(0x31748F),
            grey: Color::hex_u24(0x6E6A86),
            orange: Color::hex_u24(0xEBBCBA),
            purple: Color::hex_u24(0xC4A7E7),
        }
        .into_theme()
    }

    /// Rosé Pine Moon theme.
    ///
    /// The mid-dark variant of Rosé Pine.
    pub fn rose_pine_moon() -> Self {
        PresetColors {
            text: Color::hex_u24(0xE0DEF4),
            bg: Color::hex_u24(0x232136),
            accent: Color::hex_u24(0xC4A7E7),
            success: Color::hex_u24(0x9CCFD8),
            warning: Color::hex_u24(0xF6C177),
            error: Color::hex_u24(0xEB6F92),
            info: Color::hex_u24(0x3E8FB0),
            azure: Color::hex_u24(0x9CCFD8),
            blue: Color::hex_u24(0x3E8FB0),
            cyan: Color::hex_u24(0x3E8FB0),
            grey: Color::hex_u24(0x6E6A86),
            orange: Color::hex_u24(0xEA9A97),
            purple: Color::hex_u24(0xC4A7E7),
        }
        .into_theme()
    }

    /// Rosé Pine Dawn theme.
    ///
    /// The light variant of Rosé Pine.
    pub fn rose_pine_dawn() -> Self {
        PresetColors {
            text: Color::hex_u24(0x575279),
            bg: Color::hex_u24(0xFAF4ED),
            accent: Color::hex_u24(0x907AA9),
            success: Color::hex_u24(0x56949F),
            warning: Color::hex_u24(0xEA9D34),
            error: Color::hex_u24(0xB4637A),
            info: Color::hex_u24(0x286983),
            azure: Color::hex_u24(0x56949F),
            blue: Color::hex_u24(0x286983),
            cyan: Color::hex_u24(0x286983),
            grey: Color::hex_u24(0x9893A5),
            orange: Color::hex_u24(0xD7827E),
            purple: Color::hex_u24(0x907AA9),
        }
        .into_theme()
    }

    // ── Additional community favorites ──────────────────────────────────────

    /// Kanagawa theme.
    ///
    /// The "wave" variant, inspired by Katsushika Hokusai's colors.
    pub fn kanagawa() -> Self {
        PresetColors {
            text: Color::hex_u24(0xDCD7BA),
            bg: Color::hex_u24(0x1F1F28),
            accent: Color::hex_u24(0x7E9CD8),
            success: Color::hex_u24(0x98BB6C),
            warning: Color::hex_u24(0xE6C384),
            error: Color::hex_u24(0xFF5D62),
            info: Color::hex_u24(0x7AA89F),
            azure: Color::hex_u24(0x7FB4CA),
            blue: Color::hex_u24(0x7E9CD8),
            cyan: Color::hex_u24(0x7AA89F),
            grey: Color::hex_u24(0x727169),
            orange: Color::hex_u24(0xFFA066),
            purple: Color::hex_u24(0x957FB8),
        }
        .into_theme()
    }

    /// Everforest theme.
    ///
    /// The dark, medium-contrast variant of Everforest's green-tinted palette.
    pub fn everforest() -> Self {
        PresetColors {
            text: Color::hex_u24(0xD3C6AA),
            bg: Color::hex_u24(0x2D353B),
            accent: Color::hex_u24(0x7FBBB3),
            success: Color::hex_u24(0xA7C080),
            warning: Color::hex_u24(0xDBBC7F),
            error: Color::hex_u24(0xE67E80),
            info: Color::hex_u24(0x83C092),
            azure: Color::hex_u24(0x7FBBB3),
            blue: Color::hex_u24(0x7FBBB3),
            cyan: Color::hex_u24(0x83C092),
            grey: Color::hex_u24(0x859289),
            orange: Color::hex_u24(0xE69875),
            purple: Color::hex_u24(0xD699B6),
        }
        .into_theme()
    }

    /// Ayu Dark theme.
    pub fn ayu_dark() -> Self {
        PresetColors {
            text: Color::hex_u24(0xB3B1AD),
            bg: Color::hex_u24(0x0B0E14),
            accent: Color::hex_u24(0xE6B450),
            success: Color::hex_u24(0xAAD94C),
            warning: Color::hex_u24(0xE6B450),
            error: Color::hex_u24(0xF07178),
            info: Color::hex_u24(0x59C2FF),
            azure: Color::hex_u24(0x59C2FF),
            blue: Color::hex_u24(0x59C2FF),
            cyan: Color::hex_u24(0x95E6CB),
            grey: Color::hex_u24(0x565B66),
            orange: Color::hex_u24(0xFF8F40),
            purple: Color::hex_u24(0xD2A6FF),
        }
        .into_theme()
    }

    /// Ayu Mirage theme.
    ///
    /// The mid-dark variant of Ayu.
    pub fn ayu_mirage() -> Self {
        PresetColors {
            text: Color::hex_u24(0xCCCAC2),
            bg: Color::hex_u24(0x1F2430),
            accent: Color::hex_u24(0xFFCC66),
            success: Color::hex_u24(0xD5FF80),
            warning: Color::hex_u24(0xFFD173),
            error: Color::hex_u24(0xF28779),
            info: Color::hex_u24(0x73D0FF),
            azure: Color::hex_u24(0x73D0FF),
            blue: Color::hex_u24(0x73D0FF),
            cyan: Color::hex_u24(0x95E6CB),
            grey: Color::hex_u24(0x707A8C),
            orange: Color::hex_u24(0xFFAD66),
            purple: Color::hex_u24(0xDFBFFF),
        }
        .into_theme()
    }

    /// Ayu Light theme.
    ///
    /// Uses Ayu's blue as the UI accent; the upstream orange accent does not
    /// carry enough contrast on the near-white background for borders and
    /// focus rings.
    pub fn ayu_light() -> Self {
        PresetColors {
            text: Color::hex_u24(0x5C6166),
            bg: Color::hex_u24(0xFCFCFC),
            accent: Color::hex_u24(0x399EE6),
            success: Color::hex_u24(0x86B300),
            warning: Color::hex_u24(0xF2AE49),
            error: Color::hex_u24(0xE65050),
            info: Color::hex_u24(0x399EE6),
            azure: Color::hex_u24(0x399EE6),
            blue: Color::hex_u24(0x399EE6),
            cyan: Color::hex_u24(0x4CBF99),
            grey: Color::hex_u24(0x8A9199),
            orange: Color::hex_u24(0xFA8D3E),
            purple: Color::hex_u24(0xA37ACC),
        }
        .into_theme()
    }

    /// Nightfox theme.
    ///
    /// The default variant of the Nightfox family.
    pub fn nightfox() -> Self {
        PresetColors {
            text: Color::hex_u24(0xCDCECF),
            bg: Color::hex_u24(0x192330),
            accent: Color::hex_u24(0x719CD6),
            success: Color::hex_u24(0x81B29A),
            warning: Color::hex_u24(0xDBC074),
            error: Color::hex_u24(0xC94F6D),
            info: Color::hex_u24(0x63CDCF),
            azure: Color::hex_u24(0x719CD6),
            blue: Color::hex_u24(0x719CD6),
            cyan: Color::hex_u24(0x63CDCF),
            grey: Color::hex_u24(0x575860),
            orange: Color::hex_u24(0xF4A261),
            purple: Color::hex_u24(0x9D79D6),
        }
        .into_theme()
    }

    /// Nordfox theme.
    ///
    /// The Nord-flavored variant of the Nightfox family, warmer and higher
    /// contrast than [`Theme::nord`].
    pub fn nordfox() -> Self {
        PresetColors {
            text: Color::hex_u24(0xCDCECF),
            bg: Color::hex_u24(0x2E3440),
            accent: Color::hex_u24(0x81A1C1),
            success: Color::hex_u24(0xA3BE8C),
            warning: Color::hex_u24(0xEBCB8B),
            error: Color::hex_u24(0xBF616A),
            info: Color::hex_u24(0x88C0D0),
            azure: Color::hex_u24(0x81A1C1),
            blue: Color::hex_u24(0x81A1C1),
            cyan: Color::hex_u24(0x88C0D0),
            grey: Color::hex_u24(0x4C566A),
            orange: Color::hex_u24(0xD08770),
            purple: Color::hex_u24(0xB48EAD),
        }
        .into_theme()
    }

    /// Night Owl theme.
    ///
    /// Sarah Drasner's theme, tuned for low-light work.
    pub fn night_owl() -> Self {
        PresetColors {
            text: Color::hex_u24(0xD6DEEB),
            bg: Color::hex_u24(0x011627),
            accent: Color::hex_u24(0x82AAFF),
            success: Color::hex_u24(0xADDB67),
            warning: Color::hex_u24(0xECC48D),
            error: Color::hex_u24(0xEF5350),
            info: Color::hex_u24(0x7FDBCA),
            azure: Color::hex_u24(0x82AAFF),
            blue: Color::hex_u24(0x82AAFF),
            cyan: Color::hex_u24(0x7FDBCA),
            grey: Color::hex_u24(0x637777),
            orange: Color::hex_u24(0xF78C6C),
            purple: Color::hex_u24(0xC792EA),
        }
        .into_theme()
    }

    /// Material Palenight theme.
    pub fn material_palenight() -> Self {
        PresetColors {
            text: Color::hex_u24(0xA6ACCD),
            bg: Color::hex_u24(0x292D3E),
            accent: Color::hex_u24(0x82AAFF),
            success: Color::hex_u24(0xC3E88D),
            warning: Color::hex_u24(0xFFCB6B),
            error: Color::hex_u24(0xF07178),
            info: Color::hex_u24(0x89DDFF),
            azure: Color::hex_u24(0x82AAFF),
            blue: Color::hex_u24(0x82AAFF),
            cyan: Color::hex_u24(0x89DDFF),
            grey: Color::hex_u24(0x676E95),
            orange: Color::hex_u24(0xF78C6C),
            purple: Color::hex_u24(0xC792EA),
        }
        .into_theme()
    }

    /// Oxocarbon theme.
    ///
    /// The dark variant, based on IBM's Carbon design system palette.
    pub fn oxocarbon() -> Self {
        PresetColors {
            text: Color::hex_u24(0xF2F4F8),
            bg: Color::hex_u24(0x161616),
            accent: Color::hex_u24(0x78A9FF),
            success: Color::hex_u24(0x42BE65),
            warning: Color::hex_u24(0xF1C21B),
            error: Color::hex_u24(0xEE5396),
            info: Color::hex_u24(0x33B1FF),
            azure: Color::hex_u24(0x33B1FF),
            blue: Color::hex_u24(0x78A9FF),
            cyan: Color::hex_u24(0x3DDBD9),
            grey: Color::hex_u24(0x6F6F6F),
            orange: Color::hex_u24(0xFF7EB6),
            purple: Color::hex_u24(0xBE95FF),
        }
        .into_theme()
    }

    /// Zenburn theme.
    ///
    /// The classic low-contrast, warm-grey scheme.
    pub fn zenburn() -> Self {
        PresetColors {
            text: Color::hex_u24(0xDCDCCC),
            bg: Color::hex_u24(0x3F3F3F),
            accent: Color::hex_u24(0x8CD0D3),
            success: Color::hex_u24(0x7F9F7F),
            warning: Color::hex_u24(0xF0DFAF),
            error: Color::hex_u24(0xCC9393),
            info: Color::hex_u24(0x94BFF3),
            azure: Color::hex_u24(0x94BFF3),
            blue: Color::hex_u24(0x94BFF3),
            cyan: Color::hex_u24(0x93E0E3),
            grey: Color::hex_u24(0x709080),
            orange: Color::hex_u24(0xDFAF8F),
            purple: Color::hex_u24(0xDC8CC3),
        }
        .into_theme()
    }
}

/// The color tokens every preset is built from.
///
/// This is the three [`ThemePalette`] anchors, the four semantic status colors,
/// and the six extra hues that [`FileIconPalette`] and [`GitStatusPalette`]
/// draw from, in one flat table.
///
/// `file_icons` maps 1:1 onto these fields. `git_status` is derived:
/// `modified` = `warning`, `added` = `success`, `deleted`/`conflicted` =
/// `error`, `renamed` = `azure`, `untracked` = `purple`. A few upstream themes
/// point a git slot at a different hue than that derivation; those presets
/// override the individual slot after construction rather than bending this
/// shape to fit.
struct PresetColors {
    text: Color,
    bg: Color,
    accent: Color,
    success: Color,
    warning: Color,
    error: Color,
    info: Color,
    azure: Color,
    blue: Color,
    cyan: Color,
    grey: Color,
    orange: Color,
    purple: Color,
}

impl PresetColors {
    fn into_theme(self) -> Theme {
        let mut theme: Theme = ThemePalette::new(self.text, self.bg, self.accent)
            .success(self.success)
            .warning(self.warning)
            .error(self.error)
            .info(self.info)
            .into();

        theme.file_icons = FileIconPalette {
            azure: self.azure,
            blue: self.blue,
            cyan: self.cyan,
            green: self.success,
            grey: self.grey,
            orange: self.orange,
            purple: self.purple,
            red: self.error,
            yellow: self.warning,
        };
        theme.git_status = GitStatusPalette {
            modified: self.warning,
            added: self.success,
            deleted: self.error,
            renamed: self.azure,
            untracked: self.purple,
            conflicted: self.error,
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
        "solarizedlight" => Some(Theme::solarized_light()),
        "gruvboxlight" => Some(Theme::gruvbox_light()),
        "tokyonightday" => Some(Theme::tokyo_night_day()),
        "catppuccinlatte" => Some(Theme::catppuccin_latte()),
        "catppuccinfrappe" => Some(Theme::catppuccin_frappe()),
        "catppuccinmacchiato" => Some(Theme::catppuccin_macchiato()),
        "catppuccinmocha" => Some(Theme::catppuccin()),
        "rosepine" => Some(Theme::rose_pine()),
        "rosepinemoon" => Some(Theme::rose_pine_moon()),
        "rosepinedawn" => Some(Theme::rose_pine_dawn()),
        "kanagawa" => Some(Theme::kanagawa()),
        "everforest" => Some(Theme::everforest()),
        "ayudark" => Some(Theme::ayu_dark()),
        "ayumirage" => Some(Theme::ayu_mirage()),
        "ayulight" => Some(Theme::ayu_light()),
        "nightfox" => Some(Theme::nightfox()),
        "nordfox" => Some(Theme::nordfox()),
        "nightowl" => Some(Theme::night_owl()),
        "materialpalenight" => Some(Theme::material_palenight()),
        "oxocarbon" => Some(Theme::oxocarbon()),
        "zenburn" => Some(Theme::zenburn()),
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

    /// Every preset reachable through [`preset_by_name`], by its canonical name.
    const ALL_PRESETS: &[&str] = &[
        "one_dark",
        "dracula",
        "nord",
        "gruvbox",
        "catppuccin",
        "tokyo_night",
        "solarized_dark",
        "monokai",
        "lipan",
        "solarized_light",
        "gruvbox_light",
        "tokyo_night_day",
        "catppuccin_latte",
        "catppuccin_frappe",
        "catppuccin_macchiato",
        "rose_pine",
        "rose_pine_moon",
        "rose_pine_dawn",
        "kanagawa",
        "everforest",
        "ayu_dark",
        "ayu_mirage",
        "ayu_light",
        "nightfox",
        "nordfox",
        "night_owl",
        "material_palenight",
        "oxocarbon",
        "zenburn",
    ];

    #[test]
    fn every_preset_resolves_by_name() {
        for name in ALL_PRESETS {
            assert!(
                preset_by_name(name).is_some(),
                "{name} is not reachable through preset_by_name"
            );
        }
    }

    #[test]
    fn every_preset_separates_text_from_background() {
        // Rec. 601 luminance delta, not a WCAG contrast ratio: this is a floor
        // that catches a preset whose text would wash out on its own
        // background, not a full accessibility check.
        for name in ALL_PRESETS {
            let theme = preset_by_name(name).expect("preset resolves");
            let fg = theme.primary.fg.map(|p| p.color()).expect("preset sets fg");
            let bg = theme.primary.bg.map(|p| p.color()).expect("preset sets bg");
            let delta = (fg.luminance() - bg.luminance()).abs();
            assert!(
                delta > 0.35,
                "{name}: text/background luminance delta {delta:.3} is too low to read"
            );
        }
    }

    #[test]
    fn every_preset_elevates_surfaces_away_from_its_backdrop() {
        // Dark themes lighten as they elevate, light themes darken. Either way
        // each step must stay distinguishable from the one below it.
        for name in ALL_PRESETS {
            let theme = preset_by_name(name).expect("preset resolves");
            let s = &theme.surface;
            assert!(
                s.backdrop != s.element && s.element != s.panel && s.panel != s.menu,
                "{name}: surface elevations collapse into each other"
            );
        }
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
