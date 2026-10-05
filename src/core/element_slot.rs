//! Renderable child properties with explicit value equality.

use std::any::Any;
use std::rc::Rc;

use crate::core::element::Element;

/// A child view whose equality comes from its input props and render function.
///
/// Use this in component properties when a child view should survive unrelated
/// parent updates. Freshly allocated slots compare equal when their props and
/// renderer compare equal, without comparing element allocation or layout hashes.
///
/// The renderer is a function pointer so it cannot hide captured dependencies.
/// Include every value that affects content, style, layout, or event routing in
/// the props. Renderers must not read mutable global state.
///
/// ```
/// use tui_lipan::prelude::{Element, ElementSlot, Text};
///
/// fn heading(label: &String) -> Element {
///     Text::new(label.clone()).into()
/// }
///
/// let first = ElementSlot::new(String::from("Results"), heading);
/// let second = ElementSlot::new(String::from("Results"), heading);
/// assert!(first == second);
/// ```
#[derive(Clone)]
pub struct ElementSlot(Rc<dyn SlotView>);

impl ElementSlot {
    /// Create a slot from comparable props and a non-capturing renderer.
    pub fn new<P: PartialEq + 'static>(props: P, render: fn(&P) -> Element) -> Self {
        Self(Rc::new(TypedSlot { props, render }))
    }

    /// Render the child with the slot's current props.
    pub fn render(&self) -> Element {
        self.0.render()
    }
}

impl PartialEq for ElementSlot {
    fn eq(&self, other: &Self) -> bool {
        self.0.equals(other.0.as_ref())
    }
}

trait SlotView: Any {
    fn as_any(&self) -> &dyn Any;
    fn equals(&self, other: &dyn SlotView) -> bool;
    fn render(&self) -> Element;
}

struct TypedSlot<P> {
    props: P,
    render: fn(&P) -> Element,
}

impl<P: PartialEq + 'static> SlotView for TypedSlot<P> {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn equals(&self, other: &dyn SlotView) -> bool {
        other.as_any().downcast_ref::<Self>().is_some_and(|other| {
            self.props == other.props && std::ptr::fn_addr_eq(self.render, other.render)
        })
    }

    fn render(&self) -> Element {
        (self.render)(&self.props)
    }
}

#[cfg(test)]
mod tests {
    use super::ElementSlot;
    use crate::widgets::Text;
    use crate::{Element, Style};

    fn heading(props: &(String, Style)) -> Element {
        Text::new(props.0.clone()).style(props.1).into()
    }

    fn alternate_heading(props: &(String, Style)) -> Element {
        Text::new(format!("Alternate {}", props.0))
            .style(props.1)
            .into()
    }

    #[test]
    fn fresh_slots_compare_all_props_and_the_renderer() {
        let props = (String::from("Results"), Style::new());
        let first = ElementSlot::new(props.clone(), heading);
        assert!(first == first.clone());
        assert!(first == ElementSlot::new(props.clone(), heading));
        assert!(first != ElementSlot::new((String::from("Other"), props.1), heading));
        assert!(first != ElementSlot::new((props.0.clone(), props.1.bold()), heading));
        assert!(first != ElementSlot::new(props, alternate_heading));
        assert!(first != ElementSlot::new((), |_| Text::new("Results").into()));
    }

    fn action(callback: &crate::Callback<crate::MouseEvent>) -> Element {
        crate::widgets::Button::new("Action")
            .on_click(callback.clone())
            .into()
    }

    #[test]
    fn callback_props_preserve_behavior_identity() {
        let callback = crate::Callback::new(|_| {});
        let first = ElementSlot::new(callback.clone(), action);
        assert!(first == ElementSlot::new(callback, action));
        assert!(first != ElementSlot::new(crate::Callback::new(|_| {}), action));
    }
}
