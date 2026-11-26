//! Tooltip widget.

use crate::animation::VisibilityAnimation;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement, Key};
use crate::core::runtime_env::RuntimeEnv;
use crate::style::{BorderStyle, Length, Padding, Style};
use crate::widgets::{Frame, MouseRegion, Popover, PopoverOffset, PopoverPlacement, Text};
use web_time::Instant;

static TOOLTIP_ID: AtomicUsize = AtomicUsize::new(1);

/// Default [`Tooltip::skip_delay`]: how long after a delayed tooltip closes that the next one
/// still opens at once.
pub const DEFAULT_TOOLTIP_SKIP_DELAY: Duration = Duration::from_millis(300);

/// What a [`Tooltip`] shows: plain text, or any element for a rich card.
///
/// Strings convert to [`TooltipContent::Text`] and anything that converts into an [`Element`] to
/// [`TooltipContent::Element`], so [`Tooltip::new`] accepts either.
#[derive(Clone)]
pub enum TooltipContent {
    /// Plain text, styled by [`Tooltip::text_style`].
    Text(Arc<str>),
    /// An arbitrary element. It must stay passive: the tooltip never takes focus or pointer input.
    Element(Box<Element>),
}

impl<E: IntoElement> From<E> for TooltipContent {
    fn from(element: E) -> Self {
        Self::Element(Box::new(element.into()))
    }
}

impl From<&str> for TooltipContent {
    fn from(text: &str) -> Self {
        Self::Text(text.into())
    }
}

impl From<String> for TooltipContent {
    fn from(text: String) -> Self {
        Self::Text(text.into())
    }
}

impl From<Arc<str>> for TooltipContent {
    fn from(text: Arc<str>) -> Self {
        Self::Text(text)
    }
}

impl From<&String> for TooltipContent {
    fn from(text: &String) -> Self {
        Self::Text(text.as_str().into())
    }
}

impl From<Box<str>> for TooltipContent {
    fn from(text: Box<str>) -> Self {
        Self::Text(text.into())
    }
}

impl From<std::borrow::Cow<'_, str>> for TooltipContent {
    fn from(text: std::borrow::Cow<'_, str>) -> Self {
        Self::Text(text.into())
    }
}

impl From<&Arc<str>> for TooltipContent {
    fn from(text: &Arc<str>) -> Self {
        Self::Text(text.clone())
    }
}

/// How a tooltip looks and when it opens: its timing, surface and placement.
///
/// [`Tooltip`] carries one, and widgets that show tooltips for parts of themselves take one
/// directly, such as [`DocumentView::tooltips`](crate::widgets::DocumentView::tooltips). Build it
/// once and pass it to each, so every tooltip in an app opens and looks the same.
#[derive(Clone)]
pub struct TooltipConfig {
    pub(crate) animation: Option<VisibilityAnimation>,
    pub(crate) delay: Duration,
    skip_delay: Duration,
    text_style: Style,
    container_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    max_width: Option<Length>,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
}

impl Default for TooltipConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TooltipConfig {
    /// An immediate, borderless, unstyled tooltip placed below the start of its trigger.
    pub fn new() -> Self {
        Self {
            animation: None,
            delay: Duration::ZERO,
            skip_delay: DEFAULT_TOOLTIP_SKIP_DELAY,
            text_style: Style::default(),
            container_style: Style::default(),
            border: false,
            border_style: BorderStyle::Plain,
            padding: 0.into(),
            max_width: None,
            placement: PopoverPlacement::BelowStart,
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
        }
    }

