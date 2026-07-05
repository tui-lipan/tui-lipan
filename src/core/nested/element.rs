use std::any::TypeId;
use std::rc::Rc;

use super::any_props::AnyProps;
use super::erased::{ComponentFactory, ComponentMount, Mounted};
use crate::core::component::{Component, Context};
use crate::core::element::Key;
use crate::style::Rect;

/// A nested component element (type-erased).
#[derive(Clone)]
pub(crate) struct ComponentElement {
    pub(crate) type_id: TypeId,
    pub(crate) factory: ComponentFactory,
    pub(crate) props: AnyProps,
    /// Path-independent persistence key. When set, the component's instance is
    /// looked up in a registry-global index before falling back to the usual
    /// path-based reuse, so its state survives ancestor reshaping.
    pub(crate) state_key: Option<Key>,
}

impl ComponentElement {
    pub(crate) fn new<C, F>(factory: F, props: C::Properties) -> Self
    where
        C: Component,
        F: Fn() -> C + 'static,
    {
        let type_id = TypeId::of::<C>();

        let factory: ComponentFactory = Rc::new(move |mount: ComponentMount| {
            let component = factory();

            let expected = std::any::type_name::<C::Properties>();
            let actual = mount.props.type_name();
            let component_name = std::any::type_name::<C>();

            let props: C::Properties =
                mount.props.into_typed::<C::Properties>().ok_or_else(|| {
                    crate::Error::PropsTypeMismatch {
                        component: component_name,
                        expected,
                        actual,
                    }
                })?;

            let ctx = Context::new(
                &component,
                mount.scope,
                mount.dispatcher,
                props,
                mount.env,
                Rect::default(),
            );

            Ok(Box::new(Mounted { component, ctx }))
        });

        Self {
            type_id,
            factory,
            props: AnyProps::new(props),
            state_key: None,
        }
    }
}
