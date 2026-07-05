use std::sync::Arc;

use tui_lipan::CellMask;
use tui_lipan::prelude::*;

struct OpencodeHome {
    editor: TextEditor,
    logo_sequence: Arc<FrameSequence>,
    logo_mask: Arc<CellMask>,
}

#[derive(Clone, Debug)]
enum Msg {
    InputChanged(TextAreaEvent),
    ActivateLeader,
    LeaderTimeout(u64),
    LogoMouseDown,
    LogoClick,
    LogoHover(bool),
}

struct HomeState {
    leader_active: bool,
    leader_gen: u64,
    model_swapped: bool,
    logo_pressed: bool,
    logo_clicks: u32,
}

fn mask_from_sequence(sequence: &FrameSequence) -> Arc<CellMask> {
    let frame = sequence.get(0).expect("logo sequence must contain a frame");
    let width = frame.width();
    let height = frame.height();
    let mut bits = vec![0u64; (width as usize * height as usize).div_ceil(64)];

    for (idx, cell) in frame.buffer.cells().iter().enumerate() {
        if cell.ch != ' ' {
            bits[idx / 64] |= 1u64 << (idx % 64);
        }
    }

    Arc::new(CellMask {
        origin: (0, 0),
        w: width,
        h: height,
        bits: bits.into(),
    })
}

fn is_ctrl_letter(key: KeyEvent, letter: char) -> bool {
    if key.mods.ctrl {
        return matches!(key.code, KeyCode::Char(c) if c.eq_ignore_ascii_case(&letter));
    }

    matches!(
        (key.code, letter.to_ascii_lowercase()),
        (KeyCode::Char('\x18'), 'x') | (KeyCode::Char('\x03'), 'c') | (KeyCode::Char('\x16'), 'v')
    )
}

fn start_leader(ctx: &mut Context<OpencodeHome>) -> Update {
    ctx.state.leader_active = true;
    ctx.state.leader_gen += 1;

    let leader_gen = ctx.state.leader_gen;
    let cmd = ctx
        .link()
        .command_keyed("leader-timeout", TaskPolicy::LatestOnly, move |link| {
            std::thread::sleep(std::time::Duration::from_secs(2));
            link.send(Msg::LeaderTimeout(leader_gen));
        });

    Update::with_command(cmd)
}

