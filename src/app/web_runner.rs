//! Browser-hosted runner (wasm32 + `web` feature): `TestBackend` + xterm.js repaint.
//!
//! # Keyboard and browser shortcuts
//!
//! [`WebTerminal::dispatch_key_event`] delegates to [`TestBackend::send_key`], which uses the
//! same layered key dispatch pipeline as the native [`AppRunner`](crate::AppRunner) event loop.
//! The return value indicates whether a widget, command shortcut, framework action, or bubbling
//! `on_key` consumed the key (not unrelated [`TestBackend::pump`] work). Call
//! [`KeyboardEvent.preventDefault`](https://developer.mozilla.org/en-US/docs/Web/API/Event/preventDefault)
//! **only when that return value is `true`** so unhandled keys can keep their default browser
//! behaviour (find, devtools, etc.) *when the browser actually delivers those events to the page*.
//!
//! ## xterm.js (required wiring)
//!
//! Do **not** rely on `Terminal.onKey` alone for passthrough - it runs after xterm has already
//! decided to consume the key. Use
//! [`attachCustomKeyEventHandler`](https://xtermjs.org/docs/api/terminal/classes/terminal/#attachcustomkeyeventhandler):
//! the handler must return whether **xterm** should process the event. For a WASM-driven terminal
//! (`disableStdin: true` + ANSI `write`), use **`return false` for `keydown`** so xterm never runs
//! its own key-to-input pipeline, call `dispatch_key_event`, then `preventDefault` only when WASM
//! reports handled. See `examples/web/xterm-wasm-host.example.html` in this repo.
//!
//! Set [`disableStdin: true`](https://xtermjs.org/docs/api/terminal/interfaces/iterminaloptions/).
//!
//! ## What cannot be fixed from Rust
//!
//! Many browser UI shortcuts (new tab, close tab, etc.) are handled by the **browser chrome** and
//! **never** surface as `keydown` on your page, no matter what xterm or WASM does. Behaviour
//! differs by browser and OS. Rust can only report “we used this key” vs “we did not”; it cannot
//! force the browser to expose a shortcut it does not deliver to JavaScript.
//!
//! # Resize / CPU
//!
//! [`WebTerminal::set_viewport`] runs a full layout and repaint (ANSI stream to xterm, often a
//! full clear when the grid size changes). **`window.resize` fires many times while dragging** a
//! window edge, so CPU spikes are expected unless the host **debounces** (e.g. `requestAnimationFrame`)
//! and **skips** calls when `term.cols` / `term.rows` are unchanged after `fit.fit()`.
#![allow(missing_docs)]

use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

use crate::Result;
use crate::capture::CapturedFrame;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
use crate::clipboard::WebClipboardProvider;
use crate::core::component::Component;
use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseEvent};
use crate::style::Rect;
use crate::test_backend::TestBackend;

pub fn mount_web<C: Component>(
    component: C,
    props: C::Properties,
    term: JsValue,
    cols: u16,
    rows: u16,
) -> Result<WebTerminal<C>> {
    WebTerminal::new(component, props, term, cols, rows)
}

pub struct WebTerminal<C: Component> {
    backend: TestBackend<C>,
    term: JsValue,
    prev_frame: Option<CapturedFrame>,
    effect_phase: u64,
}

impl<C: Component> WebTerminal<C> {
    pub fn new(
        component: C,
        props: C::Properties,
        term: JsValue,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        console_error_panic_hook::set_once();
        let viewport = Rect {
            x: 0,
            y: 0,
            w: cols.max(1),
            h: rows.max(1),
        };
        let mut backend = TestBackend::new_with_props(component, props);
        #[cfg(all(target_arch = "wasm32", feature = "web"))]
        {
            backend
                .core
                .ctx
                .env()
                .clipboard
                .replace_provider(Box::new(WebClipboardProvider::new()));
        }
        backend.set_viewport(viewport);
        backend.render();
        let _ = backend.pump()?;
        let mut s = Self {
            backend,
            term,
            prev_frame: None,
            effect_phase: 0,
        };
        s.paint_now()?;
        // Enable SGR mouse reporting: button + drag + extended coordinates.
        xterm_write(&s.term, "\x1b[?1000h\x1b[?1002h\x1b[?1006h");
        Ok(s)
    }

