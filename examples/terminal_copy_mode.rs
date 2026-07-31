use tui_lipan::prelude::*;

const TERMINAL_KEY: &str = "copy-mode-terminal";
const TERMINAL_ROWS: u16 = 12;
const TERMINAL_COLS: u16 = 72;

struct TerminalCopyModeDemo;

struct State {
    screen: TerminalScreen,
    snapshot: TerminalRenderSnapshot,
    copy_mode: TerminalCopyMode,
    active: bool,
    status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    Key(KeyEvent),
}

impl Component for TerminalCopyModeDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let mut screen = TerminalScreen::new(TERMINAL_ROWS, TERMINAL_COLS, 64);
        screen.process_bytes(
            b"\x1b[1;34m$ \x1b[0mterminal-copy-mode\r\n\
              \x1b[32mOK\x1b[0m  TerminalCopyMode owns navigation, not terminal state.\r\n\
              \x1b[90m-- scrollback --\x1b[0m\r\n\
              line 01: move with j/k or the arrow keys\r\n\
              line 02: press v to place a selection anchor\r\n\
              line 03: use h/l, w/b, 0/^/$, and Ctrl-U/Ctrl-D\r\n\
              line 04: g jumps to the oldest retained row\r\n\
              line 05: G returns to the live terminal view\r\n\
              line 06: Enter or y copies the selected cells\r\n\
              \x1b[1;34m$ \x1b[0mready",
        );
        let snapshot = screen.render_snapshot();
        let copy_mode = TerminalCopyMode::new(
            snapshot.cursor_row as usize,
            snapshot.cursor_col as usize,
            snapshot.scrollback_offset,
        );

        Self::State {
            screen,
            snapshot,
            copy_mode,
            active: true,
            status: "Copy mode active • v starts a selection • q exits".to_string(),
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        ctx.request_focus(TERMINAL_KEY);
        None
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Key(key) => self.handle_key(key, ctx),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let terminal: Element = Terminal::new()
            .snapshot(ctx.state.snapshot.clone())
            .show_cursor(!ctx.state.active)
            .selection(copy_mode_highlight(
                &ctx.state.copy_mode,
                ctx.state.active,
                ctx.state.snapshot.total_scrollback_rows,
            ))
            .selection_style(Style::new().fg(Color::Black).bg(Color::LightCyan))
            .on_key(ctx.link().key_handler(|key| Some(Msg::Key(key))))
            .into();

        VStack::new()
            .child(
                Frame::new()
                    .header_left(if ctx.state.active {
                        "Terminal copy mode"
                    } else {
                        "Terminal"
                    })
                    .footer_left(ctx.state.status.clone())
                    .border(true)
                    .height(Length::Flex(1))
                    .child(terminal.key(TERMINAL_KEY)),
            )
            .child(
                Frame::new()
                    .border(true)
                    .padding((0, 1))
                    .height(Length::Auto)
                    .child(Text::new(
                        "c: enter copy mode  v/space: anchor  Enter/y: copy  Esc/q: leave  Ctrl+Q: quit",
                    )),
            )
            .into()
    }
}

/// Render the copy cursor as a one-cell controlled selection when there is no
/// anchor. With an anchor, normalize the range and extend its final display
/// column so the cursor cell remains visibly included.
fn copy_mode_highlight(
    mode: &TerminalCopyMode,
    active: bool,
    total_scrollback_rows: usize,
) -> Option<TerminalSelection> {
    if !active {
        return None;
    }
    let (cursor_row, cursor_col) = mode.cursor();
    let cursor = TerminalPos {
        line: total_scrollback_rows
            .saturating_sub(mode.scrollback_offset())
            .saturating_add(cursor_row),
        col: cursor_col,
    };
    let mut selection = mode
        .selection(total_scrollback_rows)
        .unwrap_or_else(|| TerminalSelection::new(cursor));
    let (_, end) = selection.normalized();
    if selection.anchor == end {
        selection.anchor.col = selection.anchor.col.saturating_add(1);
    } else {
        selection.cursor.col = selection.cursor.col.saturating_add(1);
    }
    Some(selection)
}

impl TerminalCopyModeDemo {
    fn handle_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> Update {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return Update::none();
        }

        if !ctx.state.active {
            if key.is(KeyCode::Char('c')) {
                self.enter_copy_mode(ctx);
                return Update::full();
            }
            return Update::none();
        }

        let action = {
            let snapshot = &ctx.state.snapshot;
            let (cursor_row, _) = ctx.state.copy_mode.cursor();
            let cursor_row_text = snapshot.text.lines().nth(cursor_row).unwrap_or("");
            let rows = snapshot.color_lines.len().max(1);
            ctx.state.copy_mode.handle_key(
                key,
                CopyModeGrid {
                    rows,
                    cols: TERMINAL_COLS as usize,
                    total_scrollback_rows: snapshot.total_scrollback_rows,
                    cursor_row_text,
                    prompt_lines: &[],
                },
            )
        };

