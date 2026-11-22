use std::collections::BTreeSet;

use tui_lipan::prelude::*;
use tui_lipan::{Heatmap, HexArea, UiChildren};

macro_rules! constructor_examples {
    (
        $(
            $(#[$feature:meta])?
            $widget:ident { $key:ident: $value:expr $(, $($rest:tt)*)? }
            $(in $parent:ident)?;
        )+
    ) => {
        const TESTED_CONSTRUCTORS: &[(&str, &str)] = &[
            $((stringify!($widget), stringify!($key))),+
        ];

        #[test]
        fn documented_constructor_nodes_compile_and_build() {
            $(
                $(#[$feature])?
                let _: Element = constructor_examples!(
                    @build [$widget { $key: $value $(, $($rest)*)? }] $(in $parent)?
                );
            )+
        }
    };
    (@build [$($node:tt)*] in $parent:ident) => {
        rsx! { $parent { $($node)* } }
    };
    (@build [$($node:tt)*]) => {
        rsx! { $($node)* }
    };
}

// The same node syntax supplies the compile test and the coverage list.
constructor_examples! {
    AccordionItem { title: "Section", Text { content: "Body" } } in Accordion;
    AsciiCanvas { lines: ["..."] };
    Badge { content: "New" };
    Button { label: "OK" };
    Checkbox { checked: true };
    ContextMenu { trigger: Button::new("Menu") };
    ContextProvider { value: 42u32 };
    Divider { orientation: Orientation::Horizontal };
    DocumentView { value: "Document" };
    DraggableTab { label: "file.rs" } in DraggableTabBar;
    FileTree { root: "/home/user" };
    Flowchart { direction: FlowDirection::LeftRight };
    Heatmap { data: [[0.0, 1.0]] };
    HexArea { bytes: vec![0u8, 1] };
    #[cfg(feature = "image")]
    Image { src: "logo.png" };
    Input { value: "Edited value" };
    ListItem { content: "Item 1" } in List;
    PaginationBar { state: PaginationState::new(12, 5) };
    ProgressBar { progress: 0.75 };
    Radio { options: ["A", "B"] };
    Slider { value: 0.5 };
    Sparkline { data: [1, 2, 3] };
    Splitter { orientation: Orientation::Vertical };
    Tab { label: "Tab 1" } in Tabs;
    Text { content: "Hello" };
    TextArea { value: "Edited value" };
    ThemeProvider { theme: Theme::default() };
    Toast { content: "Saved!" };
    Tooltip { content: "Help text" };
    Tree { root: TreeNode::new("Root") };
}

#[test]
fn constructor_compile_tests_cover_the_documented_keys() {
    let docs = include_str!("../../../docs/macros.md");
    let (_, constructors) = docs.split_once("### Constructor Keys\n").unwrap();
    let table = constructors.split("\n### ").next().unwrap();
    let documented: BTreeSet<_> = table
        .lines()
        .filter_map(|line| {
            let mut cells = line.strip_prefix("| `")?.split("` | `");
            Some((cells.next()?, cells.next()?))
        })
        .collect();

    assert_eq!(documented, TESTED_CONSTRUCTORS.iter().copied().collect());
    assert_eq!(documented.len(), TESTED_CONSTRUCTORS.len());
}

#[test]
fn grid_uses_its_uniform_columns_builder_in_rsx() {
    let _ = rsx! {
        Grid {
            uniform_columns: 3,
            Text { content: "First" },
        }
    };
}

#[test]
fn accordion_item_accepts_ui_children() {
    let _ = ui! {
        Accordion::new() => {
            AccordionItem::new("Section", Spacer::new()) => {
                Text::new("Body"),
            },
        }
    };
}

#[test]
fn a_custom_builder_can_implement_the_root_trait() {
    struct Panel(VStack);

    impl UiChildren<Text> for Panel {
        fn ui_child(self, child: Text) -> Self {
            Self(self.0.child(child))
        }
    }

    impl From<Panel> for Element {
        fn from(panel: Panel) -> Self {
            panel.0.into()
        }
    }

    let _ = ui! {
        Panel(VStack::new()) => {
            Text::new("Body"),
        }
    };
}
