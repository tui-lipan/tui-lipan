//! Tooltip widget.

use crate::animation::VisibilityAnimation;
use std::cell::Cell;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement, Key};
use crate::core::runtime_env::RuntimeEnv;
use crate::style::{BorderStyle, Length, Padding, Style};
use crate::widgets::{Frame, MouseRegion, Popover, PopoverOffset, PopoverPlacement, Text};
use web_time::Instant;

static TOOLTIP_ID: AtomicUsize = AtomicUsize::new(1);

/// How soon after another tooltip opened a closing tooltip counts as swapped out by it, covering
/// the two rendering in either order within one update.
const SWAP_WINDOW: Duration = Duration::from_millis(50);

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
    warmup_group: Option<Arc<str>>,
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
            warmup_group: None,
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

    /// Warm up only with tooltips in the same group.
    ///
    /// A delayed tooltip that closes lets the next one skip its [`delay`](Self::delay) only when
    /// both share a group. Tooltips without a group form one default group. Give a different
    /// kind of hover popup, such as a large preview card, its own group so that closing it does
    /// not open small control tooltips at once, and so that cards in a list each wait their delay
    /// when combined with a zero [`skip_delay`](Self::skip_delay).
    pub fn warmup_group(mut self, group: impl Into<Arc<str>>) -> Self {
        self.warmup_group = Some(group.into());
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
    /// Whether the current reveal skipped the delay, taking over from another tooltip: such a
    /// tooltip appears at once instead of fading in.
    instant: bool,
}

impl HoverReveal {
    pub(crate) fn revealed(&self) -> bool {
        self.revealed
    }

    /// Whether the current reveal took over from another tooltip without waiting.
    pub(crate) fn instant(&self) -> bool {
        self.revealed && self.instant
    }

    /// The tooltip, already revealed, takes over again from another one without waiting.
    pub(crate) fn mark_instant(&mut self) {
        self.instant = true;
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
                env.tooltip_closed_at
                    .borrow_mut()
                    .insert(config.warmup_group.clone(), env.now());
            }
            return RevealStep::Render;
        }
        self.hovered_since = Some(env.now());
        let warm = env
            .tooltip_closed_at
            .borrow()
            .get(&config.warmup_group)
            .is_some_and(|closed| env.elapsed(*closed) < config.skip_delay);
        if warm {
            self.revealed = true;
            self.instant = true;
            return RevealStep::Render;
        }
        self.arm(config.delay)
    }

    /// The pointer moved onto something inside the trigger that suspends the wait, such as a
    /// nested tooltip's trigger: cancel a pending reveal. A tooltip already revealed stays so.
    pub(crate) fn pause(&mut self) {
        if self.revealed {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.hovered = false;
        self.hovered_since = None;
    }

    /// The pointer entered the trigger to take over from a tooltip already showing: open at once.
    pub(crate) fn hover_revealed(&mut self, env: &RuntimeEnv) -> RevealStep {
        self.hovered = true;
        self.generation = self.generation.wrapping_add(1);
        self.hovered_since = Some(env.now());
        self.revealed = true;
        self.instant = true;
        RevealStep::Render
    }

    /// A timer armed for `generation` elapsed; returns whether the tooltip opened.
    pub(crate) fn reveal(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.hovered {
            return false;
        }
        self.revealed = true;
        self.instant = false;
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
        self.instant = false;
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
        /// Warm up only with tooltips in the same group; see [`TooltipConfig::warmup_group`].
        warmup_group(group: impl Into<Arc<str>>);
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
    /// Identifies this tooltip to the tooltips around it.
    id: usize,
    trigger_key: Key,
    /// Hover timing, used only for delayed tooltips.
    reveal: HoverReveal,
    /// Whether the pointer is on the trigger, nested triggers included.
    trigger_hovered: bool,
    /// The nested tooltips whose trigger has the pointer, each with a handle on whether its
    /// subtree is showing. While any has the pointer, this one steps aside.
    nested: ShowingHandle,
    /// The tooltip this one told the pointer is on its trigger, owed a leave.
    reported_to: Option<NestedTooltip>,
    /// Whether this tooltip is showing, or would be but for a hovered nested trigger, as of the
    /// last render. Nested tooltips read it to take over at once.
    shown: Arc<AtomicBool>,
    /// Whether the popup was open in the last render, to notice it opening and closing.
    visible: Cell<bool>,
    /// Whether another tooltip opened while this one faded out: the fade ends at once.
    fade_cut: Cell<bool>,
}

/// Whether a tooltip or any tooltip nested in it, at any depth, is showing. A tooltip hands one to
/// the tooltip enclosing it, so a pointer leaving several levels at once still carries the news
/// that a tooltip deep inside was showing.
#[derive(Clone, Default)]
struct ShowingHandle {
    own: Arc<AtomicBool>,
    nested: Arc<Mutex<Vec<(usize, ShowingHandle)>>>,
}

impl std::fmt::Debug for ShowingHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShowingHandle")
            .field("showing", &self.showing())
            .finish()
    }
}