        match action {
            CopyModeAction::Moved | CopyModeAction::SelectionChanged => {
                self.refresh_snapshot(ctx);
                Update::full()
            }
            CopyModeAction::RequestCopy => self.copy_selection(ctx),
            CopyModeAction::Cancel => {
                ctx.state.active = false;
                ctx.state.screen.set_scrollback(0);
                ctx.state.snapshot = ctx.state.screen.render_snapshot();
                ctx.state.status = "Copy mode exited • press c to enter again".to_string();
                Update::full()
            }
            CopyModeAction::Ignored => Update::none(),
        }
    }

    fn refresh_snapshot(&mut self, ctx: &mut Context<Self>) {
        // Copy-mode navigation owns the display offset; apply it to the screen before rendering.
        ctx.state
            .screen
            .set_scrollback(ctx.state.copy_mode.scrollback_offset());
        ctx.state.snapshot = ctx.state.screen.render_snapshot();
    }

    fn enter_copy_mode(&mut self, ctx: &mut Context<Self>) {
        ctx.state.screen.set_scrollback(0);
        ctx.state.snapshot = ctx.state.screen.render_snapshot();
        ctx.state.copy_mode = TerminalCopyMode::new(
            ctx.state.snapshot.cursor_row as usize,
            ctx.state.snapshot.cursor_col as usize,
            ctx.state.snapshot.scrollback_offset,
        );
        ctx.state.active = true;
        ctx.state.status = "Copy mode active • v starts a selection • q exits".to_string();
    }

    fn copy_selection(&mut self, ctx: &mut Context<Self>) -> Update {
        let Some(selection) = ctx
            .state
            .copy_mode
            .selection(ctx.state.snapshot.total_scrollback_rows)
        else {
            ctx.state.status = "No selection — press v, move, then Enter".to_string();
            return Update::full();
        };

        let text =
            ctx.state
                .screen
                .selection_display_text(&selection, SelectionEnd::Inclusive, true);
        if text.is_empty() {
            ctx.state.status = "Selection is empty".to_string();
            return Update::full();
        }

        match ctx.clipboard().copy(&text) {
            Ok(()) => {
                if ctx.has_focus_within_key(TERMINAL_KEY)
                    && let Some(node_id) = ctx.focused_node_id()
                {
                    ctx.flash_copy_feedback(node_id);
                }
                ctx.state.status = format!("Copied {} characters", text.chars().count());
            }
            Err(error) => {
                ctx.state.status = format!("Copy failed: {error}");
            }
        }
        Update::full()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Terminal copy mode")
        .mount(TerminalCopyModeDemo)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::TestBackend;

    #[test]
    fn visual_copy_cursor_tracks_mode_without_an_anchor() {
        let mut mode = TerminalCopyMode::new(2, 4, 0);
        let initial = copy_mode_highlight(&mode, true, 0).unwrap();
        assert_eq!((initial.anchor.line, initial.anchor.col), (2, 4));
        assert_eq!((initial.cursor.line, initial.cursor.col), (2, 5));

        mode.goto(1, 9, 0);
        let moved = copy_mode_highlight(&mode, true, 0).unwrap();
        assert_eq!((moved.anchor.line, moved.anchor.col), (1, 9));
        assert_eq!((moved.cursor.line, moved.cursor.col), (1, 10));
    }

    #[test]
    fn visual_selection_includes_the_cursor_cell_after_normalization() {
        let mut mode = TerminalCopyMode::new(3, 7, 0);
        mode.handle_key(
            KeyEvent {
                code: KeyCode::Char('v'),
                mods: KeyMods::NONE,
            },
            CopyModeGrid {
                rows: 4,
                cols: TERMINAL_COLS as usize,
                total_scrollback_rows: 0,
                cursor_row_text: "",
                prompt_lines: &[],
            },
        );
        mode.goto(1, 2, 0);
        let selection = copy_mode_highlight(&mode, true, 0).unwrap();
        assert_eq!((selection.anchor.line, selection.anchor.col), (3, 8));
        assert_eq!((selection.cursor.line, selection.cursor.col), (1, 2));
    }

    #[test]
    fn rendered_copy_cursor_moves_instead_of_leaving_the_hardware_cursor_at_ready() {
        let mut backend = TestBackend::new(TerminalCopyModeDemo);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 76,
            h: 18,
        });
        backend.render();
        let before_frame = backend.capture_frame();
        assert!(before_frame.cursor.is_none());
        let before = selected_cells(&before_frame);
        assert_eq!(before.len(), 1);

        assert!(
            backend
                .send_key(KeyEvent {
                    code: KeyCode::Left,
                    mods: KeyMods::NONE,
                })
                .expect("left key should dispatch")
        );
        backend.render();
        let after_frame = backend.capture_frame();
        assert!(after_frame.cursor.is_none());
        let after = selected_cells(&after_frame);

        assert_eq!(after.len(), 1);
        assert_eq!(after[0].1, before[0].1);
        assert_eq!(after[0].0 + 1, before[0].0);
    }

    #[test]
    fn reentering_copy_mode_starts_at_the_live_terminal_cursor() {
        let mut backend = TestBackend::new(TerminalCopyModeDemo);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 76,
            h: 18,
        });
        backend.render();
        let initial = selected_cells(&backend.capture_frame());
        assert_eq!(initial.len(), 1);

        for code in [KeyCode::Left, KeyCode::Char('q'), KeyCode::Char('c')] {
            assert!(
                backend
                    .send_key(KeyEvent {
                        code,
                        mods: KeyMods::NONE,
                    })
                    .expect("copy-mode key should dispatch")
            );
        }
        backend.render();

        assert_eq!(selected_cells(&backend.capture_frame()), initial);
    }

    fn selected_cells(frame: &tui_lipan::CapturedFrame) -> Vec<(u16, u16)> {
        frame
            .cells
            .iter()
            .enumerate()
            .filter_map(|(index, cell)| {
                (cell.bg == Color::LightCyan)
                    .then_some((index as u16 % frame.width, index as u16 / frame.width))
            })
            .collect()
    }
}
