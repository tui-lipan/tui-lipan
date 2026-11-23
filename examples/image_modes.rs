/// Demonstrates both TextArea image modes:
///   - **Inline mode**: sentinel characters embedded in the text value are rendered
///     as styled placeholder labels (e.g. "[Img]"). The sentinels behave like
///     normal characters for cursor movement and deletion.
///   - **Attachment mode**: pasting an image appends to an external images list
///     without touching the text value. A `DraggableTabBar` is composed above the
///     text area to display removable image chips with scrolling support.
///
/// Try pasting an image (Ctrl+V) in either text area. In inline mode, a sentinel
/// character is inserted at the cursor; in attachment mode, a chip appears above.
use tui_lipan::prelude::*;
use tui_lipan::{IMAGE_SENTINEL_BASE, ImageContent, ImageFormat, TextAreaImageMode};

struct ImageModesDemo;

#[derive(Default)]
struct State {
    // -- Inline mode --
    inline_value: String,
    inline_cursor: usize,
    inline_anchor: Option<usize>,
    inline_images: Vec<ImageContent>,

    // -- Attachment mode --
    attach_value: String,
    attach_cursor: usize,
    attach_anchor: Option<usize>,
    attach_images: Vec<ImageContent>,
}

#[derive(Clone, Debug)]
enum Msg {
    InlineChanged(TextAreaEvent),
    InlineImagesChanged(Vec<ImageContent>),
    AttachChanged(TextAreaEvent),
    AttachImagesChanged(Vec<ImageContent>),
    RemoveAttachment(DraggableTabCloseEvent),
    SeedInline,
    SeedAttach,
    ClearInline,
    ClearAttach,
}

/// Create a tiny 1x1 PNG so we have valid `ImageContent` without real files.
fn dummy_image() -> ImageContent {
    // Minimal 1x1 red PNG (67 bytes).
    let png: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x36, 0x28, 0x19, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    ImageContent::from_bytes(png, ImageFormat::Png)
}

