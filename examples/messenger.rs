use std::sync::Arc;
use tui_lipan::ImageContent;
use tui_lipan::TextAreaImageMode;
use tui_lipan::prelude::*;

struct Messenger;

/// Content of a single chat message.
#[derive(Clone, Debug)]
enum MessageContent {
    /// Plain-text message.
    Text(String),
    /// Message with optional text and attached images.
    Rich {
        text: String,
        images: Vec<Arc<[u8]>>,
    },
}

#[derive(Clone, Debug)]
struct Message {
    content: MessageContent,
    is_sent: bool,
    /// Cursor for text messages (selection support).
    cursor: usize,
    anchor: Option<usize>,
}

impl Message {
    fn text(content: impl Into<String>, is_sent: bool) -> Self {
        Self {
            content: MessageContent::Text(content.into()),
            is_sent,
            cursor: 0,
            anchor: None,
        }
    }

    fn rich(text: impl Into<String>, images: Vec<Arc<[u8]>>, is_sent: bool) -> Self {
        Self {
            content: MessageContent::Rich {
                text: text.into(),
                images,
            },
            is_sent,
            cursor: 0,
            anchor: None,
        }
    }
}

#[derive(Clone, Debug)]
struct Chat {
    name: String,
    messages: Vec<Message>,
    /// Current text in the input box.
    input: String,
    input_cursor: usize,
    input_anchor: Option<usize>,
    /// Attached images for the next sent message (attachment mode).
    images: Vec<ImageContent>,
    scroll: usize,
    pinned_to_bottom: bool,
}

#[derive(Default)]
struct State {
    chats: Vec<Chat>,
    selected_chat: usize,
}

#[derive(Clone, Debug)]
enum Msg {
    InputChanged(TextAreaEvent),
    ImagesChanged(Vec<ImageContent>),
    RemoveImage(DraggableTabCloseEvent),
    Send,
    Scrolled(ScrollEvent),
    SelectChat(usize),
    MessageUpdate(usize, TextAreaEvent),
}

impl Component for Messenger {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            selected_chat: 0,
            chats: vec![
                Chat {
                    name: "Alice".to_string(),
                    messages: vec![
                        Message::text("Hey, how are you?", false),
                        Message::text("I'm doing great! Just working on this TUI messenger.", true),
                        Message::text("That sounds cool! Is it using tui-lipan?", false),
                        Message::text(
                            "Yes, absolutely. It makes building components so easy.",
                            true,
                        ),
                        Message::text("I should try it out sometime.", false),
                        Message::text("You definitely should! The API is very React-like.", true),
                        Message::text("Nice! I love React.", false),
                        Message::text("Me too. Rust + React model is a great combo.", true),
                        Message::text("Any other cool features?", false),
                        Message::text(
                            "Yeah, it has a built-in layout engine, mouse support, and focus management.",
                            true,
                        ),
                        Message::text("Wow, that's impressive for a TUI lib.", false),
                        Message::text("Thanks! It's still a work in progress though.", true),
                    ],
                    input: String::new(),
                    input_cursor: 0,
                    input_anchor: None,
                    images: Vec::new(),
                    scroll: 0,
                    pinned_to_bottom: true,
                },
                Chat {
                    name: "Bob".to_string(),
                    messages: vec![
                        Message::text("Did you see the game last night?", false),
                        Message::text("No, I missed it. Who won?", true),
                        Message::text("The home team! It was a nail-biter.", false),
                        Message::text("Ah man, I wish I watched it.", true),
                        Message::text("You can catch the highlights on YouTube.", false),
                        Message::text("Good idea. I'll do that later.", true),
                        Message::text("Next game is on Friday.", false),
                        Message::text("I'll be there!", true),
                    ],
                    input: String::new(),
                    input_cursor: 0,
                    input_anchor: None,
                    images: Vec::new(),
                    scroll: 0,
                    pinned_to_bottom: true,
                },
                Chat {
                    name: "Charlie".to_string(),
                    messages: vec![
                        Message::text("Meeting at 3 PM.", false),
                        Message::text("Got it. I'll be ready.", true),
                        Message::text("Don't forget the reports.", false),
                        Message::text("Already printed them.", true),
                        Message::text("Perfect. See you then.", false),
                        Message::text("See ya.", true),
                    ],
                    input: String::new(),
                    input_cursor: 0,
                    input_anchor: None,
                    images: Vec::new(),
                    scroll: 0,
                    pinned_to_bottom: true,
                },
                Chat {
                    name: "David".to_string(),
                    messages: vec![
                        Message::text("Hey, are you free this weekend?", false),
                        Message::text("I think so. What's up?", true),
                        Message::text("Hiking trip!", false),
                        Message::text("Sounds fun! Where to?", true),
                    ],
                    input: String::new(),
                    input_cursor: 0,
                    input_anchor: None,
                    images: Vec::new(),
                    scroll: 0,
                    pinned_to_bottom: true,
                },
                Chat {
                    name: "Eve".to_string(),
                    messages: vec![
                        Message::text("Happy Birthday!", false),
                        Message::text("Thank you!!", true),
                    ],
                    input: String::new(),
                    input_cursor: 0,
                    input_anchor: None,
                    images: Vec::new(),
                    scroll: 0,
                    pinned_to_bottom: true,
                },
            ],
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let active_chat = &ctx.state.chats[ctx.state.selected_chat];

