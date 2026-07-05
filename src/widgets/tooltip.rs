//! Tooltip widget.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement, Key};
use crate::style::{BorderStyle, Padding, Style};
use crate::widgets::{Frame, Popover, PopoverOffset, PopoverPlacement, Text};

static TOOLTIP_ID: AtomicUsize = AtomicUsize::new(1);

/// A tooltip widget that displays help text on hover or focus.
#[derive(Clone)]
pub struct Tooltip {
    child: Element,
    text: Arc<str>,
    open: bool,
    auto: bool,
    text_style: Style,
    container_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
}

impl Tooltip {
    /// Create a new tooltip wrapping a child element.
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        Self {
            child: crate::widgets::Spacer::new().into(),
            text: text.into(),
            open: false,
            auto: true,
            text_style: Style::default(),
            container_style: Style::default(),
            border: false,
            border_style: BorderStyle::Plain,
            padding: 0.into(),
            placement: PopoverPlacement::BelowStart,
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
        }
    }

    /// Set the child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into();
        self
    }

    /// Set open state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Enable or disable auto-open on hover/focus.
    pub fn auto(mut self, auto: bool) -> Self {
        self.auto = auto;
        self
    }

    /// Set text style.
    pub fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// Set container style.
    pub fn container_style(mut self, style: Style) -> Self {
        self.container_style = style;
        self
    }

    /// Draw a border around the tooltip.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set popover placement.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Set popover offset.
    pub fn offset(mut self, offset: impl Into<PopoverOffset>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Clamp popover to the viewport bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Automatically flip placement when it overflows the viewport.
    pub fn auto_flip(mut self, auto_flip: bool) -> Self {
        self.auto_flip = auto_flip;
        self
    }
}

#[derive(Clone)]
struct TooltipProps {
    child: Element,
    text: Arc<str>,
    open: bool,
    auto: bool,
    text_style: Style,
    container_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
}

impl PartialEq for TooltipProps {
    fn eq(&self, other: &Self) -> bool {
        if self.text != other.text
            || self.open != other.open
            || self.auto != other.auto
            || self.text_style != other.text_style
            || self.container_style != other.container_style
            || self.border != other.border
            || self.border_style != other.border_style
            || self.padding != other.padding
            || self.placement != other.placement
            || self.offset != other.offset
            || self.clamp != other.clamp
            || self.auto_flip != other.auto_flip
        {
            return false;
        }

        let left = crate::layout::hash::element_layout_hash(&self.child);
        let right = crate::layout::hash::element_layout_hash(&other.child);
        match (left, right) {
            (Some(left), Some(right)) => left == right,
            _ => false,
        }
    }
}

impl From<Tooltip> for TooltipProps {
    fn from(tooltip: Tooltip) -> Self {
        Self {
            child: tooltip.child,
            text: tooltip.text,
            open: tooltip.open,
            auto: tooltip.auto,
            text_style: tooltip.text_style,
            container_style: tooltip.container_style,
            border: tooltip.border,
            border_style: tooltip.border_style,
            padding: tooltip.padding,
            placement: tooltip.placement,
            offset: tooltip.offset,
            clamp: tooltip.clamp,
            auto_flip: tooltip.auto_flip,
        }
    }
}

#[derive(Clone, Debug)]
struct TooltipState {
    trigger_key: Key,
}

struct TooltipComponent;

impl TooltipComponent {
    fn new() -> Self {
        Self
    }
}

impl Component for TooltipComponent {
    type Message = ();
    type Properties = TooltipProps;
    type State = TooltipState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let id = TOOLTIP_ID.fetch_add(1, Ordering::Relaxed);
        TooltipState {
            trigger_key: Key::from(format!("tooltip-trigger-{}", id)),
        }
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let trigger = ctx.props.child.clone().key(ctx.state.trigger_key.clone());

        let auto_open = ctx.props.auto
            && (ctx.has_hover_within_key(ctx.state.trigger_key.clone())
                || ctx.has_focus_within_key(ctx.state.trigger_key.clone()));
        let open = ctx.props.open || auto_open;

        let content = Frame::new()
            .border(ctx.props.border)
            .border_style(ctx.props.border_style)
            .padding(ctx.props.padding)
            .style(ctx.props.container_style)
            .child(Text::new(ctx.props.text.clone()).style(ctx.props.text_style));

        Popover::new()
            .trigger(trigger)
            .content(content)
            .open(open)
            .min_trigger_width(false)
            .placement(ctx.props.placement)
            .offset(ctx.props.offset)
            .clamp(ctx.props.clamp)
            .auto_flip(ctx.props.auto_flip)
            .into()
    }
}

impl From<Tooltip> for Element {
    fn from(tooltip: Tooltip) -> Self {
        crate::child(TooltipComponent::new, TooltipProps::from(tooltip))
    }
}
