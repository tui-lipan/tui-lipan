//! A generic Tree with several roots, rich rows, and independent row actions.
use tui_lipan::prelude::*;

struct Projects;
#[derive(Default)]
struct State {
    expanded: [bool; 2],
    activated: usize,
    closed: usize,
}
enum Msg {
    Toggle(usize, bool),
    Activate,
    Close,
}
impl Component for Projects {
    type Message = Msg;
    type Properties = ();
    type State = State;
    fn create_state(&self, _: &()) -> State {
        State {
            expanded: [true, false],
            ..State::default()
        }
    }
    fn update(&mut self, message: Msg, ctx: &mut Context<Self>) -> Update {
        match message {
            Msg::Toggle(index, expanded) => ctx.state.expanded[index] = expanded,
            Msg::Activate => ctx.state.activated += 1,
            Msg::Close => ctx.state.closed += 1,
        }
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = ctx.theme();
        let roots = ["rozi", "tui-lipan"]
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let content = HStack::new()
                    .gap(1)
                    .height(Length::Px(1))
                    .child(Text::new(name).style(theme.accent))
                    .child(
                        Button::new("×")
                            .tab_stop(false)
                            .on_click(ctx.link().callback(|_| Msg::Close)),
                    );
                TreeNode::new(name)
                    .key(name)
                    .content(content)
                    .expanded(ctx.state.expanded[index])
                    .child(
                        TreeNode::new("development")
                            .key(format!("{name}-development"))
                            .content(VStack::new().gap(0).child(Text::new("development")).child(
                                Text::new("2 panes · waiting for review").style(theme.muted),
                            ))
                            .content_height(2)
                            .expansion_mode(TreeExpansionMode::AlwaysOpen)
                            .child(TreeNode::new("shell")),
                    )
            });
        VStack::new()
            .gap(1)
            .padding(1)
            .child(Text::new("Projects · Tree with composed rows").style(theme.accent))
            .child(
                Tree::forest(roots)
                    .controlled_expansion(true)
                    .toggle_on_activate(false)
                    .indent_style(IndentStyle::BranchRounded)
                    .content_gap(1)
                    .scrollbar(true)
                    .scroll_keys(ScrollKeymap::DEFAULT | ScrollKeymap::VIM_JUMP)
                    .focus_key("projects")
                    .on_toggle(ctx.link().callback(|event: TreeToggleEvent| {
                        Msg::Toggle(event.path.segments()[0], event.expanded)
                    }))
                    .on_activate(ctx.link().callback(|_| Msg::Activate)),
            )
            .child(Text::new(format!(
                "Activations: {} · × actions: {}",
                ctx.state.activated, ctx.state.closed
            )))
            .child(
                Text::new("Development groups stay open · Arrow: toggle project · Row: activate · ×: close")
                    .style(theme.muted),
            )
            .into()
    }
}
fn main() -> tui_lipan::Result<()> {
    App::new().mount(Projects).run()
}
