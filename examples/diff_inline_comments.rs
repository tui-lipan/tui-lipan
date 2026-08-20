use tui_lipan::TextEditor;
use tui_lipan::prelude::*;

const BEFORE: &str = r#"use std::time::Duration;

fn retry_delay(attempt: u32) -> Duration {
    Duration::from_millis(250 * attempt as u64)
}

fn connect(endpoint: &str) -> Result<(), String> {
    println!("connecting to {endpoint}");
    Ok(())
}

fn main() -> Result<(), String> {
    connect("http://localhost:8080")
}"#;

const AFTER: &str = r#"use std::time::Duration;

fn retry_delay(attempt: u32) -> Duration {
    let capped = attempt.min(6);
    Duration::from_millis(200 * 2_u64.pow(capped))
}

fn connect(endpoint: &str, timeout: Duration) -> Result<(), String> {
    println!("connecting to {endpoint} with {timeout:?}");
    Ok(())
}

fn main() -> Result<(), String> {
    let timeout = retry_delay(3);
    connect("https://api.example.com", timeout)
}"#;

const COMMENT_EDITOR_KEY: &str = "diff-inline-comment-editor";

struct DiffInlineComments;

#[derive(Clone)]
struct ReviewComment {
    range: DiffLineRange,
    pane: DiffPane,
    body: String,
}

struct State {
    active_range: Option<DiffLineRange>,
    active_pane: Option<DiffPane>,
    draft: TextEditor,
    comments: Vec<ReviewComment>,
    status: String,
}

#[derive(Clone)]
enum Msg {
    LineClicked(DiffLineClickEvent),
    RangeSelected(DiffLineRangeEvent),
    DraftChanged(TextAreaEvent),
    Save,
    Cancel,
    Edit(DiffLineRange, DiffPane),
    Delete(DiffLineRange),
}

