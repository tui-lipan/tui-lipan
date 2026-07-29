//! Search a terminal render snapshot without changing its plain text.
//!
//! Run with:
//!   cargo run --example terminal_search_highlight --features terminal

use tui_lipan::prelude::*;
use tui_lipan::utils::spans::char_col_to_display_col;

struct TerminalSearchHighlight;

struct State {
    snapshot: TerminalRenderSnapshot,
    query: String,
}

#[derive(Clone)]
enum Msg {
    Append(char),
    Backspace,
    Clear,
    Quit,
}

impl Component for TerminalSearchHighlight {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let mut screen = TerminalScreen::new(10, 96, 32);
        screen.process_bytes(
            b"\x1b[1;36m$ cargo check -p tui-lipan\x1b[0m\r\n\
              \x1b[32m   Compiling tui-lipan\x1b[0m\r\n\
              checking terminal search support\r\n\
              search matches keep the original spans and colors\r\n\
              try: terminal, snapshot, spans, or search\r\n\
              \x1b[33mFinished `dev` profile in 1.42s\x1b[0m\r\n",
        );

        Self::State {
            snapshot: screen.render_snapshot(),
            query: String::new(),
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl || key.mods.alt || key.mods.super_key {
            return KeyUpdate::unhandled(Update::none());
        }

        let msg = match key.code {
            KeyCode::Char('q') if ctx.state.query.is_empty() => Msg::Quit,
            KeyCode::Char(ch) => Msg::Append(ch),
            KeyCode::Backspace => Msg::Backspace,
            KeyCode::Esc => Msg::Clear,
            _ => return KeyUpdate::unhandled(Update::none()),
        };
        ctx.link().send(msg);
        KeyUpdate::handled(Update::full())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Append(ch) => ctx.state.query.push(ch.to_ascii_lowercase()),
            Msg::Backspace => {
                ctx.state.query.pop();
            }
            Msg::Clear => ctx.state.query.clear(),
            Msg::Quit => {
                ctx.quit();
                return Update::none();
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let decorations = search_decorations(&ctx.state.snapshot, &ctx.state.query);
        let snapshot = ctx.state.snapshot.decorated(&decorations);
        let match_count = decorations.len();
        let footer = Text::from_spans([
            Span::new("/ search: ").fg(Color::DarkGray),
            Span::new(if ctx.state.query.is_empty() {
                "(type to highlight)"
            } else {
                ctx.state.query.as_str()
            })
            .fg(Color::Yellow)
            .bold(),
            Span::new(format!(
                "  {match_count} match(es)  |  Esc clear  |  q quit"
            ))
            .fg(Color::DarkGray),
        ]);

        VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .header_left("Terminal search / snapshot decorations")
                    .border(true)
                    .padding(0)
                    .child(Terminal::new().snapshot(snapshot).focusable(false)),
            )
            .child(Frame::new().border(true).padding((0, 1)).child(footer))
            .into()
    }
}

fn search_decorations(snapshot: &TerminalRenderSnapshot, query: &str) -> Vec<TerminalDecoration> {
    if query.is_empty() {
        return Vec::new();
    }

    let query = query.to_ascii_lowercase();
    let mut decorations = Vec::new();
    for (row, line) in snapshot.text.lines().enumerate() {
        let spans = [Span::new(line)];
        let lower = line.to_ascii_lowercase();
        let mut start = 0;
        while let Some(relative) = lower[start..].find(&query) {
            let byte_start = start + relative;
            let byte_end = byte_start + query.len();
            let start_col = char_col_to_display_col(&spans, line[..byte_start].chars().count());
            let end_col = char_col_to_display_col(&spans, line[..byte_end].chars().count());
            decorations.push(TerminalDecoration::highlight(
                row,
                start_col..end_col,
                Style::new().bg(Color::Yellow).fg(Color::Black).bold(),
            ));
            start = byte_end.max(byte_start + 1);
        }
    }
    decorations
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Terminal Search Highlight")
        .mount(TerminalSearchHighlight)
        .run()
}
