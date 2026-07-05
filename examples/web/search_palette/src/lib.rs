use std::sync::Arc;

use wasm_bindgen::prelude::*;

use tui_lipan::WebTerminal;
use tui_lipan::core::component::{Component, Context, KeyUpdate, Update};
use tui_lipan::core::element::Element;
use tui_lipan::core::event::{KeyCode, KeyEvent, KeyMods};
use tui_lipan::mount_web;
use tui_lipan::prelude::*;
use tui_lipan_web_shared::mouse_event_from_raw;

struct SearchPaletteDemo;

#[derive(Default)]
struct State {
    show: bool,
    last_selected: Option<Arc<str>>,
    modal_transparent_frame_bg: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    Toggle(bool),
    Selected(SearchEvent<Arc<str>>),
    Activated(SearchEvent<Arc<str>>),
}

impl Component for SearchPaletteDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let hint = if ctx.state.show {
            "Esc to close  |  t transparent frame (Ctrl+t from filter)  |  ↑↓  |  Enter"
        } else {
            "Press '/' to open SearchPalette"
        };

        let color_blocks = HStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x1E, 0x40, 0xAF))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Blue Section\nFiles:  1,024\nLines: 48,391")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x16, 0x5A, 0x32))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Green Section\nTests:   312\nPassed: 312")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x78, 0x35, 0x00))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Orange Section\nWarnings: 7\nErrors:   0")),
            )
            .child(
                Frame::new()
                    .style(
                        Style::new()
                            .bg(Color::rgb(0x6B, 0x21, 0xA8))
                            .fg(Color::White),
                    )
                    .padding(1)
                    .child(Text::new("Purple Section\nCoverage: 87%\nDelta:  +2%")),
            );

        let text_rows = VStack::new()
            .gap(0)
            .child(
                Text::new("src/lib.rs              - Crate root & re-exports")
                    .style(Style::new().fg(Color::Cyan)),
            )
            .child(
                Text::new("src/app.rs              - Application entrypoint")
                    .style(Style::new().fg(Color::LightGreen)),
            )
            .child(
                Text::new("src/style/theme.rs      - Style and theming primitives")
                    .style(Style::new().fg(Color::Yellow)),
            )
            .child(
                Text::new("src/widgets/list/mod.rs - List widget")
                    .style(Style::new().fg(Color::LightMagenta)),
            )
            .child(
                Text::new("src/widgets/modal.rs    - Modal dialog widget")
                    .style(Style::new().fg(Color::LightCyan)),
            )
            .child(
                Text::new("src/backend/common.rs   - Renderer utilities")
                    .style(Style::new().fg(Color::Transparent)),
            );

        let mut root = VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new(hint).style(Style::new().fg(Color::DarkGray)))
            .child(color_blocks)
            .child(text_rows);

        if let Some(path) = &ctx.state.last_selected {
            root = root
                .child(Text::new(format!("Opened: {path}")).style(Style::new().fg(Color::Green)));
        }

        if ctx.state.show {
            let entries = vec![
                SearchEntry::header("Sources"),
                SearchEntry::item("src/lib.rs", Arc::from("src/lib.rs"))
                    .description("Crate root & re-exports"),
                SearchEntry::item("src/app.rs", Arc::from("src/app.rs"))
                    .description("Application entrypoint"),
                SearchEntry::item("src/style/theme.rs", Arc::from("src/style/theme.rs"))
                    .description("Style and theming primitives"),
                SearchEntry::spacer(),
                SearchEntry::header("Widgets"),
                SearchEntry::item(
                    "src/widgets/list/mod.rs",
                    Arc::from("src/widgets/list/mod.rs"),
                )
                .description("List widget"),
                SearchEntry::item(
                    "src/widgets/search_palette/mod.rs",
                    Arc::from("src/widgets/search_palette/mod.rs"),
                )
                .description("Search palette widget"),
                SearchEntry::item(
                    "src/widgets/text_area/mod.rs",
                    Arc::from("src/widgets/text_area/mod.rs"),
                )
                .description("Multi-line text editor"),
                SearchEntry::spacer(),
                SearchEntry::header("Examples"),
                SearchEntry::item(
                    "examples/search_palette_hub.rs",
                    Arc::from("examples/search_palette_hub.rs"),
                )
                .description("Native hub demo"),
                SearchEntry::item(
                    "examples/web/search_palette/src/lib.rs",
                    Arc::from("examples/web/search_palette/src/lib.rs"),
                )
                .description("Web demo"),
                SearchEntry::item(
                    "examples/markdown_hub.rs",
                    Arc::from("examples/markdown_hub.rs"),
                )
                .description("Markdown hub demo"),
            ];

            let palette = SearchPalette::<Arc<str>>::new()
                .entries(entries)
                .height(Length::Auto)
                .input_border(false)
                .list_border(false)
                .list_scrollbar(true)
                .list_selection_full_width(true)
                .on_select(ctx.link().callback(Msg::Selected))
                .on_activate(ctx.link().callback(Msg::Activated));

            let palette = palette.list_item_hover_style(Style::new().bg(Color::DarkGray));

            let mut modal = Modal::new()
                .title("Open File")
                .child(palette)
                .width(Length::Px(60))
                .height(Length::Auto)
                .border_style(BorderStyle::Rounded)
                .padding(0)
                .backdrop_style(Style::new().tint_by(Color::rgb(10, 20, 60), 0.55))
                .on_close(ctx.link().callback(|_| Msg::Toggle(false)));
            modal = if ctx.state.modal_transparent_frame_bg {
                modal.frame_style(Style::new().bg(Color::Transparent))
            } else {
                modal
            };
            root = root.child(modal.key("search-palette"));
        }

        root.into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle(show) => {
                ctx.state.show = show;
                Update::full()
            }
            Msg::Selected(event) => {
                ctx.state.last_selected = Some(event.item.value);
                Update::full()
            }
            Msg::Activated(event) => {
                ctx.state.last_selected = Some(event.item.value);
                ctx.state.show = false;
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if ctx.state.show && matches!(key.code, KeyCode::Char('t')) {
            let plain = key.mods == KeyMods::default();
            let ctrl_t = key.mods.ctrl && !key.mods.alt && !key.mods.shift;
            if plain || ctrl_t {
                ctx.state.modal_transparent_frame_bg = !ctx.state.modal_transparent_frame_bg;
                return KeyUpdate::handled(Update::full());
            }
        }

        match key.code {
            KeyCode::Char('/') if !ctx.state.show => {
                ctx.state.show = true;
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

#[wasm_bindgen]
pub struct WebSearchPaletteHandle {
    app: std::cell::RefCell<WebTerminal<SearchPaletteDemo>>,
}

#[wasm_bindgen]
impl WebSearchPaletteHandle {
    #[wasm_bindgen(constructor)]
    pub fn new(
        term: JsValue,
        cols: u16,
        rows: u16,
    ) -> std::result::Result<WebSearchPaletteHandle, JsValue> {
        let app = mount_web(SearchPaletteDemo, (), term, cols, rows).map_err(js_err)?;
        Ok(Self {
            app: std::cell::RefCell::new(app),
        })
    }

    pub fn on_key_down(&self, ev: web_sys::KeyboardEvent) -> std::result::Result<(), JsValue> {
        let Ok(mut app) = self.app.try_borrow_mut() else {
            return Ok(());
        };
        let handled = app.dispatch_key_event(&ev).map_err(js_err)?;
        if handled {
            ev.prevent_default();
        }
        Ok(())
    }

    pub fn on_mouse(
        &self,
        x: i32,
        y: i32,
        button: u8,
        phase: u8,
        is_wheel: bool,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> std::result::Result<(), JsValue> {
        let Some(event) = mouse_event_from_raw(x, y, button, phase, is_wheel, shift, alt, ctrl)
        else {
            return Ok(());
        };

        let Ok(mut app) = self.app.try_borrow_mut() else {
            return Ok(());
        };
        app.dispatch_mouse_event(event).map_err(js_err)
    }

    pub fn prime_clipboard_text(&self, text: String) {
        if let Ok(mut app) = self.app.try_borrow_mut() {
            app.prime_clipboard_text(text);
        }
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::result::Result<(), JsValue> {
        let Ok(mut app) = self.app.try_borrow_mut() else {
            return Ok(());
        };
        app.set_viewport(cols, rows).map_err(js_err)
    }
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}