impl Component for ImageModesDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::InlineChanged(ev) => {
                ctx.state.inline_value = ev.value.to_string();
                ctx.state.inline_cursor = ev.cursor;
                ctx.state.inline_anchor = ev.anchor;
            }
            Msg::InlineImagesChanged(imgs) => {
                ctx.state.inline_images = imgs;
            }
            Msg::AttachChanged(ev) => {
                ctx.state.attach_value = ev.value.to_string();
                ctx.state.attach_cursor = ev.cursor;
                ctx.state.attach_anchor = ev.anchor;
            }
            Msg::AttachImagesChanged(imgs) => {
                ctx.state.attach_images = imgs;
            }
            Msg::RemoveAttachment(ev) => {
                if ev.index < ctx.state.attach_images.len() {
                    ctx.state.attach_images.remove(ev.index);
                }
            }
            Msg::SeedInline => {
                // Insert two dummy images with sentinel chars into the text.
                let img = dummy_image();
                let s0 = char::from_u32(IMAGE_SENTINEL_BASE as u32).unwrap();
                let s1 = char::from_u32(IMAGE_SENTINEL_BASE as u32 + 1).unwrap();
                let text = format!("Before {s0} middle {s1} after");
                ctx.state.inline_cursor = text.len();
                ctx.state.inline_value = text;
                ctx.state.inline_anchor = None;
                ctx.state.inline_images = vec![img.clone(), img];
            }
            Msg::SeedAttach => {
                let img = dummy_image();
                ctx.state.attach_value = "Caption for attached images".into();
                ctx.state.attach_cursor = ctx.state.attach_value.len();
                ctx.state.attach_anchor = None;
                ctx.state.attach_images = vec![img.clone(), img.clone(), img];
            }
            Msg::ClearInline => {
                ctx.state.inline_value.clear();
                ctx.state.inline_cursor = 0;
                ctx.state.inline_anchor = None;
                ctx.state.inline_images.clear();
            }
            Msg::ClearAttach => {
                ctx.state.attach_value.clear();
                ctx.state.attach_cursor = 0;
                ctx.state.attach_anchor = None;
                ctx.state.attach_images.clear();
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let inline_status = if ctx.state.inline_images.is_empty() {
            "No images".to_string()
        } else {
            format!("{} image(s) inline", ctx.state.inline_images.len())
        };
        let attach_status = if ctx.state.attach_images.is_empty() {
            "No images".to_string()
        } else {
            format!("{} image(s) attached", ctx.state.attach_images.len())
        };

        // Build attachment chip bar (only when images present).
        let attach_chip_bar = if !ctx.state.attach_images.is_empty() {
            Some(
                DraggableTabBar::new()
                    .tabs(ctx.state.attach_images.iter().enumerate().map(|(i, _)| {
                        DraggableTab::new(format!("Photo {}", i + 1)).closeable(true)
                    }))
                    .active(usize::MAX) // no active tab - these are chips
                    .style(Style::new().fg(Color::Green))
                    .active_style(Style::new().fg(Color::Green))
                    .close_style(Style::new().fg(Color::indexed(246)).dim())
                    .close_hover_style(Style::new().fg(Color::LightRed).bold())
                    .draggable(false)
                    .focusable(false)
                    .scroll_wheel(true)
                    .show_overflow_controls(true)
                    .on_close(ctx.link().callback(Msg::RemoveAttachment)),
            )
        } else {
            None
        };

        rsx! {
            Frame {
                title: "TextArea Image Modes",
                padding: 1,
                HStack {
                    gap: 2,
                    VStack {
                        gap: 1,
                        Text {
                            content: "Inline Mode",
                            style: Style::new().bold().fg(Color::Cyan),
                        },
                        Text {
                            content: "Sentinels embedded in text value.",
                            style: Style::new().dim(),
                        },
                        TextArea {
                            value: ctx.state.inline_value.clone(),
                            cursor: ctx.state.inline_cursor,
                            anchor: ctx.state.inline_anchor,
                            placeholder: "Type or Ctrl+V to paste image inline...",
                            border: true,
                            border_style: BorderStyle::Rounded,
                            height: Length::Px(8),
                            wrap: true,
                            scrollbar: false,
                            on_change: ctx.link().callback(Msg::InlineChanged),
                            image_mode: TextAreaImageMode::Inline,
                            images: ctx.state.inline_images.clone(),
                            on_images_change: ctx.link().callback(Msg::InlineImagesChanged),
                            image_placeholder: "[Img]",
                            image_placeholder_style: Style::new().fg(Color::Magenta).bold(),
                            image_placeholder_focus_style: Style::new().fg(Color::LightMagenta).bold(),
                        },
                        Text {
                            content: inline_status,
                            style: Style::new().dim(),
                        },
                        HStack {
                            height: Length::Auto,
                            gap: 1,
                            Button {
                                label: "Seed",
                                on_click: ctx.link().callback(|_| Msg::SeedInline),
                                variant: ButtonVariant::Outlined,
                                style: Style::new().fg(Color::Cyan),
                            },
                            Button {
                                label: "Clear",
                                on_click: ctx.link().callback(|_| Msg::ClearInline),
                                variant: ButtonVariant::Outlined,
                                style: Style::new().fg(Color::Red),
                            },
                        },
                    },
                    Divider {
                        orientation: Orientation::Vertical,
                        join_frame: true,
                    },
                    VStack {
                        gap: 1,
                        Text {
                            content: "Attachment Mode",
                            style: Style::new().bold().fg(Color::Green),
                        },
                        Text {
                            content: "Chip bar (DraggableTabBar) above text.",
                            style: Style::new().dim(),
                        },
                        VStack {
                            gap: 0,
                            height: Length::Auto,
                            if let Some(bar) = attach_chip_bar {
                                bar,
                            },
                            TextArea {
                                value: ctx.state.attach_value.clone(),
                                cursor: ctx.state.attach_cursor,
                                anchor: ctx.state.attach_anchor,
                                placeholder: "Type or Ctrl+V to attach image...",
                                border: true,
                                border_style: BorderStyle::Rounded,
                                height: Length::Px(8),
                                wrap: true,
                                scrollbar: false,
                                on_change: ctx.link().callback(Msg::AttachChanged),
                                image_mode: TextAreaImageMode::Attachment,
                                images: ctx.state.attach_images.clone(),
                                on_images_change: ctx.link().callback(Msg::AttachImagesChanged),
                            },
                        },
                        Text {
                            content: attach_status,
                            style: Style::new().dim(),
                        },
                        HStack {
                            height: Length::Auto,
                            gap: 1,
                            Button {
                                label: "Seed",
                                on_click: ctx.link().callback(|_| Msg::SeedAttach),
                                variant: ButtonVariant::Outlined,
                                style: Style::new().fg(Color::Green),
                            },
                            Button {
                                label: "Clear",
                                on_click: ctx.link().callback(|_| Msg::ClearAttach),
                                variant: ButtonVariant::Outlined,
                                style: Style::new().fg(Color::Red),
                            },
                        },
                    },
                },
            }
        }
    }
}

fn main() -> Result<()> {
    App::new().mount(ImageModesDemo).run()
}
