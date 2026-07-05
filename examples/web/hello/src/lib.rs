use wasm_bindgen::prelude::*;

use tui_lipan::WebTerminal;
use tui_lipan::core::component::{Component, Context, KeyUpdate, Update};
use tui_lipan::core::element::Element;
use tui_lipan::core::event::{KeyCode, KeyEvent};
use tui_lipan::mount_web;
use tui_lipan::prelude::{Frame, Text, VStack};
use tui_lipan::style::Style;
use tui_lipan_web_shared::mouse_event_from_raw;

struct WebCounter;

impl Component for WebCounter {
    type Message = ();
    type Properties = ();
    type State = i32;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        0
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .title("wasm hello")
            .border(true)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Text::new(format!("count: {}", ctx.state)).style(Style::default().bold()),
                    )
                    .child(Text::new("keys: + / -   (adjust terminal size to reflow)")),
            )
            .into()
    }

    fn update(&mut self, (): Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('+') | KeyCode::Char('=') => {
                ctx.state += 1;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                ctx.state = ctx.state.saturating_sub(1);
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

#[wasm_bindgen]
pub struct WebHelloHandle {
    app: std::cell::RefCell<WebTerminal<WebCounter>>,
}

#[wasm_bindgen]
impl WebHelloHandle {
    #[wasm_bindgen(constructor)]
    pub fn new(term: JsValue, cols: u16, rows: u16) -> Result<WebHelloHandle, JsValue> {
        let app = mount_web(WebCounter, (), term, cols, rows).map_err(js_err)?;
        Ok(Self {
            app: std::cell::RefCell::new(app),
        })
    }

    pub fn on_key_down(&self, ev: web_sys::KeyboardEvent) -> Result<(), JsValue> {
        let Ok(mut app) = self.app.try_borrow_mut() else {
            return Ok(());
        };
        let handled = app.dispatch_key_event(&ev).map_err(js_err)?;
        if handled {
            ev.prevent_default();
        }
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), JsValue> {
        self.app
            .borrow_mut()
            .set_viewport(cols, rows)
            .map_err(js_err)
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
    ) -> Result<(), JsValue> {
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
}

fn js_err(e: impl std::fmt::Display) -> JsValue {
    JsValue::from_str(&e.to_string())
}
