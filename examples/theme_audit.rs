//! Theme audit harness: renders a realistic developer-console UI across the
//! built-in theme presets and exports PNGs so the default theming can be judged
//! visually (contrast, surface layering, selection, focus, status colors).
//!
//! Run with:
//!   cargo run --example theme_audit --features ui-snapshot-png

use std::fs;
use std::path::PathBuf;

use tui_lipan::prelude::*;
use tui_lipan::{TestBackend, ThemePalette, UiSnapshotOptions};

/// Root that injects a theme over a rich screen subtree.
struct Themed {
    theme: Theme,
}

impl Component for Themed {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ThemeProvider::new(self.theme.clone())
            .child(tui_lipan::child::<Screen, _>(|| Screen, ()))
            .into()
    }
}

/// The actual screen. Reads `ctx.theme()` (the provided theme) for the few
/// places that need explicit semantic colors (status badges).
struct Screen;

impl Component for Screen {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = ctx.theme();
        let bg = theme.primary.bg.map(|p| p.color()).unwrap_or(Color::Black);

        // Inline status chip: colored background, dark text.
        let chip = |label: &str, color: Color| {
            Text::new(format!(" {label} ")).style(Style::new().bg(color).fg(bg).bold())
        };

        // ── Header ──────────────────────────────────────────────────────────
        let tabs = Tabs::new()
            .tab("Explorer")
            .tab("Search")
            .tab("Git")
            .tab("Debug")
            .active(0);

        let header = HStack::new()
            .height(Length::Px(1))
            .padding((0, 1))
            .gap(2)
            .child(
                Text::new("lipan").style(
                    Style::new()
                        .fg(theme.accent.fg.map(|p| p.color()).unwrap_or(bg))
                        .bold(),
                ),
            )
            .child(tabs)
            .child(Spacer::new())
            .child(chip("PASS", theme.status.success))
            .child(chip("2 WARN", theme.status.warning))
            .child(chip("ERR", theme.status.error))
            .child(chip("INFO", theme.status.info));

        // ── Sidebar: file list with selection + scrollbar ───────────────────
        let files = [
            "src/",
            "  main.rs",
            "  theme.rs",
            "  widgets/",
            "    list.rs",
            "    table.rs",
            "  app.rs",
            "Cargo.toml",
            "README.md",
            "LICENSE",
        ];
        let sidebar = Frame::new()
            .title("Explorer")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Px(24))
            .style(Style::new().bg(theme.surface.panel))
            .child(
                List::new()
                    .items(files.map(ListItem::new))
                    .selected(2)
                    .scrollbar(true)
                    .key("files"),
            );

        // ── Center: inspector table + input + buttons + progress ────────────
        let table = Table::new()
            .header(TableRow::new(["Property", "Value"]).style(Style::new().bold()))
            .rows([
                TableRow::section("Build"),
                TableRow::key_value("Profile", "release"),
                TableRow::key_value("Target", "x86_64-linux"),
                TableRow::key_value("Opt level", "3"),
                TableRow::section("Runtime"),
                TableRow::key_value("Threads", "8"),
                TableRow::key_value("Memory", "412 MB"),
            ])
            .widths(vec![ColumnWidth::Min(12), ColumnWidth::Percent(70)])
            .column_spacing(2)
            .selected(5)
            .height(Length::Px(5))
            .border(false);

        let accent = theme.accent.fg.map(|p| p.color()).unwrap_or(bg);
        let actions = HStack::new()
            .gap(2)
            .child(
                Button::filled("Run")
                    .style(Style::new().bg(accent).fg(bg).bold())
                    .key("run"),
            )
            .child(Button::outlined("Build"))
            .child(Checkbox::new(true).label("watch"))
            .child(Spacer::new())
            .child(Text::new("8 passed · 2 skipped").style(theme.muted));

        let progress = ProgressBar::new(0.62)
            .show_percentage(true)
            .label("Indexing");

        let center = Frame::new()
            .title("Inspector")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Flex(2))
            .style(Style::new().bg(theme.surface.panel))
            .child(
                VStack::new()
                    .padding(1)
                    .gap(1)
                    .child(
                        Input::new("theme")
                            .placeholder("Filter symbols…")
                            .key("filter"),
                    )
                    .child(table)
                    .child(actions)
                    .child(progress),
            );

        // ── Right: a popover/menu surface + role swatches ───────────────────
        let menu = Frame::new()
            .title("Menu")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(Style::new().bg(theme.surface.menu))
            .child(
                VStack::new()
                    .padding(1)
                    .child(Text::new("Open file"))
                    .child(Text::new("Rename").style(theme.selection))
                    .child(Text::new("Delete").style(Style::new().fg(theme.status.error)))
                    .child(Text::new("Disabled").style(theme.muted)),
            );

        let details = Frame::new()
            .title("Roles")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .width(Length::Flex(1))
            .style(Style::new().bg(theme.surface.panel))
            .child(
                VStack::new()
                    .padding(1)
                    .child(Text::new("accent").style(theme.accent))
                    .child(Text::new("muted secondary").style(theme.muted))
                    .child(Slider::new(0.4).label("opacity"))
                    .child(menu),
            );

        let body = HStack::new()
            .gap(1)
            .height(Length::Flex(1))
            .child(sidebar)
            .child(center)
            .child(details);

        // ── Footer status bar ───────────────────────────────────────────────
        let footer = StatusBar::new()
            .style(theme.primary.patch(Style::new().bg(theme.surface.menu)))
            .padding((0, 1))
            .gap(2)
            .left(Text::new("main").style(theme.accent))
            .left(Text::new("✓ synced").style(Style::new().fg(theme.status.success)))
            .center(Text::new("theme audit"))
            .right(Text::new("UTF-8"))
            .right(Text::new("Ln 42, Col 8"));

        VStack::new().child(header).child(body).child(footer).into()
    }
}

