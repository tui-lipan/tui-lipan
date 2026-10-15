//! Arrows and icons are independent; their columns remain aligned across styles.
use tui_lipan::TestBackend;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods};
use tui_lipan::prelude::*;

struct ArrowTree {
    icons: bool,
    arrows: bool,
    style: FileIconStyle,
    guides: IndentStyle,
    custom: bool,
    indent: u16,
}

impl Component for ArrowTree {
    type Message = ();
    type Properties = ();
    type State = (bool, bool, u16, u16);

    fn create_state(&self, _: &()) -> Self::State {
        (self.icons, self.arrows, 1, 1)
    }
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut tree = FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        FileTreeEntry::directory("directory"),
                        FileTreeEntry::file("file.txt"),
                        FileTreeEntry::file("link").symlink(true),
                    ],
                ),
                FileTreeDirectoryListing::new("directory", [FileTreeEntry::file("nested.txt")]),
            ]))
            .change_source(FileTreeChangeSource::Provided(Vec::new()))
            .show_icons(ctx.state.0)
            .show_arrows(ctx.state.1)
            .content_gap(ctx.state.2)
            .icon_gap(ctx.state.3)
            .icon_style(self.style)
            .indent_style(self.guides)
            .indent_width(self.indent)
            .scrollbar(false);
        if self.custom {
            tree = tree
                .expanded_arrow("界")
                .collapsed_arrow("[+]")
                .arrow_style(Style::new().fg(Color::Red))
                .icon_override("directory", "★", None);
        }
        tree.into()
    }
}

fn backend(tree: ArrowTree) -> TestBackend<ArrowTree> {
    let mut backend = TestBackend::new(tree);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 14,
    });
    backend.render();
    backend
}

fn label_column(lines: &[String], label: &str) -> usize {
    let line = lines.iter().find(|line| line.contains(label)).unwrap();
    line[..line.find(label).unwrap()].chars().count()
}

#[test]
fn arrow_and_icon_combinations_align_sibling_labels_in_every_guide_style() {
    for style in [
        FileIconStyle::Text,
        FileIconStyle::NerdFont,
        FileIconStyle::NerdFontColored,
    ] {
        for guides in [
            IndentStyle::None,
            IndentStyle::Line,
            IndentStyle::LineCapped,
            IndentStyle::LineCappedRounded,
            IndentStyle::Branch,
            IndentStyle::BranchRounded,
        ] {
            for icons in [false, true] {
                for arrows in [false, true] {
                    let backend = backend(ArrowTree {
                        icons,
                        arrows,
                        style,
                        guides,
                        custom: false,
                        indent: 1,
                    });
                    let lines = backend.capture_frame().to_fixed_grid_lines();
                    let text = lines.join("\n");
                    assert_eq!(text.contains('▶'), arrows, "{style:?}/{guides:?}:\n{text}");
                    assert_eq!(text.contains("[F]"), icons && style == FileIconStyle::Text);
                    if arrows && !icons {
                        let file = lines
                            .iter()
                            .find(|line| line.contains("file.txt"))
                            .unwrap()
                            .trim_end();
                        let expected = match guides {
                            IndentStyle::None => "   file.txt",
                            IndentStyle::Line
                            | IndentStyle::LineCapped
                            | IndentStyle::LineCappedRounded => "│  file.txt",
                            IndentStyle::Branch | IndentStyle::BranchRounded => "├─ file.txt",
                        };
                        assert_eq!(file, expected, "{guides:?}");
                    }

                    assert_eq!(
                        label_column(&lines, "directory"),
                        label_column(&lines, "file.txt"),
                        "{style:?}/{guides:?}, icons={icons}, arrows={arrows}:\n{text}"
                    );
                    assert_eq!(
                        label_column(&lines, "directory"),
                        label_column(&lines, "link")
                    );
                }
            }
        }
    }
}

