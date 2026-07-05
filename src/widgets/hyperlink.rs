//! Hyperlink widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseEvent};
use crate::style::{Align, Length, Padding, Style, StyleSlot};
use crate::widgets::Button;

/// Event emitted when a [`Hyperlink`] is activated.
#[derive(Clone, Debug)]
pub struct HyperlinkEvent {
    /// Link label text.
    pub label: Arc<str>,
    /// Optional destination URL.
    pub href: Option<Arc<str>>,
}

/// Clickable text-style link widget.
///
/// This is an interactive wrapper over [`Button`] with link-focused defaults
/// (underlined text, no chrome, keyboard activation).
#[derive(Clone)]
pub struct Hyperlink {
    label: Arc<str>,
    href: Option<Arc<str>>,
    style: Style,
    hover_style: StyleSlot,
    focus_style: StyleSlot,
    disabled_style: Style,
    visited_style: Option<Style>,
    width: Length,
    height: Length,
    align: Align,
    padding: Padding,
    focusable: bool,
    disabled: bool,
    visited: bool,
    on_activate: Option<Callback<HyperlinkEvent>>,
    on_key: Option<KeyHandler>,
}

impl Hyperlink {
    /// Create a new hyperlink with the given visible label.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            href: None,
            style: Style::new().underline(),
            hover_style: StyleSlot::Extend(Style::new().underline()),
            focus_style: StyleSlot::Extend(Style::new().underline().bold()),
            disabled_style: Style::default(),
            visited_style: None,
            width: Length::Auto,
            height: Length::Auto,
            align: Align::Start,
            padding: Padding::default(),
            focusable: true,
            disabled: false,
            visited: false,
            on_activate: None,
            on_key: None,
        }
    }

    /// Set destination URL associated with this hyperlink.
    pub fn href(mut self, href: impl Into<Arc<str>>) -> Self {
        self.href = Some(href.into());
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hover style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set focus style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focus style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed focus style.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set style overlay applied when `visited(true)`.
    pub fn visited_style(mut self, style: Style) -> Self {
        self.visited_style = Some(style);
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

    /// Set label alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Set inner padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Control focus traversal participation.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Mark the hyperlink as visited.
    pub fn visited(mut self, visited: bool) -> Self {
        self.visited = visited;
        self
    }

    /// Set activation callback (mouse click, `Enter`, or `Space`).
    ///
    /// For the common "just open the URL" case, see [`crate::callbacks::open_hyperlink`].
    pub fn on_activate(mut self, cb: Callback<HyperlinkEvent>) -> Self {
        self.on_activate = Some(cb);
        self
    }

    /// Set keyboard handler.
    ///
    /// The custom handler runs when activation keys are not handled.
    pub fn on_key(mut self, cb: KeyHandler) -> Self {
        self.on_key = Some(cb);
        self
    }
}

impl From<Hyperlink> for Element {
    fn from(value: Hyperlink) -> Self {
        let Hyperlink {
            label,
            href,
            style,
            hover_style,
            focus_style,
            disabled_style,
            visited_style,
            width,
            height,
            align,
            padding,
            focusable,
            disabled,
            visited,
            on_activate,
            on_key,
        } = value;

        let style = apply_visited_style(style, visited, visited_style);

        let mut button = Button::filled(label.clone())
            .style(style)
            .hover_style_slot(hover_style)
            .focus_style_slot(focus_style)
            .disabled_style(disabled_style)
            .width(width)
            .height(height)
            .align(align)
            .padding(padding)
            .focusable(focusable && !disabled)
            .disabled(disabled);

        if let Some(cb) = on_activate.clone() {
            let event_label = label.clone();
            let event_href = href.clone();
            button = button.on_click(Callback::new(move |_: MouseEvent| {
                cb.emit(HyperlinkEvent {
                    label: event_label.clone(),
                    href: event_href.clone(),
                });
            }));
        }

        match (on_activate, on_key) {
            (Some(cb), custom_key) => {
                let event_label = label;
                let event_href = href;
                button = button.on_key(KeyHandler::new(move |key: KeyEvent| {
                    if is_activation_key(key) {
                        cb.emit(HyperlinkEvent {
                            label: event_label.clone(),
                            href: event_href.clone(),
                        });
                        return true;
                    }
                    custom_key.as_ref().map(|h| h.handle(key)).unwrap_or(false)
                }));
            }
            (None, Some(custom_key)) => {
                button = button.on_key(custom_key);
            }
            (None, None) => {}
        }

        button.into()
    }
}

fn is_activation_key(key: KeyEvent) -> bool {
    if has_non_shift_modifiers(key.mods) {
        return false;
    }

    matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
}

fn has_non_shift_modifiers(mods: KeyMods) -> bool {
    mods.ctrl || mods.alt || mods.super_key
}

fn apply_visited_style(style: Style, visited: bool, visited_style: Option<Style>) -> Style {
    if visited {
        style.patch(visited_style.unwrap_or_default())
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::{apply_visited_style, is_activation_key};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::style::Style;

    #[test]
    fn enter_and_space_are_activation_keys() {
        let enter = KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::default(),
        };
        let space = KeyEvent {
            code: KeyCode::Char(' '),
            mods: KeyMods::default(),
        };

        assert!(is_activation_key(enter));
        assert!(is_activation_key(space));
    }

    #[test]
    fn ctrl_modified_key_is_not_activation() {
        let key = KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };

        assert!(!is_activation_key(key));
    }

    #[test]
    fn visited_style_overlays_base_style() {
        let base = Style::new().fg(crate::style::Color::Blue).underline();
        let visited = Style::new().fg(crate::style::Color::Magenta);

        let resolved = apply_visited_style(base, true, Some(visited));

        assert_eq!(resolved.fg, Some(crate::style::Color::Magenta.into()));
        assert_eq!(resolved.underline, Some(true));
    }
}
