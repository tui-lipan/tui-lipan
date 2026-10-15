use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct TreeRows(Vec<TreeNode>, u16);

impl Component for TreeRows {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        Tree::new(
            TreeNode::new("root")
                .expanded(true)
                .children(self.0.clone()),
        )
        .show_icons(false)
        .indent_style(IndentStyle::Short)
        .selection_symbol(Some(""))
        .unselected_symbol(Some(""))
        .width(Length::Px(self.1))
        .height(Length::Px(12))
        .into()
    }
}

fn rows(nodes: Vec<TreeNode>, width: u16) -> Vec<String> {
    let mut backend = TestBackend::new(TreeRows(nodes, width));
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: width,
        h: 12,
    });
    backend.render();
    let frame = backend.capture_frame();
    (0..12)
        .map(|y| {
            (0..width)
                .map(|x| frame.cell(x, y).symbol.clone())
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

#[test]
fn label_spinners_follow_guides_without_moving_child_connectors() {
    for position in [ListSymbolPosition::Left, ListSymbolPosition::Right] {
        let rendered = rows(
            vec![
                TreeNode::new(
                    ListItem::new("loading")
                        .label_spinner(Spinner::new().frame(0))
                        .label_spinner_position(position),
                )
                .expanded(true)
                .child(TreeNode::new("child")),
                TreeNode::new("ready"),
            ],
            30,
        );
        let expected = match position {
            ListSymbolPosition::Left => "├ ⠋ loading",
            ListSymbolPosition::Right => "├ loading ⠋",
        };
        assert_eq!(rendered[1], expected);
        assert_eq!(rendered[2], "│ └ child");
        assert_eq!(rendered[3], "└ ready");
    }
}

#[test]
fn numbered_extra_lines_align_with_labels_at_each_depth() {
    let rendered = rows(
        vec![
            TreeNode::new(ListItem::new("one").numbered(1).line("detail"))
                .expanded(true)
                .child(TreeNode::new(
                    ListItem::new("two").numbered(12).line("nested"),
                )),
            TreeNode::new(ListItem::new("plain").prefix("p ").line("detail")),
        ],
        30,
    );
    assert_eq!(rendered[1], " 1. ├ one");
    assert_eq!(rendered[2], "      detail");
    assert_eq!(rendered[3], "12. │ └ two");
    assert_eq!(rendered[4], "        nested");
    assert_eq!(rendered[5], "p └ plain");
    assert_eq!(rendered[6], "    detail");
}

#[test]
fn numbered_wrapped_labels_keep_the_tree_indent() {
    let rendered = rows(
        vec![TreeNode::new(
            ListItem::new("alpha beta gamma")
                .numbered(1)
                .primary_wrap_label(true),
        )],
        16,
    );
    assert_eq!(rendered[1], "1. └ alpha beta");
    assert_eq!(rendered[2], "     gamma");
}

#[test]
fn wrapped_labels_keep_spinners_after_the_guides() {
    let rendered = rows(
        vec![TreeNode::new(
            ListItem::new("alpha beta gamma")
                .label_spinner(Spinner::new().frame(0))
                .primary_wrap_label(true),
        )],
        16,
    );
    assert_eq!(rendered[1], "└ ⠋ alpha beta");
    assert_eq!(rendered[2], "  gamma");
}
