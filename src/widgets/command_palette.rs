use std::sync::Arc;

use crate::app::input::command_registry::{CommandEntry, CommandId, CommandRegistry};
use crate::callback::Callback;
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::overlay::OverlayScope;
use crate::style::{Align, BorderStyle, Length, Padding, RichText, Span, Style};
use crate::widgets::{
    ItemDescription, ListItem, Modal, SearchEntry, SearchHighlight, SearchItem, SearchPalette,
};

#[derive(Clone, PartialEq)]
struct CommandPaletteProps {
    on_close: Option<Callback<()>>,
    show_disabled: bool,
    width: Length,
    height: Length,
    backdrop_style: Style,
    frame_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    title: Option<RichText>,
    title_style: Style,
    title_alignment: Align,
    scope: OverlayScope,
}

#[derive(Clone, Default, PartialEq)]
struct CommandPaletteState {
    entries: Vec<SearchEntry<CommandId>>,
    disabled_ids: Vec<CommandId>,
}

#[derive(Clone, PartialEq)]
struct DisabledRenderStyle {
    style: Style,
    disabled_ids: Vec<CommandId>,
}

struct CommandPaletteComponent;

impl Component for CommandPaletteComponent {
    type Message = ();
    type Properties = CommandPaletteProps;
    type State = CommandPaletteState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        CommandPaletteState::default()
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
        rebuild_state(ctx);
        None
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        if old_props.show_disabled != ctx.props.show_disabled
            || old_props.on_close != ctx.props.on_close
            || old_props.width != ctx.props.width
            || old_props.height != ctx.props.height
            || old_props.backdrop_style != ctx.props.backdrop_style
            || old_props.frame_style != ctx.props.frame_style
            || old_props.border != ctx.props.border
            || old_props.border_style != ctx.props.border_style
            || old_props.padding != ctx.props.padding
            || old_props.title != ctx.props.title
            || old_props.title_style != ctx.props.title_style
            || old_props.title_alignment != ctx.props.title_alignment
            || old_props.scope != ctx.props.scope
        {
            rebuild_state(ctx);
            return Update::full();
        }
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let muted_style = ctx.theme().muted.dim();
        let disabled_style = DisabledRenderStyle {
            style: muted_style,
            disabled_ids: ctx.state.disabled_ids.clone(),
        };
        let render_item = Arc::new(
            move |item: &SearchItem<CommandId>, _highlight: &SearchHighlight| {
                if !disabled_style
                    .disabled_ids
                    .iter()
                    .any(|id| id == &item.value)
                {
                    return None;
                }

                let mut spans = vec![Span::new(item.label.clone()).style(disabled_style.style)];
                if let Some(description) = &item.description
                    && let Some(left) = &description.left
                {
                    spans.push(Span::new(" - ").style(disabled_style.style));
                    spans.push(Span::new(left.clone()).style(disabled_style.style));
                }

                let mut row = ListItem::from_spans(spans).style(disabled_style.style);
                if let Some(description) = &item.description
                    && let Some(right) = &description.right
                {
                    row = row.description_spans([
                        Span::new(" ").style(disabled_style.style),
                        Span::new(right.clone()).style(disabled_style.style),
                    ]);
                }

                Some(row)
            },
        );

        let registry = ctx.command_registry();
        let disabled_for_activate = ctx.state.disabled_ids.clone();
        let on_close_for_activate = ctx.props.on_close.clone();
        let on_activate = Callback::new(move |event: crate::widgets::SearchEvent<CommandId>| {
            if disabled_for_activate
                .iter()
                .any(|id| id == &event.item.value)
            {
                return;
            }
            if registry.execute(event.item.value.clone())
                && let Some(on_close) = &on_close_for_activate
            {
                on_close.emit(());
            }
        });

        let palette = SearchPalette::<CommandId>::new()
            .entries(ctx.state.entries.clone())
            .height(Length::Flex(1))
            .input_border(false)
            .list_border(false)
            .list_selection_full_width(true)
            .preserve_groups(true)
            .on_activate(on_activate)
            .render_item(render_item);

        let mut modal = Modal::new()
            .child(palette)
            .width(ctx.props.width)
            .height(ctx.props.height)
            .backdrop_style(ctx.props.backdrop_style)
            .frame_style(ctx.props.frame_style)
            .border(ctx.props.border)
            .border_style(ctx.props.border_style)
            .padding(ctx.props.padding)
            .title_style(ctx.props.title_style)
            .title_alignment(ctx.props.title_alignment)
            .scope(ctx.props.scope);