    /// Animate popup content using shared visibility timing and effects.
    /// Root-portal scope only; the trigger remains active while content closes.
    pub fn animation(mut self, animation: impl Into<Option<VisibilityAnimation>>) -> Self {
        self.animation = animation.into();
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

    /// Set the style of text content. Element content carries its own styles.
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

    /// Cap the popup width. Percent values resolve against the overlay bounds.
    pub fn max_width(mut self, max_width: Length) -> Self {
        self.max_width = Some(max_width);
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

    /// The passive popover showing `content` beside `trigger`.
    pub(crate) fn popover(
        &self,
        trigger: Element,
        content: &TooltipContent,
        open: bool,
    ) -> Popover {
        let body = match content {
            TooltipContent::Text(text) => Text::new(text.clone()).style(self.text_style).into(),
            TooltipContent::Element(element) => (**element).clone(),
        };
        let content = Frame::new()
            .border(self.border)
            .border_style(self.border_style)
            .padding(self.padding)
            .style(self.container_style)
            .child(body);
        let mut popover = Popover::new()
            .animation(self.animation.clone())
            .trigger(trigger)
            .content(content)
            .open(open)
            .capture_focus(false)
            .capture_pointer(false)
            .auto_focus(false)
            .min_trigger_width(false)
            .placement(self.placement)
            .offset(self.offset)
            .clamp(self.clamp)
            .auto_flip(self.auto_flip);
        if let Some(max_width) = self.max_width {
            popover = popover.max_width(max_width);
        }
        popover
    }
}

/// What a hover change asks of the component that owns a [`HoverReveal`].
#[must_use]
pub(crate) enum RevealStep {
    /// Nothing visible changed.
    Nothing,
    /// The tooltip opened or closed: render again.
    Render,
    /// Send a reveal for `generation` after `wait`.
    Arm { wait: Duration, generation: u64 },
}

/// The hover timing behind a delayed tooltip: wait [`TooltipConfig::delay`] after the pointer
/// arrives, and skip the wait while another tooltip closed moments ago.
#[derive(Clone, Debug, Default)]
pub(crate) struct HoverReveal {
    /// Whether the pointer is over the trigger.
    hovered: bool,
    /// Whether the tooltip has finished waiting and is shown.
    revealed: bool,
    /// Bumped on every hover change and timing change so a timer armed earlier is ignored.
    generation: u64,
    /// When the current hover began, so a changed delay counts from the original entry.
    hovered_since: Option<Instant>,
}

impl HoverReveal {
    pub(crate) fn revealed(&self) -> bool {
        self.revealed
    }

    /// The pointer entered (`true`) or left (`false`) the trigger.
    pub(crate) fn hover(
        &mut self,
        hovered: bool,
        config: &TooltipConfig,
        env: &RuntimeEnv,
    ) -> RevealStep {
        self.hovered = hovered;
        self.generation = self.generation.wrapping_add(1);
        if !hovered {
            self.hovered_since = None;
            if !std::mem::take(&mut self.revealed) {
                return RevealStep::Nothing;
            }
            // Only a delayed tooltip the user actually saw warms its neighbours: one that opens at
            // once never made the user wait, as with a plain `Tooltip` that has no delay.
            if !config.delay.is_zero() {
                env.tooltip_closed_at.set(Some(env.now()));
            }
            return RevealStep::Render;
        }
        self.hovered_since = Some(env.now());
        let warm = env
            .tooltip_closed_at
            .get()
            .is_some_and(|closed| env.elapsed(closed) < config.skip_delay);
        if warm {
            self.revealed = true;
            return RevealStep::Render;
        }
        self.arm(config.delay)
    }

    /// A timer armed for `generation` elapsed; returns whether the tooltip opened.
    pub(crate) fn reveal(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.hovered {
            return false;
        }
        self.revealed = true;
        true
    }

    /// Hover stopped being timed here: forget it without warming other tooltips.
    pub(crate) fn reset(&mut self) -> RevealStep {
        self.generation = self.generation.wrapping_add(1);
        self.hovered = false;
        self.hovered_since = None;
        if std::mem::take(&mut self.revealed) {
            RevealStep::Render
        } else {
            RevealStep::Nothing
        }
    }

    /// The delay changed: re-arm the current hover, counting from when it began.
    pub(crate) fn retime(&mut self, config: &TooltipConfig, env: &RuntimeEnv) -> RevealStep {
        // Whatever timer is pending was armed for the old delay.
        self.generation = self.generation.wrapping_add(1);
        let Some(since) = self.hovered_since.filter(|_| self.hovered) else {
            return RevealStep::Nothing;
        };
        if self.revealed {
            return RevealStep::Nothing;
        }
        let waited = env.elapsed(since);
        self.arm(config.delay.saturating_sub(waited))
    }

    /// Schedule a reveal for the current hover after `wait`, or reveal now when nothing is left.
    fn arm(&mut self, wait: Duration) -> RevealStep {
        if wait.is_zero() {
            self.revealed = true;
            return RevealStep::Render;
        }
        RevealStep::Arm {
            wait,
            generation: self.generation,
        }
    }
}

/// Forward [`TooltipConfig`] builders onto a widget that holds one in `config`.
macro_rules! forward_tooltip_config {
    ($($(#[$doc:meta])* $name:ident($arg:ident: $ty:ty);)*) => {
        $(
            $(#[$doc])*
            pub fn $name(mut self, $arg: $ty) -> Self {
                self.config = self.config.$name($arg);
                self
            }
        )*
    };
}

/// A passive popup shown on hover or focus: help text or a rich, non-interactive card.
#[derive(Clone)]
pub struct Tooltip {
    child: Element,
    content: TooltipContent,
    open: Option<bool>,
    auto: bool,
    show_on_focus: bool,
    config: TooltipConfig,
}

impl Tooltip {
    /// Create a tooltip showing `content`: a string, or any element for a rich card.
    pub fn new(content: impl Into<TooltipContent>) -> Self {
        Self {
            child: crate::widgets::Spacer::new().into(),
            content: content.into(),
            open: None,
            auto: true,
            show_on_focus: true,
            config: TooltipConfig::new(),
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

    /// Replace the timing, surface and placement with a shared [`TooltipConfig`].
    pub fn config(mut self, config: TooltipConfig) -> Self {
        self.config = config;
        self
    }

    forward_tooltip_config! {
        /// Animate popup content using shared visibility timing and effects.
        /// Root-portal scope only; the trigger remains active while content closes.
        animation(animation: impl Into<Option<VisibilityAnimation>>);
        /// Wait this long after the pointer enters the trigger before showing the tooltip.
        ///
        /// Zero (the default) shows it at once. Leaving the trigger before the delay elapses
        /// cancels it, so sweeping the pointer across a row of controls opens nothing. Focus is
        /// not delayed.
        delay(delay: Duration);
        /// Skip [`delay`](Self::delay) when another delayed tooltip closed less than this long
        /// ago.
        ///
        /// Once the user has waited for one tooltip, moving to a neighbouring control shows its
        /// tooltip immediately, as desktop toolkits do. Defaults to
        /// [`DEFAULT_TOOLTIP_SKIP_DELAY`]; zero always waits.
        skip_delay(skip_delay: Duration);
        /// Set the style of text content. Element content carries its own styles.
        text_style(style: Style);
        /// Set container style.
        container_style(style: Style);
        /// Draw a border around the tooltip.
        border(border: bool);
        /// Set border style.
        border_style(border_style: BorderStyle);
        /// Set padding.
        padding(padding: impl Into<Padding>);
        /// Cap the popup width. Percent values resolve against the overlay bounds.
        max_width(max_width: Length);
        /// Set popover placement.
        placement(placement: PopoverPlacement);
        /// Set popover offset.
        offset(offset: impl Into<PopoverOffset>);
        /// Clamp popover to the viewport bounds.
        clamp(clamp: bool);
        /// Automatically flip placement when it overflows the viewport.
        auto_flip(auto_flip: bool);
    }
}

#[derive(Clone)]
struct TooltipProps {
    child: Element,
    content: TooltipContent,
    open: Option<bool>,
    auto: bool,
    show_on_focus: bool,
    config: TooltipConfig,
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
            content: tooltip.content,
            open: tooltip.open,
            auto: tooltip.auto,
            show_on_focus: tooltip.show_on_focus,
            config: tooltip.config,
        }
    }
}

#[derive(Clone, Debug)]
struct TooltipState {
    trigger_key: Key,
    /// Hover timing, used only for delayed tooltips.
    reveal: HoverReveal,
}

impl TooltipProps {
    /// Whether hover opens this tooltip after [`Tooltip::delay`]. A controlled tooltip
    /// ([`Tooltip::open`]) never runs the hover timer, so it can neither reveal nor warm others.
    fn delays_hover(&self) -> bool {
        self.auto && self.open.is_none() && !self.config.delay.is_zero()
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

    fn step(step: RevealStep, ctx: &mut Context<Self>) -> Update {
        match step {
            RevealStep::Nothing => Update::none(),
            RevealStep::Render => Update::render(),
            RevealStep::Arm { wait, generation } => {
                Update::command_only(ctx.link().command_after(wait, move |link| {
                    link.send(TooltipMsg::Reveal(generation));
                }))
            }
        }
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
            reveal: HoverReveal::default(),
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let delays = ctx.props.delays_hover();
        if delays == old_props.delays_hover() && ctx.props.config.delay == old_props.config.delay {
            return Update::none();
        }
        let step = if delays {
            let config = ctx.props.config.clone();
            let env = ctx.env().clone();
            ctx.state.reveal.retime(&config, &env)
        } else {
            // Hover is no longer tracked here; closing this way does not warm other tooltips.
            ctx.state.reveal.reset()
        };
        Self::step(step, ctx)
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TooltipMsg::Hover(hovered) => {
                if !ctx.props.delays_hover() {
                    return Update::none();
                }
                let config = ctx.props.config.clone();
                let env = ctx.env().clone();
                let step = ctx.state.reveal.hover(hovered, &config, &env);
                Self::step(step, ctx)
            }
            TooltipMsg::Reveal(generation) => {
                if ctx.state.reveal.reveal(generation) {
                    Update::render()
                } else {
                    Update::none()
                }
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
        let trigger: Element = region.into();
        let trigger = trigger.key(ctx.state.trigger_key.clone());

        let hover_open = if delayed {
            ctx.state.reveal.revealed()
        } else {
            ctx.has_hover_within_key(ctx.state.trigger_key.clone())
        };
        let auto_open = ctx.props.auto
            && (hover_open
                || (ctx.props.show_on_focus
                    && ctx.has_focus_within_key(ctx.state.trigger_key.clone())));
        let open = ctx.props.open.unwrap_or(auto_open);
        ctx.props
            .config
            .popover(trigger, &ctx.props.content, open)
            .into()
    }
}

impl From<Tooltip> for Element {
    fn from(tooltip: Tooltip) -> Self {
        crate::child(TooltipComponent::new, TooltipProps::from(tooltip))
    }
}
