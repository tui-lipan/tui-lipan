use tui_lipan::prelude::*;

struct FileTreeShowcase;

#[derive(Clone, Copy, Default)]
struct State {
    guide: usize,
    icon: usize,
    arrow: usize,
    theme: usize,
    indent_width: u16,
    content_gap: u16,
    icon_gap: u16,
    show_icons: bool,
    show_arrows: bool,
    git_status: bool,
    diff_stats: bool,
    git_icons: bool,
    custom_icons: bool,
    path_style: bool,
}

#[derive(Clone, Copy)]
enum Msg {
    NextGuide,
    NextIcon,
    NextArrow,
    NextTheme,
    NextIndentWidth,
    NextContentGap,
    NextIconGap,
    ToggleIcons,
    ToggleArrows,
    ToggleGit,
    ToggleDiffStats,
    ToggleGitIcons,
    ToggleCustomIcons,
    TogglePathStyle,
}

const GUIDES: [IndentStyle; 6] = [
    IndentStyle::None,
    IndentStyle::Line,
    IndentStyle::LineCapped,
    IndentStyle::LineCappedRounded,
    IndentStyle::Branch,
    IndentStyle::BranchRounded,
];

const GUIDE_NAMES: [&str; 6] = [
    "None",
    "Line",
    "LineCapped",
    "LineCappedRounded",
    "Branch",
    "BranchRounded",
];

const ICONS: [FileIconStyle; 3] = [
    FileIconStyle::Text,
    FileIconStyle::NerdFont,
    FileIconStyle::NerdFontColored,
];

const ARROWS: [(&str, &str, &str); 4] = [
    ("Unicode", "▼", "▶"),
    ("Nerd Font", "", ""),
    ("ASCII", "v", ">"),
    ("Brackets", "[-]", "[+]"),
];

const ICON_NAMES: [&str; 3] = ["Text", "NerdFont", "NerdFontColored"];

