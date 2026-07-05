use crate::core::element::Element;
use crate::layout::axis::Axis;
use crate::widgets::internal::StackProps;
use crate::widgets::internal::measure_stack;

pub(crate) fn measure_scroll_view(
    props: &StackProps,
    children: &[Element],
    max_w: Option<u16>,
) -> (u16, u16) {
    measure_stack(props, children, Axis::Vertical, None, max_w)
}
