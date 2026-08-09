//! Compile-time guardrail for shared interaction builder APIs.
//!
//! If a method is dropped from a focusable widget's public builder surface, or a
//! newly added focusable widget omits one of the shared props, this test fails to
//! compile. Runtime assertions are intentionally absent.

#![allow(unused_must_use)]

use tui_lipan::KeyHandler;
use tui_lipan::prelude::*;
use tui_lipan::style::StyleSlot;

fn interaction_chain_button() {
    let _ = Button::new("ok")
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_checkbox() {
    let _ = Checkbox::new(false)
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_input() {
    let _ = Input::new("")
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_text_area() {
    let _ = TextArea::new("")
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_slider() {
    let _ = Slider::new(0.0)
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_radio() {
    let _ = Radio::new(["a", "b"])
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .focus_key("radio-test")
        .on_focus(Callback::new(|_i| {}))
        .on_blur(Callback::new(|_i| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_date_picker() {
    let _ = DatePicker::new()
        .disabled(false)
        .disabled_style(Style::default())
        .focusable(true)
        .tab_stop(true)
        .focus_key("datepicker-test")
        .on_focus(Callback::new(|_ev| {}))
        .on_blur(Callback::new(|_ev| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_select() {
    let _ = Select::new()
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_combo_box() {
    let _ = ComboBox::new()
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_multi_select() {
    let _ = MultiSelect::new()
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

fn interaction_chain_hyperlink() {
    let _ = Hyperlink::new("link")
        .disabled(false)
        .disabled_style(Style::default())
        .hover_style(Style::default())
        .extend_hover_style(Style::default())
        .inherit_hover_style()
        .hover_style_slot(StyleSlot::Inherit)
        .focus_style(Style::default())
        .extend_focus_style(Style::default())
        .inherit_focus_style()
        .focus_style_slot(StyleSlot::Inherit)
        .focusable(true)
        .tab_stop(true)
        .on_focus(Callback::new(|_| {}))
        .on_blur(Callback::new(|_| {}))
        .on_key(KeyHandler::new(|_key| false));
}

#[test]
fn interaction_parity_builders_compile() {
    interaction_chain_button();
    interaction_chain_checkbox();
    interaction_chain_input();
    interaction_chain_text_area();
    interaction_chain_slider();
    interaction_chain_radio();
    interaction_chain_date_picker();
    interaction_chain_select();
    interaction_chain_combo_box();
    interaction_chain_multi_select();
    interaction_chain_hyperlink();
}
