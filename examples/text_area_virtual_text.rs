//! TextArea virtual text demo.
//!
//! Shows inline inlay hints and end-of-line diagnostics that render without
//! entering the editable buffer.
//!
//! Run with:
//!   cargo run --example text_area_virtual_text

use tui_lipan::prelude::*;

struct VirtualTextDemo;

impl Component for VirtualTextDemo {
    type Message = TextAreaEvent;
    type Properties = ();
    type State = TextEditor;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        TextEditor::new("let total = add(1, 2);\nprintln!(\"{}\", total);")
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let value = ctx.state.text();
        let muted = Style::new().fg(Color::DarkGray).italic();
        let diagnostic = Style::new().fg(Color::Yellow);

        let mut virtual_texts = Vec::new();
        if let Some(anchor) = value.find('1') {
            virtual_texts.push(TextAreaVirtualText::inline(
                anchor,
                vec![Span::new("x: ").style(muted)],
            ));
        }
        if let Some(anchor) = value.find('2') {
            virtual_texts.push(TextAreaVirtualText::inline(
                anchor,
                vec![Span::new("y: ").style(muted)],
            ));
        }
        if let Some(anchor) = value.find('\n') {
            virtual_texts.push(TextAreaVirtualText::eol(
                anchor,
                vec![Span::new("  // inferred: i32").style(diagnostic)],
            ));
        }

        VStack::new()
            .gap(1)
            .child(Text::new(
                "Inline hints shift columns; EOL diagnostics do not reflow.",
            ))
            .child(
                TextArea::bound(&ctx.state)
                    .border(true)
                    .line_numbers(true)
                    .virtual_texts(virtual_texts)
                    .on_change(ctx.link().callback(|event| event)),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        msg.apply_to(&mut ctx.state);
        Update::full()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - TextArea Virtual Text")
        .mount(VirtualTextDemo)
        .run()
}