impl ShowingHandle {
    fn list(&self) -> std::sync::MutexGuard<'_, Vec<(usize, ShowingHandle)>> {
        self.nested
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn showing(&self) -> bool {
        self.own.load(Ordering::Relaxed) || self.list().iter().any(|(_, nested)| nested.showing())
    }

    fn is_empty(&self) -> bool {
        self.list().is_empty()
    }

    fn contains(&self, id: usize) -> bool {
        self.list().iter().any(|(nested, _)| *nested == id)
    }

    fn insert(&self, id: usize, handle: ShowingHandle) {
        self.list().push((id, handle));
    }

    fn remove(&self, id: usize) -> bool {
        let mut nested = self.list();
        let Some(index) = nested.iter().position(|(nested, _)| *nested == id) else {
            return false;
        };
        nested.swap_remove(index);
        true
    }

    fn clear(&self) {
        self.list().clear();
    }
}

/// What a nested tooltip tells the tooltip whose trigger holds it.
#[derive(Clone)]
struct NestedHover {
    /// The nested tooltip.
    id: usize,
    /// The pointer entered (`true`) or left (`false`) the nested trigger.
    hovered: bool,
    /// Whether the nested tooltip, or one nested in it, was showing as the pointer crossed.
    shown: bool,
    /// Whether the nested tooltip's subtree is showing, kept while the pointer is on it.
    showing: ShowingHandle,
}

/// What a tooltip offers the tooltips nested in its trigger: a way to say the pointer is on them,
/// so it can step aside while their tooltip is the one that matters.
#[derive(Clone)]
struct NestedTooltip {
    /// The tooltip receiving the reports.
    owner: usize,
    report: crate::callback::Callback<NestedHover>,
    /// Whether each enclosing tooltip, this one and its own ancestors, is showing. A nested
    /// tooltip entered while any is takes over at once, however deep it sits.
    shown: Rc<[Arc<AtomicBool>]>,
}

impl std::fmt::Debug for NestedTooltip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NestedTooltip").finish_non_exhaustive()
    }
}

impl NestedTooltip {
    fn any_shown(&self) -> bool {
        self.shown.iter().any(|shown| shown.load(Ordering::Relaxed))
    }
}

impl PartialEq for NestedTooltip {
    /// Equal while it reaches the same tooltip: a fresh callback each render must not invalidate
    /// the trigger's subtree.
    fn eq(&self, other: &Self) -> bool {
        self.owner == other.owner
    }
}

impl TooltipProps {
    /// Whether hover opens this tooltip after [`Tooltip::delay`]. A controlled tooltip
    /// ([`Tooltip::open`]) never runs the hover timer, so it can neither reveal nor warm others.
    fn delays_hover(&self) -> bool {
        self.auto && self.open.is_none() && !self.config.delay.is_zero()
    }

    /// Whether this tooltip can show at all: one that cannot never makes an enclosing tooltip
    /// step aside.
    fn can_show(&self) -> bool {
        self.open.unwrap_or(self.auto)
    }
}

enum TooltipMsg {
    Hover(bool),
    Reveal(u64),
    /// The pointer entered or left the trigger of a tooltip nested in this one's trigger.
    Nested(NestedHover),
    /// Another tooltip opened while this one fades out.
    CutFade,
}

struct TooltipComponent;

impl TooltipComponent {
    fn new() -> Self {
        Self
    }

    /// Tell the enclosing tooltip whether the pointer is on this trigger, when that changed, this
    /// tooltip stopped or started being able to show, or the enclosing tooltip changed.
    fn sync_report(ctx: &mut Context<Self>) {
        let on = ctx.state.trigger_hovered && ctx.props.can_show();
        let target = on.then(|| ctx.use_context::<NestedTooltip>()).flatten();
        let report = |outer: &NestedTooltip, hovered: bool, ctx: &Context<Self>| {
            outer.report.emit(NestedHover {
                id: ctx.state.id,
                hovered,
                shown: ctx.state.nested.showing(),
                showing: ctx.state.nested.clone(),
            });
        };
        match (ctx.state.reported_to.take(), target) {
            (Some(reported), Some(target)) if reported.owner == target.owner => {
                ctx.state.reported_to = Some(target);
            }
            (reported, target) => {
                if let Some(reported) = &reported {
                    report(reported, false, ctx);
                }
                if let Some(target) = &target {
                    report(target, true, ctx);
                }
                ctx.state.reported_to = target;
            }
        }
    }