impl Component for FileTreeShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            guide: 4,
            icon: 2,
            indent_width: 2,
            content_gap: 1,
            icon_gap: 1,
            show_icons: true,
            show_arrows: true,
            git_status: true,
            diff_stats: true,
            git_icons: false,
            custom_icons: false,
            path_style: false,
            ..State::default()
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::NextGuide => ctx.state.guide = (ctx.state.guide + 1) % GUIDES.len(),
            Msg::NextIcon => ctx.state.icon = (ctx.state.icon + 1) % ICONS.len(),
            Msg::NextArrow => ctx.state.arrow = (ctx.state.arrow + 1) % ARROWS.len(),
            Msg::NextTheme => ctx.state.theme = (ctx.state.theme + 1) % 3,
            Msg::NextIndentWidth => ctx.state.indent_width = (ctx.state.indent_width + 1) % 5,
            Msg::NextContentGap => ctx.state.content_gap = (ctx.state.content_gap + 1) % 4,
            Msg::NextIconGap => ctx.state.icon_gap = (ctx.state.icon_gap + 1) % 4,
            Msg::ToggleIcons => ctx.state.show_icons = !ctx.state.show_icons,
            Msg::ToggleArrows => ctx.state.show_arrows = !ctx.state.show_arrows,
            Msg::ToggleGit => ctx.state.git_status = !ctx.state.git_status,
            Msg::ToggleDiffStats => ctx.state.diff_stats = !ctx.state.diff_stats,
            Msg::ToggleGitIcons => ctx.state.git_icons = !ctx.state.git_icons,
            Msg::ToggleCustomIcons => ctx.state.custom_icons = !ctx.state.custom_icons,
            Msg::TogglePathStyle => ctx.state.path_style = !ctx.state.path_style,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let state = &ctx.state;
        let accent = match state.theme {
            0 => Color::LightCyan,
            1 => Color::LightGreen,
            _ => Color::LightMagenta,
        };
        let tree = FileTree::new("/demo/project")
            .entry_source(FileTreeEntrySource::provided(demo_listings()))
            .initial_expanded_paths(["src", "src/widgets", "examples", "docs"])
            .change_source(FileTreeChangeSource::Provided(demo_changes()))
            .show_hidden(true)
            .show_icons(state.show_icons)
            .show_arrows(state.show_arrows)
            .expanded_arrow(ARROWS[state.arrow].1)
            .collapsed_arrow(ARROWS[state.arrow].2)
            .arrow_style(Style::new().fg(accent))
            .content_gap(state.content_gap)
            .icon_gap(state.icon_gap)
            .icon_style(ICONS[state.icon])
            .indent_style(GUIDES[state.guide])
            .indent_guide_style(Style::new().fg(accent).dim())
            .directory_label_style(Style::new().fg(accent).bold())
            .file_label_style(Style::new().fg(Color::indexed(252)))
            .selection_style(Style::new().bg(Color::indexed(236)).fg(Color::White).bold())
            .change_suffix_style(Style::new().fg(accent).bold())
            .git_status(state.git_status)
            .git_icon_style(if state.git_icons {
                GitIconStyle::NerdFont
            } else {
                GitIconStyle::Text
            })
            .show_diff_stats(state.diff_stats)
            .indent_width(state.indent_width);

        let tree = if state.path_style {
            tree.path_style(
                "src/theme.rs",
                FileTreeItemStyle::new()
                    .row(Style::new().bg(Color::indexed(235)))
                    .icon(Style::new().fg(Color::LightYellow))
                    .label(Style::new().fg(Color::LightYellow).bold()),
            )
        } else {
            tree
        };

        let tree = if state.custom_icons {
            tree.icon_override("rs", "󰌠", Some(Color::LightBlue))
                .icon_override("toml", "󰅐", Some(Color::LightYellow))
                .icon_override("widgets", "★", Some(Color::LightYellow))
        } else {
            tree
        };

        let controls = VStack::new()
            .gap(1)
            .width(Length::Px(34))
            .child(Text::new("FILE TREE CONTROLS").style(Style::new().fg(accent).bold()))
            .child(
                control_button(format!("Guides: {}", GUIDE_NAMES[state.guide]))
                    .on_click(ctx.link().callback(|_| Msg::NextGuide))
                    .automation_id("guide-style"),
            )
            .child(
                control_button(format!("Icons: {}", ICON_NAMES[state.icon]))
                    .disabled(!state.show_icons)
                    .on_click(ctx.link().callback(|_| Msg::NextIcon)),
            )
            .child(
                control_button(spacing_label("Indent", state.indent_width))
                    .on_click(ctx.link().callback(|_| Msg::NextIndentWidth))
                    .automation_id("indent-width"),
            )
            .child(
                control_button(toggle_label("File icons", state.show_icons))
                    .on_click(ctx.link().callback(|_| Msg::ToggleIcons))
                    .automation_id("file-icons"),
            )
            .child(
                control_button(toggle_label("Expand arrows", state.show_arrows))
                    .on_click(ctx.link().callback(|_| Msg::ToggleArrows))
                    .automation_id("expand-arrows"),
            )
            .child(
                control_button(format!("Arrow glyphs: {}", ARROWS[state.arrow].0))
                    .disabled(!state.show_arrows)
                    .on_click(ctx.link().callback(|_| Msg::NextArrow))
                    .automation_id("arrow-glyphs"),
            )
            .child(
                control_button(spacing_label("Content gap", state.content_gap))
                    .on_click(ctx.link().callback(|_| Msg::NextContentGap))
                    .automation_id("content-gap"),
            )
            .child(
                control_button(spacing_label("Icon gap", state.icon_gap))
                    .disabled(!state.show_icons)
                    .on_click(ctx.link().callback(|_| Msg::NextIconGap))
                    .automation_id("icon-gap"),
            )
            .child(
                control_button(toggle_label("Git status markers", state.git_status))
                    .on_click(ctx.link().callback(|_| Msg::ToggleGit))
                    .automation_id("git-status-markers"),
            )
            .child(
                control_button(toggle_label("Diff counts", state.diff_stats))
                    .on_click(ctx.link().callback(|_| Msg::ToggleDiffStats))
                    .automation_id("diff-counts"),
            )
            .child(
                control_button(toggle_label("Nerd Font Git marks", state.git_icons))
                    .disabled(!state.git_status)
                    .on_click(ctx.link().callback(|_| Msg::ToggleGitIcons))
                    .automation_id("nerd-git-marks"),
            )
            .child(
                control_button(toggle_label(
                    "Custom file/directory icons",
                    state.custom_icons,
                ))
                .disabled(!state.show_icons)
                .on_click(ctx.link().callback(|_| Msg::ToggleCustomIcons)),
            )
            .child(
                control_button(toggle_label("Highlight src/theme.rs", state.path_style))
                    .on_click(ctx.link().callback(|_| Msg::TogglePathStyle))
                    .automation_id("highlight-theme-file"),
            )
            .child(
                control_button(format!("Theme accent: {}", state.theme + 1))
                    .on_click(ctx.link().callback(|_| Msg::NextTheme)),
            );
        let header = HStack::new()
            .child(Text::new("/demo/project").style(Style::new().bold()))
            .child(Spacer::new())
            .child(Text::new(format!(
                "{} · {}",
                GUIDE_NAMES[state.guide], ICON_NAMES[state.icon]
            )))
            .height(Length::Px(1));

        HStack::new()
            .gap(0)
            .child(controls)
            .child(Divider::vertical().style(Style::new().fg(Color::indexed(238))))
            .child(
                VStack::new()
                    .gap(1)
                    .height(Length::Flex(1))
                    .child(header)
                    .child(tree.height(Length::Flex(1)))
                    .child(
                        Text::new("Enter/→ open · ← collapse · ↑/↓ navigate")
                            .style(Style::new().fg(Color::DarkGray)),
                    ),
            )
            .into()
    }
}

