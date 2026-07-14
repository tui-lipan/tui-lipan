use tui_lipan::TextEditor;
use tui_lipan::prelude::*;

struct TextAreaDemo;

struct State {
    editor: TextEditor,
    viewer: TextEditor,
    vim_editor: TextEditor,
    vim_mode: TextAreaVimMode,
}

impl Default for State {
    fn default() -> Self {
        Self {
            editor: TextEditor::new(
                "Hello\n\
                 World\n\
                 This is a text area.\n\
                 With multiple lines.\n\
                 You can scroll through them!\n\
                 Try using arrow keys.\n\
                 And select text with Shift+Arrow keys.\n\
                 \n\
                 The scrollbar appears when needed.\n\
                 Keep scrolling down!\n\
                 More content here...\n\
                 And more...\n\
                 Almost done...\n\
                 Just a few more lines...\n\
                 Still going...\n\
                 Scroll bar should be visible now.\n\
                 Testing vertical scrolling.\n\
                 Navigation works great!\n\
                 You can use PageUp/PageDown too.\n\
                 Home and End keys work as well.\n\
                 Try Ctrl+Home for top of document.\n\
                 And Ctrl+End for bottom.\n\
                 Selection spanning multiple lines works!\n\
                 The scrollbar is draggable.\n\
                 Mouse wheel scrolling is smooth.\n\
                 Line numbers help with navigation.\n\
                 Almost at the end now...\n\
                 Just a couple more...\n\
                 One more to go...\n\
                 Last line!",
            ),
            viewer: TextEditor::new(
                "READ-ONLY MODE\n\
                 \n\
                 This text area is read-only.\n\
                 You can navigate and select text,\n\
                 but you cannot modify the content.\n\
                 \n\
                 Try typing here - nothing will happen.\n\
                 \n\
                 (Except for navigation keys like Arrows,\n\
                 PageUp/Down, Home/End)\n\
                 \n\
                 This demonstrates read-only functionality.\n\
                 Useful for log viewers, help text, etc.\n\
                 \n\
                 You can still scroll through content.\n\
                 And select text for copying.\n\
                 But editing operations are disabled.\n\
                 \n\
                 The scrollbar works here too.\n\
                 Try dragging it!\n\
                 \n\
                 Mouse wheel scrolling is enabled.\n\
                 Line numbers are visible.\n\
                 Navigation is fully functional.\n\
                 \n\
                 This is great for displaying static content.\n\
                 Like documentation or log files.\n\
                 Almost done with this demo...\n\
                  One more line...\n\
                  End of read-only content!",
            ),
            vim_editor: TextEditor::new(
                "VIM MOTIONS (opt-in)\n\
                 \n\
                 Starts in NORMAL mode.\n\
                 Try h/j/k/l, w/b/e, 0/$, gg, G, and counts.\n\
                 Press v in NORMAL mode to start VISUAL selection;\n\
                 press V to select whole logical lines.\n\
                 motions extend the selection, and v or Esc exits.\n\
                 Use d/y/c operators, yy/dd, x/X, p/P, and . repeat.\n\
                 Search with / or ?; the query prompt opens a bottom search bar.\n\
                 Repeat with n/N, and set marks with m.\n\
                 Text objects include iw/aw, ip/ap, quotes, and brackets.\n\
                 Press i, a, I, or A to return to INSERT mode.\n\
                 \n\
                 This panel uses TextArea::vim_motions(true) and\n\
                 on_vim_mode_change to update the frame title.",
            ),
            vim_mode: TextAreaVimMode::Normal,
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    EditorChanged(TextAreaEvent),
    ViewerChanged(TextAreaEvent),
    VimChanged(TextAreaEvent),
    VimModeChanged(TextAreaVimMode),
    ScrollTo(usize),
}

impl Component for TextAreaDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        #[cfg(feature = "syntax-syntect")]
        let mut root = VStack::new()
            .child(
                Frame::new()
                    .title("TextArea Demo (Arrows, Shift+Arrow, scrollbar is DRAGGABLE, try it!)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(ctx.state.editor.text().to_owned())
                            .border(true)
                            .cursor(ctx.state.editor.cursor())
                            .anchor(ctx.state.editor.anchor())
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .caret_shape(CaretShape::Block)
                            .scrollbar(true)
                            .scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Standalone),
                            )
                            .on_change(ctx.link().callback(Msg::EditorChanged))
                            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                            .min_line_number_width(3),
                    ),
            )
            .child(
                Frame::new()
                    .title("Read Only Panel")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(ctx.state.viewer.text().to_owned())
                            .read_only(true)
                            .border(true)
                            .cursor(ctx.state.viewer.cursor())
                            .anchor(ctx.state.viewer.anchor())
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .caret_shape(CaretShape::Block)
                            .scrollbar(true)
                            .scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Standalone),
                            )
                            .on_change(ctx.link().callback(Msg::ViewerChanged))
                            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                            .min_line_number_width(3),
                    ),
            )
            .child(
                Frame::new()
                    .title(format!("Vim Motions Panel [{:?}]", ctx.state.vim_mode))
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::bound(&ctx.state.vim_editor)
                            .border(true)
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .scrollbar(true)
                            .vim_motions(true)
                            .vim_config(
                                TextAreaVimConfig::new()
                                    .current_line_highlight(TextAreaVimCurrentLineHighlight::Full)
                                    .current_line_style(Style::new().bg(Color::indexed(236)))
                                    .current_line_number_style(
                                        Style::new().fg(Color::Yellow).bold(),
                                    ),
                            )
                            .on_change(ctx.link().callback(Msg::VimChanged))
                            .on_vim_mode_change(ctx.link().callback(Msg::VimModeChanged))
                            .min_line_number_width(3),
                    ),
            );

        #[cfg(not(feature = "syntax-syntect"))]
        let root = VStack::new()
            .child(
                Frame::new()
                    .title("TextArea Demo (Arrows, Shift+Arrow, scrollbar is DRAGGABLE, try it!)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(ctx.state.editor.text().to_owned())
                            .border(true)
                            .cursor(ctx.state.editor.cursor())
                            .anchor(ctx.state.editor.anchor())
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .caret_shape(CaretShape::Block)
                            .scrollbar(true)
                            .scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Standalone),
                            )
                            .on_change(ctx.link().callback(Msg::EditorChanged))
                            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                            .min_line_number_width(3),
                    ),
            )
            .child(
                Frame::new()
                    .title("Read Only Panel")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(ctx.state.viewer.text().to_owned())
                            .read_only(true)
                            .border(true)
                            .cursor(ctx.state.viewer.cursor())
                            .anchor(ctx.state.viewer.anchor())
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .caret_shape(CaretShape::Block)
                            .scrollbar(true)
                            .scrollbar_config(
                                ScrollbarConfig::new().variant(ScrollbarVariant::Standalone),
                            )
                            .on_change(ctx.link().callback(Msg::ViewerChanged))
                            .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
                            .min_line_number_width(3),
                    ),
            )
            .child(
                Frame::new()
                    .title(format!("Vim Motions Panel [{:?}]", ctx.state.vim_mode))
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::bound(&ctx.state.vim_editor)
                            .border(true)
                            .scroll_wheel(true)
                            .line_numbers(true)
                            .scrollbar(true)
                            .vim_motions(true)
                            .vim_config(
                                TextAreaVimConfig::new()
                                    .current_line_highlight(TextAreaVimCurrentLineHighlight::Full)
                                    .current_line_style(Style::new().bg(Color::indexed(236)))
                                    .current_line_number_style(
                                        Style::new().fg(Color::Yellow).bold(),
                                    ),
                            )
                            .on_change(ctx.link().callback(Msg::VimChanged))
                            .on_vim_mode_change(ctx.link().callback(Msg::VimModeChanged))
                            .min_line_number_width(3),
                    ),
            );

        #[cfg(feature = "syntax-syntect")]
        {
            root = root.child(
                Frame::new()
                    .title("Syntax Highlight (syntect)")
                    .padding(1)
                    .height(Length::Flex(1))
                    .child(
                        TextArea::new(
                            r#"fn main() {
    let message = "Hello, syntax!";
    for i in 0..3 {
        println!("{} {}", i, message);
    }
}"#,
                        )
                        .read_only(true)
                        .line_numbers(true)
                        .wrap(false)
                        .with_syntax("rust", "base16-ocean.light")
                        .scrollbar(true)
                        .scrollbar_config(
                            ScrollbarConfig::new().variant(ScrollbarVariant::Standalone),
                        )
                        .min_line_number_width(3),
                    ),
            );
        }

        root.into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::EditorChanged(ev) => {
                ctx.state.editor.set_text(ev.value.to_string());
                ctx.state.editor.set_cursor(ev.cursor);
                ctx.state.editor.set_anchor(ev.anchor);
                Update::layout()
            }
            Msg::ViewerChanged(ev) => {
                // In read-only mode, the text shouldn't change, but cursor/anchor might
                ctx.state.viewer.set_text(ev.value.to_string());
                ctx.state.viewer.set_cursor(ev.cursor);
                ctx.state.viewer.set_anchor(ev.anchor);
                Update::layout()
            }
            Msg::VimChanged(ev) => {
                ev.apply_to(&mut ctx.state.vim_editor);
                Update::layout()
            }
            Msg::VimModeChanged(mode) => {
                ctx.state.vim_mode = mode;
                Update::layout()
            }
            Msg::ScrollTo(_offset) => {
                // Framework handles manual scrolling automatically via NodeKind state
                Update::none()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - TextArea Demo")
        .mount(TextAreaDemo)
        .run()
}