    /// A nested tooltip entered or left; `shown` says whether its subtree was showing as it left.
    fn nested(ctx: &mut Context<Self>, report: NestedHover) -> Update {
        let before = !ctx.state.nested.is_empty();
        if report.hovered {
            // A report from a pointer already gone from this trigger is stale.
            if !ctx.state.trigger_hovered || ctx.state.nested.contains(report.id) {
                return Update::none();
            }
            ctx.state.nested.insert(report.id, report.showing);
        } else if !ctx.state.nested.remove(report.id) {
            return Update::none();
        }
        let after = !ctx.state.nested.is_empty();
        if before == after {
            // Moving between two nested triggers: this one stays aside.
            return Update::none();
        }
        // A revealed tooltip is hidden, not closed, so it shows again at once when the pointer
        // comes back.
        if !ctx.props.delays_hover() || ctx.state.reveal.revealed() {
            if !after {
                ctx.state.reveal.mark_instant();
            }
            return Update::render();
        }
        // One still waiting counts only time spent on its own part of the trigger.
        if after {
            ctx.state.reveal.pause();
            return Update::none();
        }
        let env = ctx.env().clone();
        let step = if report.shown {
            // Back from a nested tooltip the user was reading: swap tooltips at once, as when
            // moving onto the nested trigger from this one.
            ctx.state.reveal.hover_revealed(&env)
        } else {
            let config = ctx.props.config.clone();
            ctx.state.reveal.hover(true, &config, &env)
        };
        Self::step(step, ctx)
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
        let shown: Arc<AtomicBool> = Arc::default();
        TooltipState {
            id,
            trigger_key: Key::from(format!("tooltip-trigger-{}", id)),
            reveal: HoverReveal::default(),
            trigger_hovered: false,
            nested: ShowingHandle {
                own: shown.clone(),
                nested: Arc::default(),
            },
            reported_to: None,
            shown,
            visible: Cell::new(false),
            fade_cut: Cell::new(false),
        }
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        // A tooltip that stopped or started being able to show leaves or rejoins its enclosing one.
        Self::sync_report(ctx);
        let delays = ctx.props.delays_hover();
        if delays == old_props.delays_hover() && ctx.props.config.delay == old_props.config.delay {
            return Update::none();
        }
        let step = if delays {
            let config = ctx.props.config.clone();
            let env = ctx.env().clone();
            if !old_props.delays_hover() {
                // Hover was not timed until now: a pointer already resting on this trigger starts
                // the wait now, unless a nested trigger has it.
                let _ = ctx.state.reveal.reset();
                let takes_over = ctx
                    .state
                    .reported_to
                    .as_ref()
                    .is_some_and(NestedTooltip::any_shown);
                if !ctx.state.trigger_hovered || !ctx.state.nested.is_empty() {
                    RevealStep::Nothing
                } else if takes_over {
                    ctx.state.reveal.hover_revealed(&env)
                } else {
                    ctx.state.reveal.hover(true, &config, &env)
                }
            } else {
                ctx.state.reveal.retime(&config, &env)
            }
        } else {
            // Hover is no longer tracked here; closing this way does not warm other tooltips.
            ctx.state.reveal.reset()
        };
        Self::step(step, ctx)
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            TooltipMsg::Hover(hovered) => {
                ctx.state.trigger_hovered = hovered;
                // Report before forgetting nested tooltips: leaving several levels at once, the
                // enclosing tooltip learns whether one deep inside was showing.
                Self::sync_report(ctx);
                if !hovered {
                    // A nested trigger's leave may arrive after this one's: the pointer is gone.
                    ctx.state.nested.clear();
                }
                if !ctx.props.delays_hover() {
                    // Shown from the live hover state, which re-renders it already.
                    return Update::none();
                }
                if hovered && !ctx.state.nested.is_empty() {
                    // Entered straight onto a nested trigger: this one waits until the pointer
                    // comes back to its own part.
                    return Update::none();
                }
                let env = ctx.env().clone();
                // Taking over from an enclosing tooltip: the user already waited.
                let takes_over = hovered
                    && ctx
                        .state
                        .reported_to
                        .as_ref()
                        .is_some_and(NestedTooltip::any_shown);
                let step = if takes_over {
                    ctx.state.reveal.hover_revealed(&env)
                } else {
                    let config = ctx.props.config.clone();
                    ctx.state.reveal.hover(hovered, &config, &env)
                };
                Self::step(step, ctx)
            }
            TooltipMsg::Reveal(generation) => {
                if ctx.state.reveal.reveal(generation) {
                    Update::render()
                } else {
                    Update::none()
                }
            }
            TooltipMsg::Nested(report) => Self::nested(ctx, report),
            TooltipMsg::CutFade => {
                if ctx.state.visible.get() || ctx.state.fade_cut.get() {
                    return Update::none();
                }
                ctx.state.fade_cut.set(true);
                Update::render()
            }
        }
    }

    fn unmount(&mut self, ctx: &mut Context<Self>) {
        // Removed under the pointer, as when a row's button disappears: the enclosing tooltip
        // must not stay aside for a trigger that is gone.
        if let Some(outer) = ctx.state.reported_to.take() {
            outer.report.emit(NestedHover {
                id: ctx.state.id,
                hovered: false,
                shown: ctx.state.nested.showing(),
                showing: ctx.state.nested.clone(),
            });
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let delayed = ctx.props.delays_hover();
        // The trigger always sits in the same region, whatever the configuration: adding or
        // removing a wrapper would change the child's container path and remount a stateful
        // trigger. The region takes the child's sizing, so a
        // flexible child stays flexible, and its reconciled rectangle, so the popup anchors to the
        // child and hover counts only over it.
        // Tooltips nested in the trigger report the pointer through this context, and this one
        // reports its own trigger to any tooltip it is nested in.
        let shown: Rc<[Arc<AtomicBool>]> = ctx
            .use_context::<NestedTooltip>()
            .map(|outer| outer.shown.iter().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .chain([ctx.state.shown.clone()])
            .collect();
        let own = NestedTooltip {
            owner: ctx.state.id,
            report: ctx.link().callback(TooltipMsg::Nested),
            shown,
        };
        // A tooltip that cannot show stays out of the way: tooltips nested in it report to the
        // nearest enclosing tooltip that can.
        let provided = if ctx.props.can_show() {
            own
        } else {
            ctx.use_context::<NestedTooltip>().unwrap_or(own)
        };
        let nested = crate::widgets::ContextProvider::new(provided).child(ctx.props.child.clone());
        let region = MouseRegion::new()
            .child(nested)
            .fitted_to_child()
            .on_hover_change(ctx.link().callback(TooltipMsg::Hover));
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
        ctx.state
            .shown
            .store(ctx.props.open.unwrap_or(auto_open), Ordering::Relaxed);
        // One tooltip at a time: while the pointer is on a nested trigger, its tooltip speaks.
        let open = ctx
            .props
            .open
            .unwrap_or(auto_open && ctx.state.nested.is_empty());
        let mut config = ctx.props.config.clone();
        let was_visible = ctx.state.visible.replace(open);
        if open {
            ctx.state.fade_cut.set(false);
            if !was_visible {
                // Opening ends every tooltip still fading out, so two never fade over each other.
                let env = ctx.env();
                env.tooltip_opened_at.set(Some((ctx.state.id, env.now())));
                let fading = std::mem::take(&mut *env.tooltip_fading.borrow_mut());
                for cut in fading {
                    cut.emit(());
                }
            }
            if ctx.state.reveal.instant() {
                // A swap from another tooltip appears at once: only a first open fades in.
                if let Some(animation) = &mut config.animation {
                    animation.enter.duration = Duration::ZERO;
                }
            }
        } else if was_visible && config.animation.is_some() {
            let env = ctx.env();
            let swapped = env.tooltip_opened_at.get().is_some_and(|(id, opened)| {
                id != ctx.state.id && env.elapsed(opened) < SWAP_WINDOW
            });
            if swapped {
                // Another tooltip just opened in this one's place: leave without fading.
                ctx.state.fade_cut.set(true);
            } else {
                env.tooltip_fading
                    .borrow_mut()
                    .push(ctx.link().callback(|()| TooltipMsg::CutFade));
            }
        }
        if !open && ctx.state.fade_cut.get() {
            config.animation = None;
        }
        config.popover(trigger, &ctx.props.content, open).into()
    }
}

impl From<Tooltip> for Element {
    fn from(tooltip: Tooltip) -> Self {
        crate::child(TooltipComponent::new, TooltipProps::from(tooltip))
    }
}