impl Component for DiffInlineComments {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            active_range: None,
            active_pane: None,
            draft: TextEditor::new(""),
            comments: Vec::new(),
            status: "Click one row, or drag through the line-number gutter for a range."
                .to_string(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::LineClicked(event) => {
                let range = DiffLineRange::single(event.anchor);
                open_editor(range, event.pane, ctx);
                ctx.state.status = format!(
                    "Commenting on {} ({:?} pane).",
                    range_label(range),
                    event.pane
                );
                Update::full()
            }
            Msg::RangeSelected(event) => {
                open_editor(event.range, event.pane, ctx);
                ctx.state.status = format!("Commenting on {}.", range_label(event.range));
                Update::full()
            }
            Msg::DraftChanged(event) => {
                event.apply_to(&mut ctx.state.draft);
                Update::layout()
            }
            Msg::Save => {
                let (Some(range), Some(pane)) = (ctx.state.active_range, ctx.state.active_pane)
                else {
                    return Update::none();
                };
                let body = ctx.state.draft.text().trim().to_string();
                if body.is_empty() {
                    ctx.state.status = "Write a comment before saving.".to_string();
                    return Update::paint();
                }
                if let Some(comment) = ctx
                    .state
                    .comments
                    .iter_mut()
                    .find(|comment| comment.range == range)
                {
                    comment.body = body;
                    comment.pane = pane;
                } else {
                    ctx.state.comments.push(ReviewComment { range, pane, body });
                }
                ctx.state.active_range = None;
                ctx.state.active_pane = None;
                ctx.state.status = format!("Saved comment on {}.", range_label(range));
                ctx.blur();
                Update::full()
            }
            Msg::Cancel => {
                ctx.state.active_range = None;
                ctx.state.active_pane = None;
                ctx.state.status = "Comment editor closed.".to_string();
                ctx.blur();
                Update::full()
            }
            Msg::Edit(range, pane) => {
                open_editor(range, pane, ctx);
                ctx.state.status = format!("Editing comment on {}.", range_label(range));
                Update::full()
            }
            Msg::Delete(range) => {
                ctx.state.comments.retain(|comment| comment.range != range);
                if ctx.state.active_range == Some(range) {
                    ctx.state.active_range = None;
                    ctx.state.active_pane = None;
                }
                ctx.state.status = format!("Deleted comment on {}.", range_label(range));
                Update::full()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut blocks = Vec::new();
        for comment in &ctx.state.comments {
            if ctx.state.active_range != Some(comment.range) {
                blocks.push(DiffInlineBlock::after_range(
                    comment.range,
                    saved_comment(comment, ctx),
                ));
            }
        }
        if let Some(range) = ctx.state.active_range {
            blocks.push(DiffInlineBlock::after_range(
                range,
                comment_editor(range, ctx),
            ));
        }

        let diff: Element = DiffView::with_content(BEFORE, AFTER)
            .mode(DiffViewMode::Split)
            .document_view(
                DocumentView::new("")
                    .wrap(true)
                    .scrollbar(false)
                    .focusable(false),
            )
            .height(Length::Auto)
            .scrollbar(false)
            .h_scrollbar(false)
            .panels_border(false)
            .vertical_separator(true)
            .highlight_full_width(true)
            .line_numbers(true)
            .inline_blocks(blocks)
            .on_line_click(ctx.link().callback(Msg::LineClicked))
            .on_line_range_select(ctx.link().callback(Msg::RangeSelected))
            .into();
        let diff = diff.key("review-diff");

        VStack::new()
            .padding(1)
            .gap(1)
            .child(
                Frame::new()
                    .header_left("Inline diff comments")
                    .footer_left("Drag line numbers for range • Ctrl+Enter save • Esc cancel")
                    .border(true)
                    .height(Length::Px(3))
                    .child(Text::new(
                        "Selected source rows stay highlighted and the editor spans both panes.",
                    )),
            )
            .child(
                ScrollView::new()
                    .border(true)
                    .height(Length::Flex(1))
                    .scrollbar(true)
                    .ambient_page_scroll(true)
                    .child(diff),
            )
            .child(Text::new(ctx.state.status.clone()).height(Length::Px(1)))
            .into()
    }
}

fn open_editor(range: DiffLineRange, pane: DiffPane, ctx: &mut Context<DiffInlineComments>) {
    let body = ctx
        .state
        .comments
        .iter()
        .find(|comment| comment.range == range)
        .map(|comment| comment.body.clone())
        .unwrap_or_default();
    ctx.state.draft.set_text(body);
    let cursor = ctx.state.draft.text().len();
    ctx.state.draft.set_cursor(cursor);
    ctx.state.draft.set_anchor(None);
    ctx.state.active_range = Some(range);
    ctx.state.active_pane = Some(pane);
    ctx.request_focus(COMMENT_EDITOR_KEY);
}

fn comment_editor(range: DiffLineRange, ctx: &Context<DiffInlineComments>) -> Element {
    let link = ctx.link().clone();
    let interceptor = KeyHandler::new(move |key: KeyEvent| {
        if key.code == KeyCode::Enter && key.mods.ctrl {
            link.send(Msg::Save);
            true
        } else if key.code == KeyCode::Esc {
            link.send(Msg::Cancel);
            true
        } else {
            false
        }
    });
    let editor: Element = TextArea::bound(&ctx.state.draft)
        .placeholder("Write a review comment…")
        .height(Length::Px(4))
        .scrollbar(false)
        .key_interceptor(interceptor)
        .on_change(ctx.link().callback(Msg::DraftChanged))
        .into();

    Frame::new()
        .header_left(format!("New comment on {}", range_label(range)))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding(1)
        .height(Length::Auto)
        .style(Style::new().bg(Color::rgb(25, 30, 38)))
        .child(
            VStack::new()
                .height(Length::Auto)
                .gap(1)
                .child(editor.key(COMMENT_EDITOR_KEY))
                .child(
                    HStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(Button::new("Save").on_click(ctx.link().callback(|_| Msg::Save)))
                        .child(
                            Button::new("Cancel").on_click(ctx.link().callback(|_| Msg::Cancel)),
                        ),
                ),
        )
        .into()
}

fn saved_comment(comment: &ReviewComment, ctx: &Context<DiffInlineComments>) -> Element {
    let edit_range = comment.range;
    let edit_pane = comment.pane;
    let delete_range = comment.range;
    Frame::new()
        .header_left(format!("Review comment on {}", range_label(comment.range)))
        .border(true)
        .border_style(BorderStyle::Rounded)
        .padding((0, 1))
        .height(Length::Auto)
        .style(Style::new().bg(Color::rgb(22, 35, 30)))
        .child(
            VStack::new()
                .height(Length::Auto)
                .child(Text::new(comment.body.clone()))
                .child(
                    HStack::new()
                        .height(Length::Auto)
                        .gap(1)
                        .child(
                            Button::new("Edit").on_click(
                                ctx.link()
                                    .callback(move |_| Msg::Edit(edit_range, edit_pane)),
                            ),
                        )
                        .child(
                            Button::new("Delete")
                                .on_click(ctx.link().callback(move |_| Msg::Delete(delete_range))),
                        ),
                ),
        )
        .into()
}

fn range_label(range: DiffLineRange) -> String {
    let side = match range.start.preferred_side {
        DiffLineSide::Old => "original",
        DiffLineSide::New => "modified",
    };
    match (range.start.preferred_line(), range.end.preferred_line()) {
        (Some(start), Some(end)) if start == end => format!("{side} line {start}"),
        (Some(start), Some(end)) => format!("{side} lines {start}–{end}"),
        _ => "unmapped rows".to_string(),
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Inline Diff Comments")
        .mount(DiffInlineComments)
        .run()
}