#[test]
fn custom_arrows_survive_directory_icon_overrides_and_expand_without_icons() {
    for icons in [false, true] {
        let mut backend = backend(ArrowTree {
            icons,
            arrows: true,
            style: FileIconStyle::NerdFont,
            guides: IndentStyle::Branch,
            custom: true,
            indent: 1,
        });
        let before = backend.capture_frame().to_fixed_grid_lines();
        assert!(before.iter().any(|line| line.contains("[+]")));
        assert_eq!(before.join("\n").contains('★'), icons);
        assert_eq!(
            label_column(&before, "directory"),
            label_column(&before, "file.txt"),
            "icons={icons}:\n{}",
            before.join("\n")
        );
        backend.focus_next();
        for code in [KeyCode::Down, KeyCode::Right] {
            backend
                .send_key(KeyEvent {
                    code,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        backend.render();
        let expanded = backend.capture_frame().to_fixed_grid_lines();
        assert!(expanded.iter().any(|line| line.contains("nested.txt")));
        assert!(
            expanded
                .iter()
                .any(|line| line.contains('界') && line.contains("directory"))
        );
        assert_eq!(
            label_column(&expanded, "directory"),
            label_column(&expanded, "file.txt")
        );
        assert_eq!(
            label_column(&before, "directory"),
            label_column(&expanded, "directory")
        );
        backend
            .send_key(KeyEvent {
                code: KeyCode::Left,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
        assert!(!backend.capture_frame().plain_text().contains("nested.txt"));
    }
}

#[test]
fn toggling_arrows_and_icons_updates_existing_rows_without_losing_text() {
    let mut backend = backend(ArrowTree {
        icons: true,
        arrows: true,
        style: FileIconStyle::NerdFontColored,
        guides: IndentStyle::Branch,
        custom: false,
        indent: 1,
    });
    for icons in [false, true] {
        for arrows in [false, true] {
            *backend.state_mut() = (icons, arrows, 1, 1);
            backend.render();
            let frame = backend.capture_frame();
            let text = frame.plain_text();
            assert!(
                text.contains("directory") && text.contains("file.txt"),
                "{text}"
            );
            for cell in &frame.cells {
                assert!(
                    cell.symbol.chars().count() <= 1,
                    "a cell must contain a single glyph, got {:?}",
                    cell.symbol
                );
            }
        }
    }
}

#[test]
fn custom_gaps_align_rows_and_leave_only_the_arrow_glyph_area_filled() {
    for custom in [false, true] {
        let mut backend = backend(ArrowTree {
            icons: true,
            arrows: true,
            style: if custom {
                FileIconStyle::NerdFont
            } else {
                FileIconStyle::Text
            },
            guides: IndentStyle::BranchRounded,
            custom,
            indent: 1,
        });
        let arrow_width = if custom { 3 } else { 1 };
        let icon_width = if custom { 1 } else { 3 };
        for icons in [false, true] {
            for arrows in [false, true] {
                for content_gap in 0..=3 {
                    for icon_gap in 0..=3 {
                        *backend.state_mut() = (icons, arrows, content_gap, icon_gap);
                        backend.render();
                        let lines = backend.capture_frame().to_fixed_grid_lines();
                        let expected_column = 1
                            + if arrows { arrow_width } else { 0 }
                            + content_gap as usize
                            + if icons {
                                icon_width + icon_gap as usize
                            } else {
                                0
                            };
                        assert_eq!(label_column(&lines, "directory"), expected_column);
                        assert_eq!(label_column(&lines, "file.txt"), expected_column);
                        assert_eq!(label_column(&lines, "link"), expected_column);
                        let file = lines.iter().find(|line| line.contains("file.txt")).unwrap();
                        let prefix = if arrows {
                            format!(
                                "├{}{}",
                                "─".repeat(arrow_width),
                                " ".repeat(content_gap as usize)
                            )
                        } else {
                            format!("├{}", " ".repeat(content_gap as usize))
                        };
                        assert!(file.starts_with(&prefix), "{file:?}");
                    }
                }
            }
        }
    }
}

#[test]
fn no_prefix_means_no_content_gap_even_with_file_icons() {
    for icons in [false, true] {
        let mut backend = backend(ArrowTree {
            icons,
            arrows: false,
            style: FileIconStyle::Text,
            guides: IndentStyle::BranchRounded,
            custom: false,
            indent: 0,
        });
        for gap in [0, 1, 3] {
            *backend.state_mut() = (icons, false, gap, 1);
            backend.render();
            let lines = backend.capture_frame().to_fixed_grid_lines();
            let expected = if icons { 4 } else { 0 };
            assert_eq!(label_column(&lines, "/remote/repo"), expected);
            assert_eq!(label_column(&lines, "directory"), expected);
            assert_eq!(label_column(&lines, "file.txt"), expected);
        }
    }
}

#[test]
fn a_root_without_an_arrow_has_no_content_gap_while_children_do() {
    let mut backend = backend(ArrowTree {
        icons: false,
        arrows: false,
        style: FileIconStyle::Text,
        guides: IndentStyle::Branch,
        custom: false,
        indent: 1,
    });
    *backend.state_mut() = (false, false, 3, 0);
    backend.render();
    let lines = backend.capture_frame().to_fixed_grid_lines();
    assert_eq!(label_column(&lines, "/remote/repo"), 0);
    assert_eq!(label_column(&lines, "directory"), 4);
    assert_eq!(label_column(&lines, "file.txt"), 4);
}
