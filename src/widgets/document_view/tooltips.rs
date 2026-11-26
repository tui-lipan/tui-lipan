//! Tooltips over parts of a [`DocumentView`]: see [`DocumentView::tooltips`].

use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::widgets::TooltipConfig;
use crate::widgets::tooltip::{HoverReveal, RevealStep, TooltipContent};

use super::DocumentView;
use super::node::DocumentHoverHint;

/// `view` inside a host that shows `config` tooltips for the hints under the pointer.
pub(crate) fn document_tooltips(view: DocumentView, config: TooltipConfig) -> Element {
    crate::child(
        DocumentTooltips::new,
        DocumentTooltipsProps { view, config },
    )
}

#[derive(Clone)]
struct DocumentTooltipsProps {
    view: DocumentView,
    config: TooltipConfig,
}

impl PartialEq for DocumentTooltipsProps {
    /// Always changed, like [`Tooltip`](crate::widgets::Tooltip): the view is rebuilt on every
    /// parent render. Re-rendering keeps the hint and the hover timer.
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

#[derive(Clone, Debug, Default)]
struct DocumentTooltipsState {
    /// The hint under the pointer.
    hint: Option<DocumentHoverHint>,
    /// The hint last shown, kept while the tooltip closes so its text and place stay put.
    shown: Option<DocumentHoverHint>,
    reveal: HoverReveal,
}

enum DocumentTooltipsMsg {
    Hint(Option<DocumentHoverHint>),
    Reveal(u64),
}

struct DocumentTooltips;

impl DocumentTooltips {
    fn new() -> Self {
        Self
    }

    fn arm(step: RevealStep, ctx: &mut Context<Self>) -> Option<crate::core::component::Command> {
        let RevealStep::Arm { wait, generation } = step else {
            return None;
        };
        Some(ctx.link().command_after(wait, move |link| {
            link.send(DocumentTooltipsMsg::Reveal(generation));
        }))
    }

    /// The pointer moved onto `next`: the old hint's tooltip closes and the new one's opens.
    fn hint(next: Option<DocumentHoverHint>, ctx: &mut Context<Self>) -> Update {
        let same_part = match (&ctx.state.hint, &next) {
            (Some(current), Some(next)) => current.part == next.part,
            (None, None) => true,
            _ => false,
        };
        if same_part {
            // The same part moved, as when the document scrolls under a resting pointer.
            let moved = ctx.state.hint != next;
            ctx.state.hint = next;
            if moved && ctx.state.reveal.revealed() {
                ctx.state.shown.clone_from(&ctx.state.hint);
                return Update::render();
            }
            return Update::none();
        }
        let config = ctx.props.config.clone();
        let env = ctx.env().clone();
        let mut render = false;
        if ctx.state.hint.is_some() {
            render |= matches!(
                ctx.state.reveal.hover(false, &config, &env),
                RevealStep::Render
            );
        }
        ctx.state.hint = next;
        let mut command = None;
        if ctx.state.hint.is_some() {
            let step = ctx.state.reveal.hover(true, &config, &env);
            render |= matches!(step, RevealStep::Render);
            command = Self::arm(step, ctx);
        }
        if ctx.state.reveal.revealed() {
            ctx.state.shown.clone_from(&ctx.state.hint);
        }
        match (render, command) {
            (true, command) => Update::render_with_command(command),
            (false, Some(command)) => Update::command_only(command),
            (false, None) => Update::none(),
        }
    }
}

impl Component for DocumentTooltips {
    type Message = DocumentTooltipsMsg;
    type Properties = DocumentTooltipsProps;
    type State = DocumentTooltipsState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        DocumentTooltipsState::default()
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        if ctx.props.config.delay == old_props.config.delay {
            return Update::none();
        }
        let config = ctx.props.config.clone();
        let env = ctx.env().clone();
        let step = ctx.state.reveal.retime(&config, &env);
        let render = matches!(step, RevealStep::Render);
        if render {
            ctx.state.shown.clone_from(&ctx.state.hint);
        }
        match Self::arm(step, ctx) {
            Some(command) => Update::command_only(command),
            None if render => Update::render(),
            None => Update::none(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            DocumentTooltipsMsg::Hint(hint) => Self::hint(hint, ctx),
            DocumentTooltipsMsg::Reveal(generation) => {
                if !ctx.state.reveal.reveal(generation) {
                    return Update::none();
                }
                ctx.state.shown.clone_from(&ctx.state.hint);
                Update::render()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut view = ctx.props.view.clone();
        view.on_hover_hint = Some(ctx.link().callback(DocumentTooltipsMsg::Hint));
        let open = ctx.state.reveal.revealed() && ctx.state.hint.is_some();
        let shown = ctx.state.shown.as_ref();
        let content = TooltipContent::Text(shown.map(|hint| hint.text.clone()).unwrap_or_default());
        ctx.props
            .config
            .popover(view.into(), &content, open)
            .anchor_area(shown.map(|hint| hint.area))
            .into()
    }
}
