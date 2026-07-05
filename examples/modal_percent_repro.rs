use tui_lipan::prelude::*;

struct ModalPercentRepro;

#[derive(Clone, Debug)]
enum Msg {
    Close,
}

struct State {
    show_modal: bool,
    modal_height_percent: u16,
    scope: OverlayScope,
}

impl Component for ModalPercentRepro {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            show_modal: true,
            modal_height_percent: 50,
            scope: OverlayScope::RootPortal,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Close => {
                ctx.state.show_modal = false;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('Q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('m') | KeyCode::Char('M') => {
                ctx.state.show_modal = !ctx.state.show_modal;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('1') => {
                ctx.state.scope = OverlayScope::RootPortal;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') => {
                ctx.state.scope = OverlayScope::Local;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Up | KeyCode::Char('+') | KeyCode::Char('=') => {
                ctx.state.modal_height_percent =
                    ctx.state.modal_height_percent.saturating_add(5).min(100);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Down | KeyCode::Char('-') => {
                ctx.state.modal_height_percent =
                    ctx.state.modal_height_percent.saturating_sub(5).max(5);
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let scope_label = match ctx.state.scope {
            OverlayScope::RootPortal => "RootPortal",
            OverlayScope::Local => "Local",
        };

        let panel_background = Frame::new()
            .title("Panel Host (Local scope target)")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(
                        "Local modal should stay inside this panel and size against it.",
                    ))
                    .child(
                        Frame::new()
                            .title("Panel Background")
                            .border(true)
                            .height(Length::Flex(1))
                            .child(Text::new(
                                "This area should remain visible outside local modal bounds.",
                            )),
                    ),
            );

        let mut panel_stack = ZStack::new().child(panel_background);

        if ctx.state.show_modal {
            panel_stack = panel_stack.child(
                Modal::new()
                    .title("Percent Height Modal")
                    .scope(ctx.state.scope)
                    .width(Length::Percent(70))
                    .height(Length::Percent(ctx.state.modal_height_percent))
                    .padding(1)
                    .border_style(BorderStyle::Rounded)
                    .backdrop_style(Style::new().dim_by(0.5))
                    .on_close(ctx.link().callback(|_| Msg::Close))
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Short content to make under-sizing obvious."))
                            .child(Text::new(
                                "The modal body should still respect percent height.",
                            ))
                            .child(
                                Frame::new()
                                    .title("Fill target")
                                    .border(true)
                                    .height(Length::Flex(1))
                                    .child(Text::new(
                                        "If percent works, this area should be tall.",
                                    )),
                            ),
                    ),
            );
        }

        Frame::new()
            .title("Modal Length::Percent Repro")
            .border(true)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Goal: compare RootPortal vs Local modal behavior."))
                    .child(Text::new(
                        "Controls: m toggle modal | 1 root portal | 2 local | +/- or up/down adjust percent | q quit",
                    ))
                    .child(Text::new(format!(
                        "Current: visible={} | scope={} | modal height={}%%",
                        ctx.state.show_modal, scope_label, ctx.state.modal_height_percent
                    )))
                    .child(Text::new(
                        "Expected: RootPortal affects full viewport. Local is constrained to panel host only.",
                    ))
                    .child(
                        HStack::new()
                            .gap(1)
                            .height(Length::Flex(1))
                            .child(
                                Frame::new()
                                    .title("Outside Panel Area")
                                    .border(true)
                                    .width(Length::Flex(1))
                                    .child(Text::new(
                                        "Local scope should NOT dim or block this area.",
                                    )),
                            )
                            .child(
                                Frame::new()
                                    .title("Host Area")
                                    .border(true)
                                    .padding(1)
                                    .width(Length::Flex(2))
                                    .child(
                                        Center::new()
                                            .width(Size::Percent(90))
                                            .height(Size::Percent(85))
                                            .child(panel_stack),
                                    ),
                            ),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Modal Percent Repro")
        .mount(ModalPercentRepro)
        .run()
}
