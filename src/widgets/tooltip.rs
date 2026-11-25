//! Tooltip widget.

use crate::animation::VisibilityAnimation;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement, Key};
use crate::style::{BorderStyle, Padding, Style};
use crate::widgets::{Frame, MouseRegion, Popover, PopoverOffset, PopoverPlacement, Text};

static TOOLTIP_ID: AtomicUsize = AtomicUsize::new(1);

/// Default [`Tooltip::skip_delay`]: how long after a delayed tooltip closes that the next one
/// still opens at once.
pub const DEFAULT_TOOLTIP_SKIP_DELAY: Duration = Duration::from_millis(300);

/// A tooltip widget that displays help text on hover or focus.
#[derive(Clone)]
pub struct Tooltip {
    child: Element,
    text: Arc<str>,
    open: Option<bool>,
    pub(crate) animation: Option<VisibilityAnimation>,
    auto: bool,
    show_on_focus: bool,
    delay: Duration,
    skip_delay: Duration,
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
    /// Animate popup content using shared visibility timing and effects.
    /// Root-portal scope only; the trigger remains active while content closes.
    pub fn animation(mut self, animation: impl Into<Option<VisibilityAnimation>>) -> Self {
        self.animation = animation.into();
        self
    }

    /// Create a new tooltip wrapping a child element.
    pub fn new(text: impl Into<Arc<str>>) -> Self {
        Self {
            child: crate::widgets::Spacer::new().into(),
            text: text.into(),
            open: None,
            animation: None,
            auto: true,
            show_on_focus: true,
            delay: Duration::ZERO,
            skip_delay: DEFAULT_TOOLTIP_SKIP_DELAY,
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
        self.open = Some(open);
        self
    }

    /// Enable or disable auto-open on hover/focus.
    pub fn auto(mut self, auto: bool) -> Self {
        self.auto = auto;
        self
    }

    /// Enable or disable automatic display while the trigger has focus.
    pub fn show_on_focus(mut self, show_on_focus: bool) -> Self {
        self.show_on_focus = show_on_focus;
        self
    }

    /// Wait this long after the pointer enters the trigger before showing the tooltip.
    ///
    /// Zero (the default) shows it at once. Leaving the trigger before the delay elapses cancels
    /// it, so sweeping the pointer across a row of controls opens nothing. Focus is not delayed.
    pub fn delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// Skip [`delay`](Self::delay) when another delayed tooltip closed less than this long ago.
    ///
    /// Once the user has waited for one tooltip, moving to a neighbouring control shows its
    /// tooltip immediately, as desktop toolkits do. Defaults to [`DEFAULT_TOOLTIP_SKIP_DELAY`];
    /// zero always waits.
    pub fn skip_delay(mut self, skip_delay: Duration) -> Self {
        self.skip_delay = skip_delay;
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
    open: Option<bool>,
    pub(crate) animation: Option<VisibilityAnimation>,
    auto: bool,
    show_on_focus: bool,
    delay: Duration,
    skip_delay: Duration,
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
    /// Always changed: the child is a fresh element on every parent render, and nothing short of
    /// rendering it tells a changed style or callback from an unchanged one (its layout hash only
    /// covers geometry). Re-rendering keeps the state and the hover timer.
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

impl From<Tooltip> for TooltipProps {
    fn from(tooltip: Tooltip) -> Self {
        Self {
            child: tooltip.child,
            text: tooltip.text,
            open: tooltip.open,
            animation: tooltip.animation,
            auto: tooltip.auto,
            show_on_focus: tooltip.show_on_focus,
            delay: tooltip.delay,
            skip_delay: tooltip.skip_delay,
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
    /// Whether the pointer is over the trigger, tracked only for delayed tooltips.
    hovered: bool,
    /// Whether a delayed tooltip has finished waiting and is shown for hover.
    revealed: bool,
    /// Bumped on every hover change and timing change so a timer armed earlier is ignored.
    hover_generation: u64,
    /// When the current hover began, so a changed delay counts from the original entry.
    hovered_since: Option<web_time::Instant>,
}

impl TooltipProps {
    /// Whether hover opens this tooltip after [`Tooltip::delay`]. A controlled tooltip
    /// ([`Tooltip::open`]) never runs the hover timer, so it can neither reveal nor warm others.
    fn delays_hover(&self) -> bool {
        self.auto && self.open.is_none() && !self.delay.is_zero()
    }
}

enum TooltipMsg {
    Hover(bool),
    Reveal(u64),
}

struct TooltipComponent;

impl TooltipComponent {
    fn new() -> Self {
        Self
    }

    fn hover(hovered: bool, ctx: &mut Context<Self>) -> Update {
        if !ctx.props.delays_hover() {
            return Update::none();
        }
        ctx.state.hovered = hovered;
        ctx.state.hover_generation = ctx.state.hover_generation.wrapping_add(1);
        if !hovered {
            ctx.state.hovered_since = None;
            if !std::mem::take(&mut ctx.state.revealed) {
                return Update::none();
            }
            // Only a tooltip the user actually saw warms its neighbours.
            let env = ctx.env();
            env.tooltip_closed_at.set(Some(env.now()));
            return Update::render();
        }
        let now = ctx.env().now();
        ctx.state.hovered_since = Some(now);
        let env = ctx.env();
        let warm = env
            .tooltip_closed_at
            .get()
            .is_some_and(|closed| env.elapsed(closed) < ctx.props.skip_delay);
        if warm {
            ctx.state.revealed = true;
            return Update::render();
        }
        Self::arm(ctx.props.delay, ctx)
    }

    /// Schedule a reveal for the current hover after `wait`, or reveal now when nothing is left.
    fn arm(wait: Duration, ctx: &mut Context<Self>) -> Update {
        if wait.is_zero() {
            ctx.state.revealed = true;
            return Update::render();
        }
        let generation = ctx.state.hover_generation;
        Update::command_only(ctx.link().command_after(wait, move |link| {
            link.send(TooltipMsg::Reveal(generation));
        }))
    }
}

impl Component for TooltipComponent {
    type Message = TooltipMsg;
    type Properties = TooltipProps;
    type State = TooltipState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let id = TOOLTIP_ID.fetch_add(1, Ordering::Relaxed);
        TooltipState {
            trigger_key: Key::from(format!("tooltip-trigger-{}", id)),
            hovered: false,
            revealed: false,
            hover_generation: 0,
            hovered_since: None,
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let delays = ctx.props.delays_hover();
        if delays == old_props.delays_hover() && ctx.props.delay == old_props.delay {
            return Update::none();
        }
        // Whatever timer is pending was armed for the old configuration.
        ctx.state.hover_generation = ctx.state.hover_generation.wrapping_add(1);
        if !delays {
            // Hover is no longer tracked here; closing this way does not warm other tooltips.
            ctx.state.hovered = false;
            ctx.state.hovered_since = None;
            return if std::mem::take(&mut ctx.state.revealed) {
                Update::render()
            } else {
                Update::none()
            };
        }
        let Some(since) = ctx.state.hovered_since.filter(|_| ctx.state.hovered) else {
            return Update::none();
        };
        if ctx.state.revealed {
            return Update::none();
        }
        let waited = ctx.env().elapsed(since);
        Self::arm(ctx.props.delay.saturating_sub(waited), ctx)
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TooltipMsg::Hover(hovered) => Self::hover(hovered, ctx),
            TooltipMsg::Reveal(generation) => {
                if generation != ctx.state.hover_generation || !ctx.state.hovered {
                    return Update::none();
                }
                ctx.state.revealed = true;
                Update::render()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let delayed = ctx.props.delays_hover();
        // The trigger always sits in the same region, whatever the configuration: adding or
        // removing a wrapper would change the child's container path and remount a stateful
        // trigger. Only the hover callback comes and goes. The region takes the child's sizing, so a
        // flexible child stays flexible, and its reconciled rectangle, so the popup anchors to the
        // child and hover counts only over it.
        let mut region = MouseRegion::new()
            .child(ctx.props.child.clone())
            .fitted_to_child();
        if delayed {
            region = region.on_hover_change(ctx.link().callback(TooltipMsg::Hover));
        }
        let mut trigger: Element = region.into();
        trigger.layout = ctx.props.child.layout_constraints();
        let trigger = trigger.key(ctx.state.trigger_key.clone());

        let hover_open = if delayed {
            ctx.state.revealed
        } else {
            ctx.has_hover_within_key(ctx.state.trigger_key.clone())
        };
        let auto_open = ctx.props.auto
            && (hover_open
                || (ctx.props.show_on_focus
                    && ctx.has_focus_within_key(ctx.state.trigger_key.clone())));
        let open = ctx.props.open.unwrap_or(auto_open);

        let content = Frame::new()
            .border(ctx.props.border)
            .border_style(ctx.props.border_style)
            .padding(ctx.props.padding)
            .style(ctx.props.container_style)
            .child(Text::new(ctx.props.text.clone()).style(ctx.props.text_style));

        Popover::new()
            .animation(ctx.props.animation.clone())
            .trigger(trigger)
            .content(content)
            .open(open)
            .capture_focus(false)
            .capture_pointer(false)
            .auto_focus(false)
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