fn out_dir() -> PathBuf {
    PathBuf::from("/tmp/tui-lipan-theme-audit")
}

fn render_theme(name: &str, theme: Theme) {
    // Demonstrates `App::screen_background` / `App::fill_background`: fill the
    // root viewport with the theme backdrop so gaps between panels read as a
    // designed surface instead of the host terminal color.
    let backdrop = theme.surface.backdrop;
    let mut backend = TestBackend::new(Themed { theme });
    backend.set_screen_background(Some(Style::new().bg(backdrop)));
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 104,
        h: 30,
    });
    backend.render();
    // Move focus once so focus chrome is exercised on the first input/list.
    backend.focus_next();
    backend.render();

    let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::default());
    let dir = out_dir();
    fs::create_dir_all(&dir).unwrap();

    #[cfg(feature = "ui-snapshot-png")]
    {
        let png_path = dir.join(format!("{name}.png"));
        fs::write(&png_path, snapshot.to_png_default()).unwrap();
        println!("Wrote {}", png_path.display());
    }
    #[cfg(not(feature = "ui-snapshot-png"))]
    {
        let md_path = dir.join(format!("{name}.md"));
        fs::write(&md_path, snapshot.to_markdown()).unwrap();
        println!(
            "Wrote {} (enable ui-snapshot-png for images)",
            md_path.display()
        );
    }
}

/// A clean paper / GitHub-light style theme from 3 tokens. Kept as a local
/// (not a preset) to exercise `ThemePalette` derivation on a light background.
fn paper_light() -> Theme {
    ThemePalette::new(
        Color::hex_u24(0x24292F), // text
        Color::hex_u24(0xFFFFFF), // background
        Color::hex_u24(0x0969DA), // accent
    )
    .into_theme()
}

/// Name plus a zero-arg constructor for a theme to render.
type ThemeEntry = (&'static str, fn() -> Theme);

fn main() {
    let themes: &[ThemeEntry] = &[
        ("lipan", Theme::lipan),
        ("default", Theme::default),
        ("one_dark", Theme::one_dark),
        ("dracula", Theme::dracula),
        ("nord", Theme::nord),
        ("gruvbox_dark", Theme::gruvbox_dark),
        ("catppuccin_mocha", Theme::catppuccin_mocha),
        ("tokyo_night", Theme::tokyo_night),
        ("solarized_dark", Theme::solarized_dark),
        ("monokai", Theme::monokai),
        ("solarized_light", Theme::solarized_light),
        ("gruvbox_light", Theme::gruvbox_light),
        ("tokyo_night_day", Theme::tokyo_night_day),
        ("catppuccin_latte", Theme::catppuccin_latte),
        ("catppuccin_frappe", Theme::catppuccin_frappe),
        ("catppuccin_macchiato", Theme::catppuccin_macchiato),
        ("rose_pine", Theme::rose_pine),
        ("rose_pine_moon", Theme::rose_pine_moon),
        ("rose_pine_dawn", Theme::rose_pine_dawn),
        ("kanagawa", Theme::kanagawa),
        ("everforest", Theme::everforest),
        ("ayu_dark", Theme::ayu_dark),
        ("ayu_mirage", Theme::ayu_mirage),
        ("ayu_light", Theme::ayu_light),
        ("nightfox", Theme::nightfox),
        ("nordfox", Theme::nordfox),
        ("night_owl", Theme::night_owl),
        ("material_palenight", Theme::material_palenight),
        ("oxocarbon", Theme::oxocarbon),
        ("zenburn", Theme::zenburn),
        ("paper_light", paper_light),
    ];
    for (name, factory) in themes {
        render_theme(name, factory());
    }
}
