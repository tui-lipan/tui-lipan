use crate::core::context_value::ContextValue;
use crate::core::element::Element;
use crate::widgets::{
    Accordion, AccordionItem, Animated, Badge, Canvas, CanvasItem, Center, ContextProvider,
    DragSource, DraggableTab, DraggableTabBar, DropTarget, EffectScope, Flow, Frame, Graph,
    GraphNode, Grid, HStack, List, ListItem, Modal, MouseRegion, PanView, Popover, ScrollView,
    Splitter, StatusBar, Tab, Tabs, ThemeProvider, Tooltip, TreeNode, VStack, ZStack,
};

/// Connects a builder's child method to [`ui!`](crate::ui) and [`rsx!`](crate::rsx).
///
/// Implement this on a custom builder that accepts child nodes in either macro.
/// `Child` is the accepted builder or element type. Use a generic
/// `const ADDITIONAL: bool` to accept multiple direct children in a block.
/// For a builder that accepts one child, use the default `false`.
///
/// A builder at the root of a macro invocation must also convert into [`Element`].
///
/// ```compile_fail,E0277
/// use tui_lipan::prelude::*;
///
/// ui! { Text::new("Leaf") => { Text::new("Child") } };
/// ```
///
/// ```compile_fail,E0277
/// use tui_lipan::prelude::*;
///
/// type Panel = Frame;
/// rsx! { Panel { Text { content: "First" }, Text { content: "Second" } } };
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot accept this macro child",
    note = "check the parent's child type and use a stack to group siblings in a single-child widget"
)]
pub trait UiChildren<Child, const ADDITIONAL: bool = false> {
    /// Adds a child using the same rules as the builder API.
    fn ui_child(self, child: Child) -> Self;
}

macro_rules! convertible_children {
    (many: $($parent:ty => $method:ident($child:ty)),+ $(,)?) => {
        $(
            impl<C: Into<$child>, const ADDITIONAL: bool> UiChildren<C, ADDITIONAL> for $parent {
                fn ui_child(self, child: C) -> Self {
                    self.$method(child)
                }
            }
        )+
    };
    (one: $($parent:ty => $method:ident),+ $(,)?) => {
        $(
            impl<C: Into<Element>> UiChildren<C> for $parent {
                fn ui_child(self, child: C) -> Self {
                    self.$method(child)
                }
            }
        )+
    };
}

convertible_children! {
    many:
    Flow => child(Element),
    Grid => child(Element),
    HStack => child(Element),
    ScrollView => child(Element),
    Splitter => child(Element),
    StatusBar => child(Element),
    VStack => child(Element),
    ZStack => child(Element),
    List => item(ListItem),
    Tabs => tab(Tab),
    DraggableTabBar => tab(DraggableTab),
}

convertible_children! {
    one:
    AccordionItem => content,
    Animated => child,
    Badge => child,
    Center => child,
    DragSource => child,
    DropTarget => child,
    EffectScope => child,
    Frame => child,
    Modal => child,
    MouseRegion => child,
    PanView => child,
    Popover => content,
    ThemeProvider => child,
    Tooltip => child,
}

impl<T: ContextValue, C: Into<Element>> UiChildren<C> for ContextProvider<T> {
    fn ui_child(self, child: C) -> Self {
        self.child(child)
    }
}

impl<const ADDITIONAL: bool> UiChildren<AccordionItem, ADDITIONAL> for Accordion {
    fn ui_child(self, child: AccordionItem) -> Self {
        self.item(child)
    }
}

impl<const ADDITIONAL: bool> UiChildren<CanvasItem, ADDITIONAL> for Canvas {
    fn ui_child(self, child: CanvasItem) -> Self {
        self.child_at(child.rect, child.element)
    }
}

impl UiChildren<GraphNode> for Graph {
    fn ui_child(self, child: GraphNode) -> Self {
        self.root(child)
    }
}

impl<const ADDITIONAL: bool> UiChildren<GraphNode, ADDITIONAL> for GraphNode {
    fn ui_child(self, child: GraphNode) -> Self {
        self.child(child)
    }
}

impl<const ADDITIONAL: bool> UiChildren<TreeNode, ADDITIONAL> for TreeNode {
    fn ui_child(self, child: TreeNode) -> Self {
        self.child(child)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::ElementKind;
    use crate::widgets::{FlowDirection, Flowchart, Text, Toast};
    use crate::{rsx, ui};

    #[test]
    fn ui_dispatches_typed_children_through_builder_expressions() {
        type TabBar = Tabs;
        let make_tabs = || TabBar::new();
        let tabs = ui! {
            make_tabs() => {
                "First",
                for label in ["Second", "Third"] {
                    Tab::new(label),
                },
            }
        };

        let ElementKind::Tabs(tabs) = tabs.kind else {
            panic!("expected a tab bar");
        };
        assert_eq!(tabs.tabs.len(), 3);
    }

    #[test]
    fn rsx_dispatches_list_items_through_qualified_aliases() {
        mod aliased {
            pub(super) type Rows = crate::widgets::List;
        }

        let rows = rsx! {
            aliased :: Rows {
                ListItem { label: "First" },
                "Second",
            }
        };
        let ElementKind::List(list) = rows.kind else {
            panic!("expected a list");
        };
        assert_eq!(list.items.len(), 2);
    }

    #[test]
    fn rsx_builds_context_provider_with_new_constructor_keys() {
        let element = rsx! {
            ContextProvider {
                value: 42u32,
                VStack {
                    Tooltip {
                        content: "Help",
                        Text { content: "Trigger" },
                    },
                    Toast { content: "Saved" },
                    Flowchart { direction: FlowDirection::LeftRight },
                },
            }
        };

        let ElementKind::ContextProvider(provider) = element.kind else {
            panic!("expected a context provider");
        };
        assert_eq!(provider.value.downcast_ref::<u32>(), Some(&42));
        assert_eq!(provider.child.kind.children().len(), 3);
    }

    #[test]
    fn custom_container_accepts_siblings_from_both_macros() {
        struct Rows(List);

        impl Rows {
            fn new() -> Self {
                Self(List::new())
            }
        }

        impl<const ADDITIONAL: bool> UiChildren<ListItem, ADDITIONAL> for Rows {
            fn ui_child(self, child: ListItem) -> Self {
                Self(self.0.item(child))
            }
        }

        impl From<Rows> for Element {
            fn from(rows: Rows) -> Self {
                rows.0.into()
            }
        }

        let ui_rows = ui! {
            Rows::new() => {
                ListItem::new("First"),
                ListItem::new("Second"),
            }
        };
        let rsx_rows = rsx! {
            Rows {
                ListItem { label: "First" },
                ListItem { label: "Second" },
            }
        };

        for rows in [ui_rows, rsx_rows] {
            let ElementKind::List(list) = rows.kind else {
                panic!("expected a list");
            };
            assert_eq!(list.items.len(), 2);
        }
    }
}
