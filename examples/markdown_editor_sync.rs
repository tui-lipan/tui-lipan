use std::sync::Arc;

use tui_lipan::prelude::*;

#[derive(Default)]
struct MarkdownSyncDemo;

#[derive(Default)]
struct State {
    markdown: Arc<str>,
    cursor: usize,
    anchor: Option<usize>,
    /// Scroll offset to force on the editor (set only by preview events).
    /// None means the editor manages its own scroll position.
    editor_scroll: Option<usize>,
    preview_source_line: usize,
}

#[derive(Clone)]
enum Msg {
    Edited(TextAreaEvent),
    EditorScrolled(ScrollEvent),
    PreviewScrolled(ScrollEvent),
    PreviewClicked(DocumentClickEvent),
}

impl Component for MarkdownSyncDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let markdown: Arc<str> = Arc::from(
            r#"# Live Markdown Sync

A synchronized editor + preview demo.

## Features

- type in editor
- preview updates immediately
- scroll editor -> preview follows
- scroll preview -> editor follows

> Clicking a preview line jumps editor scroll target.

## Table

| Item | Value |
|:-----|------:|
| CPU  | 63%   |
| RAM  | 41%   |
| IO   | 12%   |

## Code

```rust
fn main() {
    println!("hello sync");
}
```
"#,
        );
        let cursor = markdown.len();
        State {
            markdown,
            cursor,
            ..State::default()
        }
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Edited(ev) => {
                ctx.state.markdown = ev.value;
                ctx.state.cursor = ev.cursor;
                ctx.state.anchor = ev.anchor;
            }
            Msg::EditorScrolled(ev) => {
                ctx.state.editor_scroll = None;
                ctx.state.preview_source_line = ev.offset;
            }
            Msg::PreviewScrolled(ev) => {
                ctx.state.preview_source_line = ev.offset;
                ctx.state.editor_scroll = Some(ev.offset);
            }
            Msg::PreviewClicked(ev) => {
                ctx.state.preview_source_line = ev.source_line;
                ctx.state.editor_scroll = Some(ev.source_line);
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        HStack::new()
            .gap(1)
            .child(Frame::new().title("Editor").border(true).child({
                let mut ta = TextArea::new(ctx.state.markdown.clone())
                    .border(false)
                    .tab_width(4)
                    .cursor(ctx.state.cursor)
                    .anchor(ctx.state.anchor)
                    .line_numbers(true)
                    .wrap(false)
                    .h_scrollbar(true)
                    .with_syntax("markdown", "One Dark")
                    .on_change(ctx.link().callback(Msg::Edited))
                    .on_scroll(ctx.link().callback(Msg::EditorScrolled));
                if let Some(offset) = ctx.state.editor_scroll {
                    ta = ta.scroll_offset(offset);
                }
                ta
            }))
            .child(
                Frame::new().title("Preview").border(true).child(
                    DocumentView::new(ctx.state.markdown.clone())
                        .border(false)
                        .markdown()
                        .line_numbers(true)
                        .wrap(false)
                        .h_scrollbar(true)
                        .scroll_to_source_line(ctx.state.preview_source_line)
                        .on_scroll(ctx.link().callback(Msg::PreviewScrolled))
                        .on_click(ctx.link().callback(Msg::PreviewClicked)),
                ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Markdown Editor + Preview Sync")
        .mount(MarkdownSyncDemo)
        .run()
}