        let placeholder: Arc<str> = if !active_chat.images.is_empty() {
            "Add a caption... (Ctrl+V to attach more)".into()
        } else {
            "Type a message... (Ctrl+V to paste image)".into()
        };

        // Build the attachment chip bar (only when images are present).
        let attachment_bar = if !active_chat.images.is_empty() {
            Some(
                DraggableTabBar::new()
                    .tabs(active_chat.images.iter().enumerate().map(|(i, _)| {
                        DraggableTab::new(format!("Image {}", i + 1)).closeable(true)
                    }))
                    .active(usize::MAX) // no active tab - these are chips, not tabs
                    .style(Style::new().fg(Color::Cyan))
                    .active_style(Style::new().fg(Color::Cyan)) // neutralize active highlight
                    .close_symbol("x")
                    .close_style(Style::new().fg(Color::indexed(246)).dim())
                    .close_hover_style(Style::new().fg(Color::LightRed).bold())
                    .draggable(false)
                    .focusable(false)
                    .scroll_wheel(true)
                    .show_overflow_controls(true)
                    .overflow_style(Style::new().fg(Color::Cyan).dim())
                    .overflow_hover_style(Style::new().fg(Color::LightCyan).bold())
                    .on_close(ctx.link().callback(Msg::RemoveImage)),
            )
        } else {
            None
        };

