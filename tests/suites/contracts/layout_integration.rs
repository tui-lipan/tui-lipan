//! Integration tests for layout behavior through the public API.
//!
//! These tests verify that complex widget layouts work correctly from a user's perspective.

use tui_lipan::prelude::*;

/// Simple test component that renders a Center with VStack.
struct CenterTestComponent {
    content_width: usize,
}

#[derive(Clone, Debug)]
enum TestMsg {}

impl Component for CenterTestComponent {
    type Message = TestMsg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let text_content: String = "x".repeat(self.content_width);

        rsx! {
            Center {
                VStack {
                    width: Length::Auto,
                    gap: 1,
                    HStack {
                        Text { content: text_content },
                    },
                },
            }
        }
    }
}

/// Test that Center with VStack can be created and rendered without panic.
/// This is a regression test for the width collapse bug.
#[test]
fn center_with_vstack_does_not_panic() {
    let component = CenterTestComponent { content_width: 20 };

    // This should not panic - before the fix, the layout system would
    // produce an invalid state when measuring VStack width
    let _backend = tui_lipan::TestBackend::new(component);
}

/// Test various content widths to ensure layout is stable.
#[test]
fn center_layout_with_various_content_sizes() {
    for width in [1, 5, 10, 20, 50, 100] {
        let component = CenterTestComponent {
            content_width: width,
        };
        let _backend = tui_lipan::TestBackend::new(component);
        // If we get here without panic, the layout worked
    }
}

/// Test component with deeply nested structure similar to opencode_home.rs.
struct ComplexLayoutComponent;

impl Component for ComplexLayoutComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        rsx! {
            Center {
                VStack {
                    width: Length::Auto,
                    gap: 1,
                    alignment: Align::Center,

                    // Logo-like HStack
                    HStack {
                        height: Length::Auto,
                        Text { content: "Big" },
                        Text { content: "Text" }
                    },

                    Spacer { height: Length::Px(2) },

                    // Input Area Frame
                    Frame {
                        border: false,
                        style: Style::new(),
                        padding: (1, 2),

                        VStack {
                            height: Length::Auto,
                            gap: 0,

                            Text { content: "Input prompt here" },

                            HStack {
                                height: Length::Px(1),
                                gap: 1,
                                Text { content: "Status line" }
                            }
                        }
                    },

                    // Shortcuts row
                    HStack {
                        height: Length::Auto,
                        alignment: Align::End,
                        gap: 2,

                        HStack {
                            gap: 1,
                            Text { content: "ctrl+t" },
                            Text { content: "variants" }
                        },
                        HStack {
                            gap: 1,
                            Text { content: "tab" },
                            Text { content: "agents" }
                        }
                    }
                }
            }
        }
    }
}

/// Test that complex nested layout similar to opencode_home.rs works.
#[test]
fn complex_nested_layout_does_not_panic() {
    let component = ComplexLayoutComponent;
    let _backend = tui_lipan::TestBackend::new(component);
}

/// Test component that creates multiple nested VStack/HStack combinations.
struct NestedStacksComponent;

impl Component for NestedStacksComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        rsx! {
            VStack {
                width: Length::Auto,
                // Nested structure: VStack { HStack { Text } }
                    HStack {
                        Text { content: "A" },
                        Text { content: "B" }
                    },
                HStack {
                    Text { content: "Third longer text" }
                },
                VStack {
                    width: Length::Auto,
                    HStack {
                        Text { content: "Deeply" },
                        Text { content: "Nested" }
                    }
                }
            }
        }
    }
}

/// Test that multiple levels of VStack/HStack nesting work.
#[test]
fn deeply_nested_stacks_work() {
    let component = NestedStacksComponent;
    let _backend = tui_lipan::TestBackend::new(component);
}

/// Test Frame with various content types.
#[test]
fn frame_with_various_content() {
    struct FrameTestComponent;

    impl Component for FrameTestComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            rsx! {
                VStack {
                    Frame {
                        border: true,
                        Text { content: "Short" },
                    },
                    Frame {
                        border: true,
                        Text { content: "This is a much longer text content" },
                    },
                    Frame {
                        border: false,
                        HStack {
                            Text { content: "A" },
                            Text { content: "B" },
                        },
                    },
                }
            }
        }
    }

    let _backend = tui_lipan::TestBackend::new(FrameTestComponent);
}

/// VStack with two Frame children using Flex(1) and Flex(2) weights.
/// Verifies that flex distribution renders without panic on a tall viewport.
#[test]
fn flex_distribution_in_vstack() {
    struct FlexDistComponent;

    impl Component for FlexDistComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            rsx! {
                VStack {
                    height: Length::Flex(1),
                    Frame {
                        border: true,
                        height: Length::Flex(1),
                        Text { content: "Top panel" },
                    },
                    Frame {
                        border: true,
                        height: Length::Flex(2),
                        Text { content: "Bottom panel" },
                    },
                }
            }
        }
    }

    let mut backend = tui_lipan::TestBackend::new(FlexDistComponent);
    // Use a 80x30 viewport so flex children have room to distribute.
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 30,
    });
    backend.render();
    // If we get here without panic, flex distribution worked.
}

/// Frame with explicit Px width/height. Verifies the frame renders at fixed
/// dimensions without panic (border consumes 1 cell per edge).
#[test]
fn frame_px_outer_size() {
    struct FramePxComponent;

    impl Component for FramePxComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            rsx! {
                Frame {
                    width: Length::Px(20),
                    height: Length::Px(10),
                    border: true,
                    Text { content: "Fixed size" },
                }
            }
        }
    }

    let _backend = tui_lipan::TestBackend::new(FramePxComponent);
}

/// HStack with Align::Stretch should stretch the cross-axis of children
/// to fill the full height. Verifies no panic.
#[test]
fn align_stretch_fills_cross_axis() {
    struct StretchComponent;

    impl Component for StretchComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            rsx! {
                HStack {
                    alignment: Align::Stretch,
                    height: Length::Px(10),
                    Button { label: "Stretched" },
                }
            }
        }
    }

    let _backend = tui_lipan::TestBackend::new(StretchComponent);
}

/// ScrollView with tall content (100 Text items) inside a small viewport.
/// Should not panic even though content exceeds the viewport.
#[test]
fn scroll_view_tall_content_no_panic() {
    struct ScrollTallComponent;

    impl Component for ScrollTallComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let children: Vec<Element> = (0..100)
                .map(|i| Text::new(format!("Line {i}")).into())
                .collect();

            ScrollView::new().children(children).into()
        }
    }

    let _backend = tui_lipan::TestBackend::new(ScrollTallComponent);
}

/// VStack with zero children should render without panic.
#[test]
fn empty_vstack_no_panic() {
    struct EmptyVStackComponent;

    impl Component for EmptyVStackComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new().into()
        }
    }

    let _backend = tui_lipan::TestBackend::new(EmptyVStackComponent);
}

/// Deeply nested Frame > Frame > Frame > Text should render without panic.
#[test]
fn deeply_nested_frames() {
    struct DeepFrameComponent;

    impl Component for DeepFrameComponent {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            rsx! {
                Frame {
                    border: true,
                    Frame {
                        border: true,
                        Frame {
                            border: true,
                            Text { content: "hello" },
                        },
                    },
                }
            }
        }
    }

    let _backend = tui_lipan::TestBackend::new(DeepFrameComponent);
}
