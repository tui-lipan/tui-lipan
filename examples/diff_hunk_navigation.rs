use tui_lipan::prelude::*;

struct DiffHunkNavigation;

struct PatchExample {
    title: &'static str,
    path: &'static str,
    patch: &'static str,
}

struct GlobalHunk {
    patch_index: usize,
    hunk_index: usize,
    anchor: DiffHunkAnchor,
}

const PATCHES: &[PatchExample] = &[
    PatchExample {
        title: "Renderer hover states",
        path: "src/backend/ratatui_backend/renderers/text_area.rs",
        patch: concat!(
            "diff --git a/src/backend/ratatui_backend/renderers/text_area.rs b/src/backend/ratatui_backend/renderers/text_area.rs\n",
            "--- a/src/backend/ratatui_backend/renderers/text_area.rs\n",
            "+++ b/src/backend/ratatui_backend/renderers/text_area.rs\n",
            "@@ -24,6 +24,15 @@ fn render_text_area_row(row: Row) {\n",
            "     let mut style = row.style;\n",
            "     if row.selected {\n",
            "         style = style.patch(theme.selection);\n",
            "     }\n",
            "+    if row.context_separator && row.hovered {\n",
            "+        style = style.patch(theme.diff_context_hover);\n",
            "+    }\n",
            "+\n",
            "+    if row.context_separator {\n",
            "+        render_full_width_background(row.area, style);\n",
            "+    }\n",
            "     draw_spans(row.spans, style);\n",
            " }\n",
            "@@ -86,7 +95,11 @@ fn row_hit_test(line: usize, x: u16) -> Hit {\n",
            "     if x < gutter_width {\n",
            "         return Hit::Gutter;\n",
            "     }\n",
            "-    Hit::Content(line)\n",
            "+    if context_separator_lines.contains(&line) {\n",
            "+        Hit::ContextSeparator(line)\n",
            "+    } else {\n",
            "+        Hit::Content(line)\n",
            "+    }\n",
            " }\n",
        ),
    },
    PatchExample {
        title: "Session keybindings",
        path: "src/screens/diff_viewer.rs",
        patch: concat!(
            "diff --git a/src/screens/diff_viewer.rs b/src/screens/diff_viewer.rs\n",
            "--- a/src/screens/diff_viewer.rs\n",
            "+++ b/src/screens/diff_viewer.rs\n",
            "@@ -12,6 +12,7 @@ pub enum DiffViewerMsg {\n",
            "     Close,\n",
            "     Scroll(usize),\n",
            "     ToggleWrap,\n",
            "+    JumpToHunk(usize),\n",
            " }\n",
            "@@ -48,8 +49,16 @@ impl Component for DiffViewer {\n",
            "         match key.code {\n",
            "             KeyCode::Esc => ctx.emit(DiffViewerMsg::Close),\n",
            "             KeyCode::Char('w') => ctx.emit(DiffViewerMsg::ToggleWrap),\n",
            "+            KeyCode::Char(']') => {\n",
            "+                let next = self.current_hunk.saturating_add(1);\n",
            "+                ctx.emit(DiffViewerMsg::JumpToHunk(next));\n",
            "+            }\n",
            "+            KeyCode::Char('[') => {\n",
            "+                let prev = self.current_hunk.saturating_sub(1);\n",
            "+                ctx.emit(DiffViewerMsg::JumpToHunk(prev));\n",
            "+            }\n",
            "             _ => return KeyUpdate::unhandled(Update::none()),\n",
            "         }\n",
            "@@ -92,7 +101,9 @@ fn view_diff(patch: &str, state: &State) -> Element {\n",
            "     DiffView::from_patch(patch)\n",
            "         .mode(DiffViewMode::Unified)\n",
            "         .wrap(state.wrap)\n",
            "-        .height(Length::Flex(1))\n",
            "+        .height(Length::Flex(1))\n",
            "+        .scroll_to_hunk(state.current_hunk)\n",
            "+        .context_lines(4)\n",
            "         .into()\n",
            " }\n",
        ),
    },
    PatchExample {
        title: "Context collapse docs",
        path: "docs/widgets/input.md",
        patch: concat!(
            "diff --git a/docs/widgets/input.md b/docs/widgets/input.md\n",
            "--- a/docs/widgets/input.md\n",
            "+++ b/docs/widgets/input.md\n",
            "@@ -700,6 +700,10 @@ DiffView supports collapsed context regions.\n",
            " Use `context_lines(n)` to keep a compact window around changes.\n",
            " The separator line is styled through `DiffPalette`.\n",
            "\n",
            "+Patch-backed views also expose hunk anchors for keyboard navigation.\n",
            "+Use `DiffData::hunk_anchors(...)` for labels and `DiffView::scroll_to_hunk(...)`\n",
            "+to let the active backend resolve wrapped visual rows after layout.\n",
            "+\n",
            " ```rust\n",
            " DiffView::from_patch(patch)\n",
            "     .context_lines(3)\n",
            "@@ -728,7 +732,7 @@ ScrollView::new()\n",
            "     .children(messages.iter().map(render_message));\n",
            " ```\n",
            "\n",
            "-Outer scroll views own timeline positioning.\n",
            "+Outer scroll views own timeline positioning; inner DiffViews own hunk positioning.\n",
        ),
    },
];