        rsx! {
            Frame {
                title: "Messenger",
                border: true,
                padding: 0,
                HStack {
                    gap: 1,
                    padding: 1,
                    Frame {
                        width: Length::Px(20),
                        height: Length::Flex(1),
                        border: true,
                        border_style: BorderStyle::Plain,
                        padding: 1,
                        ScrollView {
                            scrollbar: true,
                            scrollbar_config: ScrollbarConfig::new().variant(ScrollbarVariant::Integrated),
                            padding: 1,
                            gap: 1,
                            for (i, chat) in ctx.state.chats.iter().enumerate() {
                                Button {
                                    label: Arc::from(chat.name.clone()),
                                    width: Length::Flex(1),
                                    on_click: ctx.link().callback(move |_| Msg::SelectChat(i)),
                                    variant: ButtonVariant::Filled,
                                    focus_style: Style::new().bg(Color::Red).fg(Color::White),
                                    style: if i == ctx.state.selected_chat {
                                        Style::new().bg(Color::Green).fg(Color::White)
                                    } else {
                                        Style::new().bg(Color::indexed(236)).fg(Color::Gray)
                                    },
                                    border_style: BorderStyle::Plain,
                                    align: Align::Start,
                                },
                            },
                        },
                    },
                    VStack {
                        gap: 1,
                        ScrollView {
                            border: true,
                            padding: 1,
                            gap: 1,
                            offset: active_chat.scroll,
                            scrollbar: true,
                            scrollbar_config: ScrollbarConfig::new()
                                .variant(ScrollbarVariant::Integrated)
                                .thumb_style(Style::new().fg(Color::Green).dim())
                                .thumb_focus_style(Style::new().fg(Color::Green)),
                            focusable: true,
                            on_scroll: ctx.link().callback(Msg::Scrolled),
                            clip_mode: ScrollClip::Partial,
                            for (i, msg) in active_chat.messages.iter().enumerate() {
                                HStack {
                                    height: Length::Auto,
                                    align: if msg.is_sent { Align::End } else { Align::Start },
                                    if msg.is_sent {
                                        Spacer { width: Length::Flex(1) },
                                    },
                                    match &msg.content {
                                        MessageContent::Text(text) => {
                                            rsx! {
                                                Frame {
                                                    border: true,
                                                    border_style: if msg.is_sent { BorderStyle::Rounded } else { BorderStyle::Plain },
                                                    style: if msg.is_sent {
                                                        Style::new().fg(Color::Green)
                                                    } else {
                                                        Style::new().fg(Color::Yellow)
                                                    },
                                                    padding: Padding {
                                                        left: 1,
                                                        right: 1,
                                                        top: 0,
                                                        bottom: 0,
                                                    },
                                                    join_frame: true,
                                                    width: Length::Auto,
                                                    height: Length::Auto,
                                                    max_width: Length::Px(40),
                                                    TextArea {
                                                        value: text.clone(),
                                                        cursor: msg.cursor,
                                                        anchor: msg.anchor,
                                                        on_change: ctx.link().callback(move |ev| Msg::MessageUpdate(i, ev)),
                                                        style: if msg.is_sent {
                                                            Style::new().fg(Color::Green)
                                                        } else {
                                                            Style::new().fg(Color::Yellow)
                                                        },
                                                        border: false,
                                                        padding: Padding::default(),
                                                        wrap: true,
                                                        max_width: Length::Px(40),
                                                        read_only: true,
                                                        scrollbar: false,
                                                        focusable: true,
                                                        height: Length::Auto,
                                                        width: Length::Auto,
                                                    },
                                                }
                                            }
                                        }
                                        MessageContent::Rich { text, images } => {
                                            rsx! {
                                                VStack {
                                                    width: Length::Auto,
                                                    height: Length::Auto,
                                                    gap: 0,
                                                    align: if msg.is_sent { Align::End } else { Align::Start },
                                                    HStack {
                                                        width: Length::Auto,
                                                        height: Length::Auto,
                                                        gap: 0,
                                                        for (idx, bytes) in images.iter().enumerate() {
                                                            Frame {
                                                                border: true,
                                                                border_style: if msg.is_sent { BorderStyle::Rounded } else { BorderStyle::Plain },
                                                                style: if msg.is_sent {
                                                                    Style::new().fg(Color::Green)
                                                                } else {
                                                                    Style::new().fg(Color::Yellow)
                                                                },
                                                                width: Length::Auto,
                                                                height: Length::Auto,
                                                                max_width: Length::Px(40),
                                                                max_height: Length::Px(15),
                                                                join_frame: true,
                                                                border_merge_mode: BorderMergeMode::Fuzzy,
                                                                key: idx.to_string(),
                                                                Image::from_bytes(bytes.clone())
                                                                    .width(Length::Auto)
                                                                    .height(Length::Auto)
                                                                    .alt("[image]"),
                                                            },
                                                        },
                                                    },
                                                    if !text.is_empty() {
                                                        Frame {
                                                            border: true,
                                                            border_style: if msg.is_sent { BorderStyle::Rounded } else { BorderStyle::Plain },
                                                            style: if msg.is_sent {
                                                                Style::new().fg(Color::Green)
                                                            } else {
                                                                Style::new().fg(Color::Yellow)
                                                            },
                                                            padding: Padding {
                                                                left: 1,
                                                                right: 1,
                                                                top: 0,
                                                                bottom: 0,
                                                            },
                                                            join_frame: true,
                                                            border_merge_mode: BorderMergeMode::Fuzzy,
                                                            width: Length::Auto,
                                                            height: Length::Auto,
                                                            max_width: Length::Px(40),
                                                            TextArea {
                                                                value: text.clone(),
                                                                cursor: msg.cursor,
                                                                anchor: msg.anchor,
                                                                on_change: ctx.link().callback(move |ev| Msg::MessageUpdate(i, ev)),
                                                                style: if msg.is_sent {
                                                                    Style::new().fg(Color::Green)
                                                                } else {
                                                                    Style::new().fg(Color::Yellow)
                                                                },
                                                                border: false,
                                                                padding: Padding::default(),
                                                                wrap: true,
                                                                max_width: Length::Px(40),
                                                                read_only: true,
                                                                scrollbar: false,
                                                                focusable: true,
                                                                height: Length::Auto,
                                                                width: Length::Auto,
                                                            },
                                                        },
                                                    },
                                                }
                                            }
                                        }
                                    },
                                    if !msg.is_sent {
                                        Spacer { width: Length::Flex(1) },
                                    },
                                },
                            },
                        },
                        VStack {
                            gap: 0,
                            height: Length::Auto,
                            if let Some(bar) = attachment_bar {
                                bar,
                            },
                            HStack {
                                gap: 1,
                                height: Length::Auto,
                                TextArea {
                                    value: active_chat.input.clone(),
                                    cursor: active_chat.input_cursor,
                                    anchor: active_chat.input_anchor,
                                    placeholder: placeholder,
                                    placeholder_style: if !active_chat.images.is_empty() {
                                        Style::new().fg(Color::Cyan).dim()
                                    } else {
                                        Style::new().dim()
                                    },
                                    border: true,
                                    width: Length::Flex(1),
                                    height: Length::Auto,
                                    wrap: true,
                                    scrollbar: false,
                                    newline_binding: TextAreaNewlineBinding::ShiftEnter,
                                    on_change: ctx.link().callback(Msg::InputChanged),
                                    image_mode: TextAreaImageMode::Attachment,
                                    images: active_chat.images.clone(),
                                    on_images_change: ctx.link().callback(Msg::ImagesChanged),
                                    on_key: ctx.link()
                                        .key_handler(|key| {
                                            if key.code == KeyCode::Enter { Some(Msg::Send) } else { None }
                                        }),
                                },
                                Button {
                                    label: "Send",
                                    on_click: ctx.link().callback(|_| Msg::Send),
                                    key: "send_btn",
                                    variant: ButtonVariant::Outlined,
                                    hover_style: Style::new().fg(Color::LightGreen),
                                    focusable: false,
                                },
                            },
                        },
                    },
                },
            }
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::InputChanged(ev) => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                chat.input = ev.value.to_string();
                chat.input_cursor = ev.cursor;
                chat.input_anchor = ev.anchor;
                Update::full()
            }
            Msg::ImagesChanged(images) => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                chat.images = images;
                Update::full()
            }
            Msg::RemoveImage(ev) => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                if ev.index < chat.images.len() {
                    chat.images.remove(ev.index);
                }
                Update::full()
            }
            Msg::Send => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                let has_text = !chat.input.trim().is_empty();
                let has_images = !chat.images.is_empty();

                if has_images || has_text {
                    // Convert ImageContent to raw bytes for the message.
                    let image_bytes: Vec<Arc<[u8]>> = chat
                        .images
                        .iter()
                        .filter_map(|img| img.to_bytes().ok())
                        .map(|b| Arc::from(b.as_slice()))
                        .collect();

                    if image_bytes.is_empty() {
                        // Text-only message.
                        let content = chat.input.clone();
                        chat.messages.push(Message::text(content, true));
                    } else {
                        // Rich message: images + optional text.
                        chat.messages
                            .push(Message::rich(chat.input.clone(), image_bytes, true));
                    }

                    chat.input.clear();
                    chat.input_cursor = 0;
                    chat.input_anchor = None;
                    chat.images.clear();
                    chat.scroll = usize::MAX;
                    chat.pinned_to_bottom = true;
                }
                Update::full()
            }
            Msg::Scrolled(ev) => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                chat.scroll = ev.offset;
                chat.pinned_to_bottom = ev.offset == ev.metrics.max_offset;
                Update::full()
            }
            Msg::MessageUpdate(index, ev) => {
                let chat = &mut ctx.state.chats[ctx.state.selected_chat];
                if let Some(msg) = chat.messages.get_mut(index) {
                    msg.cursor = ev.cursor;
                    msg.anchor = ev.anchor;
                }
                Update::full()
            }
            Msg::SelectChat(index) => {
                ctx.state.selected_chat = index;
                Update::full()
            }
        }
    }
}

fn main() -> Result<()> {
    App::new().mount(Messenger).run()
}
