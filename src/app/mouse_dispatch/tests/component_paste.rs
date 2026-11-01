use super::*;

#[test]
fn mouse_clipboard_paste_bubbles_and_renders_once() {
    struct PasteRoot {
        views: Rc<std::cell::Cell<usize>>,
        handled: bool,
    }
    impl Component for PasteRoot {
        type Properties = ();
        type Message = ();
        type State = (String, usize);
        fn create_state(&self, _: &()) -> Self::State {
            (String::new(), 0)
        }
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            self.views.set(self.views.get() + 1);
            crate::widgets::Input::new(ctx.state.0.clone())
                .read_only(true)
                .border(false)
                .key("readonly")
        }
        fn on_paste(
            &mut self,
            text: &str,
            origin: crate::PasteOrigin,
            ctx: &mut Context<Self>,
        ) -> crate::KeyUpdate {
            assert_eq!(origin, crate::PasteOrigin::Clipboard);
            assert!(ctx.has_focus_within_key("readonly"));
            ctx.state.0 = text.to_owned();
            ctx.state.1 += 1;
            crate::KeyUpdate {
                handled: self.handled,
                update: Update::layout(),
            }
        }
    }
    for handled in [true, false] {
        let clipboard = Rc::new(RefCell::new(MouseClipboardState {
            clipboard: "clipboard answer".into(),
            ..MouseClipboardState::default()
        }));
        let views = Rc::new(std::cell::Cell::new(0));
        let app = crate::App::new()
            .clipboard_provider(MouseClipboardProvider(clipboard.clone()))
            .clipboard_config(ClipboardConfig {
                middle_click_paste: PasteSource::Clipboard,
                ..ClipboardConfig::default()
            });
        let mut backend = TestBackend::new_with_app(
            app,
            PasteRoot {
                views: views.clone(),
                handled,
            },
            (),
        );
        assert!(backend.focus_key(&Key::from("readonly")));
        let rect = backend.rect_of_key(&Key::from("readonly")).unwrap();
        let before = views.get();
        backend
            .send_mouse(MouseEvent {
                x: rect.x as u16,
                y: rect.y as u16,
                kind: MouseKind::Down(MouseButton::Middle),
                mods: KeyMods::NONE,
            })
            .unwrap();
        assert_eq!(backend.state(), &("clipboard answer".to_string(), 1));
        assert_eq!(clipboard.borrow().clipboard_reads, 1);
        assert_eq!(views.get() - before, 1, "one render for handled={handled}");
        assert!(
            backend
                .capture_frame()
                .plain_text()
                .contains("clipboard answer")
        );
    }
}