#[derive(Default)]
struct State {
    global_hunk: usize,
}

#[derive(Clone, Debug)]
enum Msg {}

impl Component for DiffHunkNavigation {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char(']') => {
                move_global_hunk(&mut ctx.state, 1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('[') => {
                move_global_hunk(&mut ctx.state, -1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('1') => {
                select_patch(&mut ctx.state, 0);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') => {
                select_patch(&mut ctx.state, 1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('3') => {
                select_patch(&mut ctx.state, 2);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Tab => {
                move_patch(&mut ctx.state, 1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::BackTab => {
                move_patch(&mut ctx.state, -1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let hunks = global_hunks();
        let current = current_global_hunk(&ctx.state, &hunks);
        let active = current
            .map(|hunk| hunk.patch_index)
            .unwrap_or_default()
            .min(PATCHES.len().saturating_sub(1));
        let active_key = patch_key(active);
        let active_offset = current.map(hunk_outer_offset).unwrap_or_default();
        let active_hunk = current.map(|hunk| hunk.hunk_index).unwrap_or_default();
        let active_count = hunk_count(PATCHES[active].patch);

        Frame::new()
            .title("DiffView hunk navigation")
            .status("[ previous global hunk | ] next global hunk | 1/2/3 first hunk in file | Tab file | q quit")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(summary(
                        active,
                        active_hunk,
                        active_count,
                        current.map(|hunk| hunk.anchor.logical_line).unwrap_or_default(),
                        ctx.state.global_hunk.min(hunks.len().saturating_sub(1)),
                        hunks.len(),
                    ))
                    .child(
                        ScrollView::new()
                            .border(true)
                            .border_style(BorderStyle::Rounded)
                            .scrollbar(true)
                            .show_scroll_indicators(true)
                            .padding(1)
                            .gap(1)
                            .scroll_to_key_offset(active_key, active_offset)
                            .children(
                                PATCHES
                                    .iter()
                                    .enumerate()
                                    .map(|(index, _)| render_patch_card(index, current)),
                            ),
                    ),
            )
            .into()
    }
}

fn global_hunks() -> Vec<GlobalHunk> {
    PATCHES
        .iter()
        .enumerate()
        .flat_map(|(patch_index, patch)| {
            DiffData::from_patch(patch.patch)
                .hunk_anchors(DiffViewMode::Unified)
                .into_iter()
                .map(move |anchor| GlobalHunk {
                    patch_index,
                    hunk_index: anchor.index,
                    anchor,
                })
        })
        .collect()
}

fn current_global_hunk<'a>(state: &State, hunks: &'a [GlobalHunk]) -> Option<&'a GlobalHunk> {
    hunks.get(state.global_hunk.min(hunks.len().saturating_sub(1)))
}

fn move_global_hunk(state: &mut State, delta: isize) {
    let count = global_hunks().len();
    if count == 0 {
        return;
    }

    state.global_hunk = if delta.is_negative() {
        state.global_hunk.checked_sub(1).unwrap_or(count - 1)
    } else {
        (state.global_hunk + 1) % count
    };
}

fn select_patch(state: &mut State, patch_index: usize) {
    let patch_index = patch_index.min(PATCHES.len().saturating_sub(1));
    let hunks = global_hunks();
    if let Some(index) = hunks
        .iter()
        .position(|hunk| hunk.patch_index == patch_index)
    {
        state.global_hunk = index;
    }
}

fn move_patch(state: &mut State, delta: isize) {
    let hunks = global_hunks();
    let Some(current) = current_global_hunk(state, &hunks) else {
        return;
    };

    let next_patch = if delta.is_negative() {
        current
            .patch_index
            .checked_sub(1)
            .unwrap_or_else(|| PATCHES.len().saturating_sub(1))
    } else {
        (current.patch_index + 1) % PATCHES.len()
    };
    select_patch(state, next_patch);
}

fn hunk_count(patch: &str) -> usize {
    DiffData::from_patch(patch)
        .hunk_anchors(DiffViewMode::Unified)
        .len()
}

fn summary(
    active: usize,
    active_hunk: usize,
    hunk_count: usize,
    logical_line: usize,
    global_hunk: usize,
    global_count: usize,
) -> Element {
    Frame::new()
        .title("How this works")
        .height(Length::Auto)
        .border(true)
        .border_style(BorderStyle::Plain)
        .padding(1)
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new(format!(
                    "Global hunk {} of {} -> file {} of {}, hunk {} of {}, pre-collapse row {}",
                    global_hunk + 1,
                    global_count.max(1),
                    active + 1,
                    PATCHES.len(),
                    active_hunk + 1,
                    hunk_count.max(1),
                    logical_line,
                )))
                .child(Text::new(
                    "One global cursor chooses a hunk row. The outer ScrollView reveals that keyed row directly.",
                ))
                .child(
                    Text::new(
                        "With auto-height DiffViews, the outer ScrollView owns hunk visibility; inner diff scrolling is disabled.",
                    )
                    .style(Style::new().fg(Color::DarkGray)),
                ),
        )
        .into()
}

fn hunk_outer_offset(hunk: &GlobalHunk) -> usize {
    // Card border + padding + path row + gap + anchor row + gap before the DiffView content.
    6usize.saturating_add(hunk.anchor.logical_line)
}

fn render_patch_card(index: usize, current: Option<&GlobalHunk>) -> Element {
    let patch = &PATCHES[index];
    let is_active = current.is_some_and(|hunk| hunk.patch_index == index);
    let hunk_index = current
        .filter(|hunk| hunk.patch_index == index)
        .map(|hunk| hunk.hunk_index)
        .unwrap_or_default();
    let anchors = DiffData::from_patch(patch.patch).hunk_anchors(DiffViewMode::Unified);
    let hunk_count = anchors.len();
    let anchor_text = anchors
        .iter()
        .map(|anchor| {
            let marker = if is_active && anchor.index == hunk_index {
                "*"
            } else {
                " "
            };
            format!(
                "{marker}#{idx}: row {row}, -{old:?} +{new:?}",
                idx = anchor.index + 1,
                row = anchor.logical_line,
                old = anchor.old_start,
                new = anchor.new_start,
            )
        })
        .collect::<Vec<_>>()
        .join("  ");
    let title = if is_active {
        format!("> {}", patch.title)
    } else {
        patch.title.to_string()
    };

    let diff = DiffView::from_patch(patch.patch)
        .mode(DiffViewMode::Unified)
        .backend(DiffViewBackend::DocumentView)
        .height(Length::Auto)
        .wrap(false)
        .line_numbers(true)
        .highlight_full_width(true)
        .context_separator_hover_style(Style::new().underline())
        .panels_border(false)
        .border(false)
        .scrollbar(false);

    Element::from(
        Frame::new()
            .title(title)
            .status(format!(
                "{} hunk(s) | file {} | {}",
                hunk_count,
                index + 1,
                if is_active { "active" } else { "inactive" }
            ))
            .height(Length::Auto)
            .border(true)
            .border_style(BorderStyle::Rounded)
            .title_style(if is_active {
                Style::new().fg(Color::LightCyan).bold()
            } else {
                Style::default()
            })
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(patch.path).style(Style::new().fg(Color::DarkGray)))
                    .child(Text::new(anchor_text))
                    .child(diff),
            ),
    )
    .key(patch_key(index))
}

fn patch_key(index: usize) -> String {
    format!("patch-{index}")
}

fn main() -> Result<()> {
    App::new()
        .title("Diff hunk navigation example")
        .mount(DiffHunkNavigation)
        .run()
}
