//! Modal widget.

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::overlay::{
    DismissPolicy, OverlayLayer, OverlayPlacement, OverlayScope, PointerCapture, Portal,
};
use crate::style::{Align, BorderStyle, Color, Length, Padding, RichText, Size, Style};
use crate::widgets::{Center, Frame, MouseRegion, Spacer, ZStack};

/// A modal dialog with optional title and child content.
#[derive(Clone)]
pub struct Modal {
    title: Option<RichText>,
    child: Element,
    on_close: Option<Callback<()>>,
    scope: OverlayScope,
    width: Length,
    height: Length,
    backdrop_style: Style,
    frame_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    title_style: Style,
    title_alignment: Align,
}

impl Modal {
    /// Create a new modal.
    pub fn new() -> Self {
        Self {
            title: None,
            child: Spacer::new().into(),
            on_close: None,
            scope: OverlayScope::RootPortal,
            width: Length::Px(60),
            height: Length::Auto,
            backdrop_style: Style::default(),
            frame_style: Style::default(),
            border: true,
            border_style: BorderStyle::Plain,
            padding: 1.into(),
            title_style: Style::default(),
            title_alignment: Align::Start,
        }
    }

    /// Set title.
    pub fn title(mut self, title: impl Into<RichText>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set modal child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = child.into();
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set on-close callback (fired when background is clicked).
    pub fn on_close(mut self, cb: Callback<()>) -> Self {
        self.on_close = Some(cb);
        self
    }

    /// Set overlay scope (portal vs local rendering).
    pub fn scope(mut self, scope: OverlayScope) -> Self {
        self.scope = scope;
        self
    }

    /// Set backdrop style.
    pub fn backdrop_style(mut self, style: Style) -> Self {
        self.backdrop_style = style;
        self
    }

    /// Set modal frame style.
    pub fn frame_style(mut self, style: Style) -> Self {
        self.frame_style = style;
        self
    }

    /// Enable or disable border decoration.
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

    /// Set title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set title alignment.
    pub fn title_alignment(mut self, align: Align) -> Self {
        self.title_alignment = align;
        self
    }
}

impl Default for Modal {
    fn default() -> Self {
        Self::new()
    }
}

impl From<Modal> for Element {
    fn from(modal: Modal) -> Self {
        let frame_style = if modal.frame_style.bg.is_none() {
            modal.frame_style.bg(Color::Backdrop)
        } else {
            modal.frame_style
        };

        let mut base_frame = Frame::new()
            .title_style(modal.title_style)
            .title_alignment(modal.title_alignment)
            .border(modal.border)
            .border_style(modal.border_style)
            .padding(modal.padding)
            .child(modal.child)
            .style(frame_style);
        if let Some(title) = modal.title {
            base_frame = base_frame.title(title);
        }

        match modal.scope {
            OverlayScope::Local => {
                let mut backdrop = MouseRegion::new().capture_click(true);

                if !modal.backdrop_style.is_empty() {
                    backdrop = backdrop.child(
                        Center::new()
                            .width(Size::Percent(100))
                            .height(Size::Percent(100))
                            .style(modal.backdrop_style),
                    );
                }

                if let Some(on_close) = modal.on_close {
                    let cb = on_close.clone();
                    backdrop = backdrop.on_click(Callback::new(move |_: MouseEvent| cb.emit(())));
                } else {
                    backdrop = backdrop.enabled(false);
                }

                let local_width = match modal.width {
                    Length::Auto => Size::Auto,
                    Length::Px(px) => Size::Fixed(px),
                    Length::Percent(percent) => Size::Percent(percent),
                    Length::Flex(_) => Size::Percent(100),
                };
                let local_height = match modal.height {
                    Length::Auto => Size::Auto,
                    Length::Px(px) => Size::Fixed(px),
                    Length::Percent(percent) => Size::Percent(percent),
                    Length::Flex(_) => Size::Percent(100),
                };

                let local_frame_width = if matches!(modal.width, Length::Auto) {
                    Length::Auto
                } else {
                    Length::Flex(1)
                };
                let local_frame_height = if matches!(modal.height, Length::Auto) {
                    Length::Auto
                } else {
                    Length::Flex(1)
                };

                let frame = base_frame
                    .clone()
                    .width(local_frame_width)
                    .height(local_frame_height);

                ZStack::new()
                    .child(backdrop)
                    .child(
                        Center::new()
                            .width(local_width)
                            .height(local_height)
                            .child(frame),
                    )
                    .into()
            }
            OverlayScope::RootPortal => {
                let frame = base_frame.width(modal.width).height(modal.height);
                let dismiss_policy = if modal.on_close.is_some() {
                    DismissPolicy::ClickOutsideOrEscape
                } else {
                    DismissPolicy::None
                };
                let portal = Portal {
                    layer: OverlayLayer::Modal,
                    content: Box::new(frame.into()),
                    placement: OverlayPlacement::Center,
                    dismiss_policy,
                    on_close: modal.on_close,
                    backdrop: Some(modal.backdrop_style),
                    captures_focus: true,
                    captures_pointer: PointerCapture::BackdropFullScreen,
                };
                Element::new(ElementKind::Portal(portal))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    #[test]
    fn local_scope_percent_size_is_resolved_by_center_layer() {
        let element: Element = Modal::new()
            .title("Local Percent")
            .scope(OverlayScope::Local)
            .width(Length::Percent(70))
            .height(Length::Percent(50))
            .child(Spacer::new())
            .into();

        let ElementKind::ZStack(zstack) = element.kind else {
            panic!("modal local scope must render as zstack");
        };
        assert_eq!(zstack.children.len(), 2);

        let ElementKind::Center(center) = &zstack.children[1].kind else {
            panic!("modal content layer must be centered");
        };
        assert_eq!(center.width, Size::Percent(70));
        assert_eq!(center.height, Size::Percent(50));

        let frame = center
            .child
            .as_deref()
            .expect("center must contain modal frame");
        let ElementKind::Frame(frame) = &frame.kind else {
            panic!("center child must be frame");
        };
        assert_eq!(frame.props.width, Length::Flex(1));
        assert_eq!(frame.props.height, Length::Flex(1));
        assert_eq!(frame.props.style.bg, Some(Color::Backdrop.into()));
    }

    #[test]
    fn explicit_transparent_frame_style_bg_is_preserved() {
        let element: Element = Modal::new()
            .scope(OverlayScope::Local)
            .frame_style(Style::new().bg(Color::Transparent))
            .child(Spacer::new())
            .into();

        let ElementKind::ZStack(zstack) = element.kind else {
            panic!("modal local scope must render as zstack");
        };

        let ElementKind::Center(center) = &zstack.children[1].kind else {
            panic!("modal content layer must be centered");
        };
        let frame = center
            .child
            .as_deref()
            .expect("center must contain modal frame");
        let ElementKind::Frame(frame) = &frame.kind else {
            panic!("center child must be frame");
        };
        assert_eq!(frame.props.style.bg, Some(Color::Transparent.into()));
    }

    #[test]
    fn local_scope_backdrop_uses_mouse_region_layer() {
        let element: Element = Modal::new()
            .title("Local Backdrop")
            .scope(OverlayScope::Local)
            .backdrop_style(Style::new().dim_by(0.5))
            .child(Spacer::new())
            .into();

        let ElementKind::ZStack(zstack) = element.kind else {
            panic!("modal local scope must render as zstack");
        };
        assert_eq!(zstack.children.len(), 2);

        let ElementKind::MouseRegion(backdrop) = &zstack.children[0].kind else {
            panic!("first local layer must be mouse region backdrop");
        };
        let backdrop_child = backdrop
            .child
            .as_deref()
            .expect("non-empty backdrop style must attach backdrop child");
        let ElementKind::Center(backdrop_layer) = &backdrop_child.kind else {
            panic!("backdrop child must be center layer");
        };
        assert_eq!(backdrop_layer.style.dim_amount, Some(0.5));
    }
}