        if let Some(title) = ctx.props.title.clone() {
            modal = modal.title(title);
        }
        if let Some(on_close) = ctx.props.on_close.clone() {
            modal = modal.on_close(on_close);
        }

        modal.into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

fn rebuild_state(ctx: &mut Context<CommandPaletteComponent>) {
    let registry = ctx.command_registry();
    let (entries, disabled_ids) = build_palette_entries(&registry, ctx.props.show_disabled);
    ctx.state.entries = entries;
    ctx.state.disabled_ids = disabled_ids;
}

fn build_palette_entries(
    registry: &CommandRegistry,
    show_disabled: bool,
) -> (Vec<SearchEntry<CommandId>>, Vec<CommandId>) {
    let mut commands: Vec<CommandEntry> = registry.entries();
    commands.sort_by(|left, right| {
        let left_category = left.category.as_deref().unwrap_or("General");
        let right_category = right.category.as_deref().unwrap_or("General");
        left_category
            .cmp(right_category)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.id.as_str().cmp(right.id.as_str()))
    });

    let mut entries = Vec::new();
    let mut disabled_ids = Vec::new();
    let mut active_category: Option<Arc<str>> = None;

    for command in commands {
        if !command.enabled {
            disabled_ids.push(command.id.clone());
            if !show_disabled {
                continue;
            }
        }

        let category = command.category.unwrap_or_else(|| Arc::from("General"));
        if active_category.as_ref() != Some(&category) {
            entries.push(SearchEntry::header(category.clone()));
            active_category = Some(category);
        }

        let mut entry = SearchEntry::item(command.label.clone(), command.id.clone());
        if command.description.is_some() || command.keybinding_hint.is_some() {
            let mut description = ItemDescription::new();
            if let Some(left) = command.description {
                description = description.left(left);
            }
            if let Some(right) = command.keybinding_hint {
                description = description.right(right);
            }
            entry = entry.description(description);
        }
        entries.push(entry);
    }

    (entries, disabled_ids)
}

/// Composite command palette built from the runtime [`CommandRegistry`].
#[derive(Clone)]
pub struct CommandPalette {
    props: CommandPaletteProps,
}

impl CommandPalette {
    /// Create a command palette with modal defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set close callback fired for both modal close and command activation.
    pub fn on_close(mut self, cb: Callback<()>) -> Self {
        self.props.on_close = Some(cb);
        self
    }

    /// Include disabled commands in search results.
    pub fn show_disabled(mut self, show: bool) -> Self {
        self.props.show_disabled = show;
        self
    }

    /// Set modal width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set modal height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Set modal backdrop style.
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.props.backdrop_style = style;
        self
    }

    /// Set modal frame style.
    pub fn frame_style(mut self, style: Style) -> Self {
        self.props.frame_style = style;
        self
    }

    /// Enable or disable modal border.
    pub fn border(mut self, border: bool) -> Self {
        self.props.border = border;
        self
    }

    /// Set modal border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.border_style = border_style;
        self
    }

    /// Set modal content padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.padding = padding.into();
        self
    }

    /// Set modal title.
    pub fn title(mut self, title: impl Into<RichText>) -> Self {
        self.props.title = Some(title.into());
        self
    }

    /// Set modal title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.props.title_style = style;
        self
    }

    /// Set modal title alignment.
    pub fn title_alignment(mut self, alignment: Align) -> Self {
        self.props.title_alignment = alignment;
        self
    }

    /// Set overlay scope.
    pub fn scope(mut self, scope: OverlayScope) -> Self {
        self.props.scope = scope;
        self
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self {
            props: CommandPaletteProps {
                on_close: None,
                show_disabled: false,
                width: Length::Px(80),
                height: Length::Px(20),
                backdrop_style: Style::default(),
                frame_style: Style::default(),
                border: true,
                border_style: BorderStyle::Plain,
                padding: 0.into(),
                title: None,
                title_style: Style::default(),
                title_alignment: Align::Start,
                scope: OverlayScope::RootPortal,
            },
        }
    }
}

impl From<CommandPalette> for Element {
    fn from(palette: CommandPalette) -> Self {
        crate::child(|| CommandPaletteComponent, palette.props)
    }
}