    /// Returns `true` if the focused widget or a bubbling `on_key` scope consumed the key (overlay
    /// dismiss counts as handled). **Not** keyed to queued-message drains: those may repaint without
    /// the key being “yours”, so the embedding page can omit `preventDefault` and keep F12 / Ctrl+F
    /// etc. See the module docs above.
    ///
    /// The terminal frame is refreshed after every dispatch so flushes from [`TestBackend::pump`]
    /// still reach xterm even when the return value is `false`.
    pub fn dispatch_key_event(&mut self, ev: &web_sys::KeyboardEvent) -> Result<bool> {
        let Some(key) = keyboard_event_to_key_event(ev) else {
            return Ok(false);
        };
        let handled = self.backend.send_key(key)?;
        if handled {
            self.backend.render();
        }
        self.paint_now()?;
        Ok(handled)
    }

    pub fn prime_clipboard_text(&mut self, text: String) {
        self.backend
            .core
            .ctx
            .env()
            .clipboard
            .set_clipboard_text_cache(text);
    }

    pub fn set_viewport(&mut self, cols: u16, rows: u16) -> Result<()> {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: cols.max(1),
            h: rows.max(1),
        };
        if self.backend.viewport() == viewport {
            return Ok(());
        }
        self.prev_frame = None;
        self.backend.set_viewport(viewport);
        self.backend.render();
        let _ = self.backend.pump()?;
        self.paint_now()
    }

    pub fn dispatch_mouse_event(&mut self, event: MouseEvent) -> Result<()> {
        let handled = self.backend.send_mouse(event)?;
        if handled {
            self.backend.render();
        }
        self.paint_now()
    }

    pub fn dispatch_message(&mut self, msg: C::Message) -> Result<()> {
        if self.backend.dispatch(msg)? {
            self.paint_now()?;
        }
        Ok(())
    }

    pub fn set_effect_phase(&mut self, phase: u64) {
        self.effect_phase = phase;
        self.backend.core.set_effect_phase(phase);
    }

    fn paint_now(&mut self) -> Result<()> {
        self.effect_phase = self.effect_phase.wrapping_add(1);
        self.backend.core.set_effect_phase(self.effect_phase);
        let frame = self
            .backend
            .capture_frame_with_effect_phase(self.effect_phase);
        xterm_write(&self.term, &frame.to_ansi_diff(self.prev_frame.as_ref()));
        self.prev_frame = Some(frame);
        Ok(())
    }
}

fn xterm_write(term: &JsValue, data: &str) {
    if let Ok(v) = js_sys::Reflect::get(term, &JsValue::from_str("write")) {
        if let Ok(f) = v.dyn_into::<js_sys::Function>() {
            let _ = f.call1(term, &JsValue::from_str(data));
        }
    }
}

fn keyboard_event_to_key_event(ev: &web_sys::KeyboardEvent) -> Option<KeyEvent> {
    let mods = KeyMods {
        ctrl: ev.ctrl_key(),
        alt: ev.alt_key(),
        shift: ev.shift_key(),
        super_key: ev.meta_key(),
    };
    let key = ev.key();
    let code = match key.as_str() {
        "Enter" => KeyCode::Enter,
        "Escape" | "Esc" => KeyCode::Esc,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Delete" => KeyCode::Delete,
        "Insert" => KeyCode::Insert,
        "Home" => KeyCode::Home,
        "End" => KeyCode::End,
        "PageUp" => KeyCode::PageUp,
        "PageDown" => KeyCode::PageDown,
        "ArrowUp" => KeyCode::Up,
        "ArrowDown" => KeyCode::Down,
        "ArrowLeft" => KeyCode::Left,
        "ArrowRight" => KeyCode::Right,
        " " => KeyCode::Char(' '),
        k if k.chars().count() == 1 => {
            let mut ch = k.chars().next()?;
            if mods.ctrl && ch.is_ascii_alphabetic() {
                ch = ch.to_ascii_lowercase();
            }
            KeyCode::Char(ch)
        }
        k if k.starts_with('F') && k.len() > 1 => {
            let n: u8 = k[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                KeyCode::F(n)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some(KeyEvent { code, mods })
}