impl Component for OpencodeHome {
    type Message = Msg;
    type Properties = ();
    type State = HomeState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        HomeState {
            leader_active: false,
            leader_gen: 0,
            model_swapped: false,
            logo_pressed: false,
            logo_clicks: 0,
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        // While leader is active, consume the follow-up key.
        if ctx.state.leader_active {
            ctx.state.leader_active = false;
            ctx.state.leader_gen += 1;
            match key.code {
                KeyCode::Char('m') | KeyCode::Char('M') => {
                    ctx.state.model_swapped = !ctx.state.model_swapped;
                    return KeyUpdate::handled(Update::full());
                }
                _ => {
                    return KeyUpdate::handled(Update::full());
                }
            }
        }

        // Ctrl+X activates leader mode.
        if is_ctrl_letter(key, 'x') {
            return KeyUpdate::handled(start_leader(ctx));
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::InputChanged(ev) => {
                self.editor.set_text(ev.value.to_string());
                self.editor.set_cursor(ev.cursor);
                self.editor.set_anchor(ev.anchor);
                Update::full()
            }
            Msg::ActivateLeader => start_leader(ctx),
            Msg::LeaderTimeout(leader_gen) => {
                // Only expire if generation matches (hasn't been cancelled).
                if ctx.state.leader_active && ctx.state.leader_gen == leader_gen {
                    ctx.state.leader_active = false;
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::LogoMouseDown => {
                ctx.state.logo_pressed = true;
                Update::full()
            }
            Msg::LogoClick => {
                ctx.state.logo_pressed = false;
                ctx.state.logo_clicks = ctx.state.logo_clicks.saturating_add(1);
                ctx.state.model_swapped = !ctx.state.model_swapped;
                Update::full()
            }
            Msg::LogoHover(inside) => {
                if inside || !ctx.state.logo_pressed {
                    Update::none()
                } else {
                    ctx.state.logo_pressed = false;
                    Update::full()
                }
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let input_bg = Color::indexed(234); // Very dark grey
        let accent_color = Color::hex("#558EDF"); // Bright blue
        let agent_color = Color::hex("#D8D7D9");
        let vendor_color = Color::hex("#767576");

        // Dim accents while leader mode is active.
        let effective_accent = if ctx.state.leader_active {
            accent_color.dim_by(0.5)
        } else {
            accent_color
        };

        let agent_name = if ctx.state.model_swapped {
            "GPT-5.4 (Leader Demo)"
        } else {
            "Gemini 3 Flash (Antigravity)"
        };

        let status_text = Text::from_spans(vec![
            Span::new("Build").fg(effective_accent),
            Span::new("  "),
            Span::new(agent_name).fg(agent_color),
            Span::new(" "),
            Span::new("Google").fg(vendor_color),
            Span::new("  logo clicks ").fg(vendor_color),
            Span::new(ctx.state.logo_clicks.to_string()).fg(effective_accent),
        ]);

        // The JSON logo has three colors (in order of first appearance):
        //   0 → #f2eded  bright near-white  (used for "opencode" glyphs in rows 1 & 3)
        //   1 → #b8b2b2  mid grey           (used for "openco" glyphs in rows 1 & 3)
        //   2 → #4b4646  dark shadow grey   (inner shadow cells in row 2)
        //
        // We remap all four to our theme palette so the logo adapts to any theme:
        let colors = self.logo_sequence.collect_colors();
        let bright = Color::hex("#D8D7D9");
        let mid = Color::hex("#767576");
        let shadow = Color::hex("#3A3A3A");

        let mut color_mapping: Vec<(Color, Color)> = Vec::new();
        if let Some(&c) = colors.first() {
            color_mapping.push((c, bright));
        }
        if let Some(&c) = colors.get(1) {
            color_mapping.push((c, mid));
        }
        if let Some(&c) = colors.get(2) {
            color_mapping.push((c, shadow));
        }

        let logo_canvas =
            AsciiCanvas::from_sequence(self.logo_sequence.clone()).color_map(color_mapping);
        let logo_tint = if ctx.state.logo_pressed { 0.55 } else { 0.0 };
        let logo_content: Element = EffectScope::new()
            .tint_by(accent_color, logo_tint)
            .child(logo_canvas)
            .into();
        let logo: Element = MouseRegion::new()
            .cell_mask(Arc::clone(&self.logo_mask))
            .capture_click(true)
            .on_mouse_down(ctx.link().callback(|_: MouseEvent| Msg::LogoMouseDown))
            .on_click(ctx.link().callback(|_: MouseEvent| Msg::LogoClick))
            .on_hover_change(ctx.link().callback(Msg::LogoHover))
            .child(logo_content)
            .into();
        let logo = logo.key("opencode-home-logo-region");

        let center_content: Element = rsx! {
            VStack {
                width: Length::Auto,
                height: Length::Auto,
                gap: 0,
                alignment: Align::Center,
                logo,
                Spacer { height: Length::Px(2) },
                Frame {
                    border: false,
                    decorations: vec![
                        EdgeDecoration::new(Edge::Bottom)
                            .glyph(DecorationGlyph::HalfBlock)
                            .style(Style::new().fg(input_bg))
                            .placement(DecorationPlacement::Outside),
                        EdgeDecoration::new(Edge::Left)
                            .glyph(DecorationGlyph::AutoBlock)
                            .style(Style::new().fg(effective_accent))
                            .cap_end(DecorationGlyph::CapBottom),
                    ],
                    style: Style::new().bg(input_bg),
                    height: Length::Auto,
                    padding: (1, 2, 0, 3),
                    VStack {
                        height: Length::Auto,
                        gap: 1,
                        TextArea {
                            value: self.editor.text().to_owned(),
                            cursor: self.editor.cursor(),
                            anchor: self.editor.anchor(),
                            read_only: ctx.state.leader_active,
                            caret_color: Color::hex("#E7E7E8"),
                            style: Style::new().fg(Color::hex("#D8D7D9")),
                            on_change: ctx.link().callback(Msg::InputChanged),
                            key_interceptor: ctx.link()
                                .key_handler(|key| {
                                    if is_ctrl_letter(key, 'x') { Some(Msg::ActivateLeader) } else { None }
                                }),
                            height: Length::Auto,
                            max_height: Length::Px(6),
                            width: Length::Px(70),
                            border: false,
                            placeholder: "Ask anything... \"Fix broken tests\"",
                            placeholder_style: Style::new().fg(Color::hex("#767576")),
                        },
                        HStack {
                            height: Length::Px(1),
                            status_text,
                        },
                    },
                },
                HStack {
                    height: Length::Auto,
                    width: Length::Flex(1),
                    alignment: Align::End,
                    justify: Justify::End,
                    gap: 1,
                    Text {
                        content: "tab",
                        style: Style::new().fg(agent_color),
                    },
                    Text {
                        content: "agents ",
                        style: Style::new().fg(vendor_color),
                    },
                    Text {
                        content: "ctrl+p",
                        style: Style::new().fg(agent_color),
                    },
                    Text {
                        content: "commands",
                        style: Style::new().fg(vendor_color),
                    },
                },
            }
        };
        let center_pin: Element = CenterPin::new().center(center_content).into();

        rsx! {
            VStack {
                center_pin,
                StatusBar {
                    style: Style::new().fg(Color::hex("#767576")),
                    left: Text::new("~/Work/Projects/tui-lipan:main"),
                    center: Text::new("tui-lipan!"),
                    right: Text::new("1.1.53"),
                    padding: (0, 0, 1, 0),
                },
            }
        }
    }
}

fn main() -> Result<()> {
    let json = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/assets/opencode_logo.json"),
    )
    .expect("Could not read examples/assets/opencode_logo.json");

    let logo_sequence =
        Arc::new(FrameSequence::from_json(&json).expect("Could not parse opencode_logo.json"));
    let logo_mask = mask_from_sequence(&logo_sequence);
    let app = OpencodeHome {
        editor: TextEditor::new(""),
        logo_sequence,
        logo_mask,
    };

    App::new().mount(app).run()
}
