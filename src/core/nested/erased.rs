use std::any::{Any, TypeId};
use std::rc::Rc;
use std::sync::Arc;

use super::any_props::AnyProps;
use crate::callback::{Dispatcher, ScopeId};
use crate::core::component::{Command, Component, Context, KeyUpdate, Update};
use crate::core::element::Element;
use crate::core::event::KeyEvent;
use crate::core::runtime_env::{MemoDependencySnapshot, RuntimeEnv};
use crate::style::{Rect, Theme};

/// Mount context passed to a component factory.
pub(crate) struct ComponentMount {
    pub(crate) scope: ScopeId,
    pub(crate) dispatcher: Dispatcher,
    pub(crate) env: RuntimeEnv,
    pub(crate) props: AnyProps,
}

/// A mounted, type-erased component instance.
pub(crate) trait ErasedComponent {
    /// Returns `true` if the given props are equal to the current props,
    /// without cloning.
    fn props_equal(&self, props: &AnyProps) -> bool;
    fn set_props(&mut self, props: AnyProps) -> Update;
    fn set_active_theme(&mut self, theme: Theme);
    fn set_contexts(
        &mut self,
        contexts: rustc_hash::FxHashMap<TypeId, Arc<dyn Any>>,
        generations: rustc_hash::FxHashMap<TypeId, u64>,
    );
    fn set_viewport(&mut self, viewport: Rect);
    fn memo_key(&self) -> Option<u64>;
    fn begin_memo_dependency_capture(&self);
    fn finish_memo_dependency_capture(&self) -> MemoDependencySnapshot;
    fn memo_dependencies_match(&self, snapshot: &MemoDependencySnapshot) -> bool;
    fn init(&mut self) -> Option<Command>;
    fn view(&self) -> Element;
    fn update(&mut self, msg: Box<dyn Any>) -> crate::Result<Update>;
    fn on_key(&mut self, key: KeyEvent) -> KeyUpdate;
    fn unmount(&mut self);
}

pub(crate) type ComponentFactory =
    Rc<dyn Fn(ComponentMount) -> crate::Result<Box<dyn ErasedComponent>>>;

pub(crate) struct Mounted<C: Component> {
    pub component: C,
    pub ctx: Context<C>,
}

impl<C: Component> ErasedComponent for Mounted<C> {
    fn props_equal(&self, props: &AnyProps) -> bool {
        props
            .downcast_ref::<C::Properties>()
            .is_some_and(|p| &self.ctx.props == p)
    }

    fn set_props(&mut self, props: AnyProps) -> Update {
        let Some(p) = props.downcast_ref::<C::Properties>() else {
            return Update::none();
        };

        if &self.ctx.props != p {
            let old_props = std::mem::replace(&mut self.ctx.props, p.clone());
            self.component.on_props_changed(&old_props, &mut self.ctx)
        } else {
            Update::none()
        }
    }

    fn set_active_theme(&mut self, theme: Theme) {
        self.ctx.set_active_theme(theme);
    }

    fn set_contexts(
        &mut self,
        contexts: rustc_hash::FxHashMap<TypeId, Arc<dyn Any>>,
        generations: rustc_hash::FxHashMap<TypeId, u64>,
    ) {
        self.ctx.set_contexts(contexts, generations);
    }

    fn set_viewport(&mut self, viewport: Rect) {
        self.ctx.set_viewport(viewport);
    }

    fn memo_key(&self) -> Option<u64> {
        self.ctx.memo_key(&self.component)
    }

    fn begin_memo_dependency_capture(&self) {
        self.ctx.begin_memo_dependency_capture();
    }

    fn finish_memo_dependency_capture(&self) -> MemoDependencySnapshot {
        self.ctx.finish_memo_dependency_capture()
    }

    fn memo_dependencies_match(&self, snapshot: &MemoDependencySnapshot) -> bool {
        self.ctx.memo_dependencies_match(snapshot)
    }

    fn init(&mut self) -> Option<Command> {
        self.component.init(&mut self.ctx)
    }

    fn view(&self) -> Element {
        self.component.view(&self.ctx)
    }

    fn update(&mut self, msg: Box<dyn Any>) -> crate::Result<Update> {
        let actual = std::any::type_name_of_val(msg.as_ref());
        let expected = std::any::type_name::<C::Message>();
        let component = std::any::type_name::<C>();
        let msg = msg
            .downcast::<C::Message>()
            .map_err(|_| crate::Error::MessageTypeMismatch {
                component,
                expected,
                actual,
            })?;
        Ok(self.component.update(*msg, &mut self.ctx))
    }

    fn on_key(&mut self, key: KeyEvent) -> KeyUpdate {
        self.component.on_key(key, &mut self.ctx)
    }

    fn unmount(&mut self) {
        self.component.unmount(&mut self.ctx);
    }
}

pub(crate) struct EmptyComponent;

impl ErasedComponent for EmptyComponent {
    fn props_equal(&self, _props: &AnyProps) -> bool {
        true
    }

    fn set_props(&mut self, _props: AnyProps) -> Update {
        Update::none()
    }

    fn set_active_theme(&mut self, _theme: Theme) {}

    fn set_contexts(
        &mut self,
        _contexts: rustc_hash::FxHashMap<TypeId, Arc<dyn Any>>,
        _generations: rustc_hash::FxHashMap<TypeId, u64>,
    ) {
    }

    fn set_viewport(&mut self, _viewport: Rect) {}

    fn memo_key(&self) -> Option<u64> {
        None
    }

    fn begin_memo_dependency_capture(&self) {}

    fn finish_memo_dependency_capture(&self) -> MemoDependencySnapshot {
        MemoDependencySnapshot::default()
    }

    fn memo_dependencies_match(&self, _snapshot: &MemoDependencySnapshot) -> bool {
        true
    }

    fn init(&mut self) -> Option<Command> {
        None
    }

    fn view(&self) -> Element {
        crate::widgets::Text::new("").into()
    }

    fn update(&mut self, _msg: Box<dyn Any>) -> crate::Result<Update> {
        Ok(Update::none())
    }

    fn on_key(&mut self, _key: KeyEvent) -> KeyUpdate {
        KeyUpdate::unhandled(Update::none())
    }

    fn unmount(&mut self) {}
}