fn spacing_label(label: &str, cells: u16) -> String {
    let unit = if cells == 1 { "cell" } else { "cells" };
    format!("{label}: {cells} {unit}")
}

fn toggle_label(label: &str, enabled: bool) -> String {
    format!("[{}] {label}", if enabled { 'x' } else { ' ' })
}

fn control_button(label: String) -> Button {
    Button::filled(label)
        .style(Style::new().fg(Color::indexed(252)))
        .disabled_style(Style::new().fg(Color::indexed(241)).dim())
}

fn demo_listings() -> Vec<FileTreeDirectoryListing> {
    vec![
        FileTreeDirectoryListing::new(
            "/demo/project",
            [
                FileTreeEntry::directory("src"),
                FileTreeEntry::directory("examples"),
                FileTreeEntry::directory("docs"),
                FileTreeEntry::directory(".github"),
                FileTreeEntry::file("Cargo.toml"),
                FileTreeEntry::file("README.md"),
                FileTreeEntry::file("rust-toolchain.toml"),
                FileTreeEntry::file("current.log").symlink_target("logs/latest.log"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/src",
            [
                FileTreeEntry::directory("widgets"),
                FileTreeEntry::directory("app"),
                FileTreeEntry::file("lib.rs"),
                FileTreeEntry::file("main.rs"),
                FileTreeEntry::file("theme.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/src/widgets",
            [
                FileTreeEntry::directory("file_tree"),
                FileTreeEntry::file("tree.rs"),
                FileTreeEntry::file("file_tree.rs"),
                FileTreeEntry::file("button.rs"),
                FileTreeEntry::file("text_input.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/examples",
            [
                FileTreeEntry::file("file_tree_showcase.rs"),
                FileTreeEntry::file("lazygit.rs"),
                FileTreeEntry::file("showcase.rs"),
            ],
        ),
        FileTreeDirectoryListing::new(
            "/demo/project/docs",
            [
                FileTreeEntry::directory("widgets"),
                FileTreeEntry::file("components.md"),
                FileTreeEntry::file("styling.md"),
                FileTreeEntry::file("data.md"),
            ],
        ),
    ]
}

fn demo_changes() -> Vec<FileTreeChange> {
    vec![
        FileTreeChange::new("src/widgets/file_tree.rs", FileTreeChangeStatus::Modified)
            .kind(FileKind::File)
            .diff_stat(42, 11),
        FileTreeChange::new("src/lib.rs", FileTreeChangeStatus::Modified).kind(FileKind::File),
        FileTreeChange::new(
            "examples/file_tree_showcase.rs",
            FileTreeChangeStatus::Added,
        )
        .kind(FileKind::File)
        .staged(true),
        FileTreeChange::new("docs/data.md", FileTreeChangeStatus::Renamed).kind(FileKind::File),
        FileTreeChange::new("README.md", FileTreeChangeStatus::Untracked).kind(FileKind::File),
    ]
}

fn main() -> Result<()> {
    App::new()
        .title("FileTree Showcase")
        .mount(FileTreeShowcase)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    #[test]
    fn nerd_git_markers_reserve_a_cell_at_the_edge_but_diff_numbers_do_not() {
        for width in [80, 100] {
            let mut backend = TestBackend::new(FileTreeShowcase);
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: width,
                h: 32,
            });
            backend.dispatch(Msg::ToggleGitIcons).unwrap();
            backend.render();
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let icon_only = lines.iter().find(|row| row.contains("lib.rs")).unwrap();
            let with_stats = lines
                .iter()
                .find(|row| row.contains("file_tree.rs"))
                .unwrap();
            assert!(with_stats.trim_end().ends_with(" +42 -11"), "{with_stats}");
            let icon_x = icon_only.chars().position(|ch| ch == '').unwrap();
            let stats_end = with_stats
                .chars()
                .enumerate()
                .filter(|(_, ch)| !ch.is_whitespace())
                .map(|(x, _)| x)
                .last()
                .unwrap();
            assert_eq!(
                icon_x + 1,
                stats_end,
                "only the icon reserves a following cell"
            );
            assert_eq!(icon_only.chars().nth(icon_x + 1), Some(' '));
        }
    }

    #[test]
    fn live_controls_keep_arrows_independent_and_preserve_complete_rows() {
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        });
        backend.render();
        for message in [
            Msg::ToggleArrows,
            Msg::ToggleIcons,
            Msg::ToggleArrows,
            Msg::NextArrow,
            Msg::NextArrow,
            Msg::NextArrow,
            Msg::ToggleIcons,
            Msg::ToggleCustomIcons,
            Msg::ToggleGit,
            Msg::ToggleDiffStats,
            Msg::ToggleGit,
            Msg::ToggleDiffStats,
            Msg::TogglePathStyle,
            Msg::NextContentGap,
            Msg::NextContentGap,
            Msg::NextContentGap,
            Msg::NextIconGap,
            Msg::NextIconGap,
            Msg::NextIconGap,
        ] {
            backend.dispatch(message).unwrap();
            backend.render();
            let frame = backend.capture_ui_snapshot().frame;
            for cell in &frame.cells {
                assert!(
                    cell.symbol.chars().count() <= 1,
                    "unexpected cell {:?}",
                    cell.symbol
                );
            }
            let lines = frame.to_fixed_grid_lines();
            assert!(lines.iter().any(|line| line.contains("Expand arrows")));
            let changed_row = lines
                .iter()
                .find(|line| line.contains("file_tree.rs"))
                .unwrap();
            let suffix = changed_row.split_once("file_tree.rs").unwrap().1.trim();
            let expected = match (backend.state().git_status, backend.state().diff_stats) {
                (true, true) => "M +42 -11",
                (true, false) => "M",
                (false, true) => "+42 -11",
                (false, false) => "",
            };
            assert_eq!(suffix, expected, "markers and diff counts are independent");
            let (y, line) = lines
                .iter()
                .enumerate()
                .find(|(_, line)| line.contains("components.md"))
                .unwrap();
            let x = line[..line.find("components.md").unwrap()].chars().count();
            let cells = &frame.row(y as u16)[x..x + "components.md".len()];
            assert!(
                cells.iter().all(|cell| cell.fg == cells[0].fg),
                "file labels must keep their color after a toggle"
            );
            assert_eq!(
                frame.plain_text().contains('▶'),
                backend.state().show_arrows && backend.state().arrow == 0
            );
        }
    }
    #[test]
    fn content_gap_control_remains_clickable_when_arrows_are_hidden() {
        use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
        let mut backend = TestBackend::new(FileTreeShowcase);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 32,
        });
        backend.render();
        backend.dispatch(Msg::ToggleArrows).unwrap();
        backend.render();
        let snapshot = backend.capture_ui_snapshot();
        let button = snapshot
            .widgets
            .iter()
            .find(|widget| {
                widget
                    .label
                    .as_deref()
                    .is_some_and(|label| label.starts_with("Content gap:"))
            })
            .unwrap()
            .rect;
        for kind in [
            MouseKind::Down(MouseButton::Left),
            MouseKind::Up(MouseButton::Left),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: button.x as u16 + 1,
                    y: button.y as u16,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        backend.render();
        assert_eq!(backend.state().content_gap, 2);
        assert!(!backend.state().show_arrows);
    }
}
