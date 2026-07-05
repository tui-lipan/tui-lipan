use std::any::Any;

use crate::style::Theme;

/// Type-erased, cloneable component properties.
pub(crate) struct AnyProps {
    pub(crate) value: Box<dyn Any>,
    clone_fn: fn(&Box<dyn Any>) -> Box<dyn Any>,
    eq_fn: fn(&dyn Any, &dyn Any) -> bool,
}

impl Clone for AnyProps {
    fn clone(&self) -> Self {
        Self {
            value: (self.clone_fn)(&self.value),
            clone_fn: self.clone_fn,
            eq_fn: self.eq_fn,
        }
    }
}

impl AnyProps {
    pub(crate) fn new<P>(props: P) -> Self
    where
        P: Clone + PartialEq + 'static,
    {
        fn clone_value<P>(v: &Box<dyn Any>) -> Box<dyn Any>
        where
            P: Clone + 'static,
        {
            let p = v.downcast_ref::<P>().unwrap_or_else(|| {
                let expected = std::any::type_name::<P>();
                let actual = std::any::type_name_of_val(v.as_ref());
                panic!("AnyProps type mismatch (expected `{expected}`, got `{actual}`)");
            });
            Box::new(p.clone())
        }

        fn eq_value<P>(a: &dyn Any, b: &dyn Any) -> bool
        where
            P: PartialEq + 'static,
        {
            match (a.downcast_ref::<P>(), b.downcast_ref::<P>()) {
                (Some(a), Some(b)) => a == b,
                _ => false,
            }
        }

        Self {
            value: Box::new(props),
            clone_fn: clone_value::<P>,
            eq_fn: eq_value::<P>,
        }
    }

    pub(crate) fn downcast_ref<P: 'static>(&self) -> Option<&P> {
        self.value.downcast_ref::<P>()
    }

    pub(crate) fn into_typed<P: 'static>(self) -> Option<P> {
        self.value.downcast::<P>().ok().map(|v| *v)
    }

    pub(crate) fn type_name(&self) -> &'static str {
        std::any::type_name_of_val(self.value.as_ref())
    }

    #[cfg(debug_assertions)]
    pub(crate) fn debug_eq(&self, other: &Self) -> bool {
        if self.value.as_ref().type_id() != other.value.as_ref().type_id() {
            return false;
        }
        (self.eq_fn)(self.value.as_ref(), other.value.as_ref())
    }
}

/// Trait for component properties that can be themed.
pub trait ThemableProps {
    /// Apply a theme to the properties.
    fn apply_theme(&mut self, theme: &Theme);
}
