use tui_lipan::prelude::*;
use tui_lipan::{CapturedCell, TestBackend};

const THEME_FG: Color = Color::Rgb(20, 21, 22);
const THEME_BG: Color = Color::Rgb(30, 31, 32);
const ALT_THEME_FG: Color = Color::Rgb(40, 41, 42);
const ALT_THEME_BG: Color = Color::Rgb(50, 51, 52);
const REPLACE_FG: Color = Color::Rgb(60, 61, 62);
const REPLACE_BG: Color = Color::Rgb(70, 71, 72);
const EXTEND_FG: Color = Color::Rgb(80, 81, 82);

#[derive(Clone, Copy)]
enum StaticCase {
    ReplaceSelection,
    ExtendSelection,
    InheritSelection,
    TextAreaReplaceSelection,
    TextAreaInheritTextSelection,
    TextAreaDefaultUnfocusedSelectionVisible,
    TextAreaOptOutUnfocusedSelectionHidden,
    TextAreaUnfocusedInheritTextSelection,
    UnfocusedMirrorsCustomSelection,
    UnfocusedDefaultInheritsTheme,
    NestedThemeProviders,
    SelectDefaultButtonFocus,
    CheckboxFocusInherit,
    CheckboxFocusReplace,
    ProgressFocusExtend,
    ProgressFocusReplace,
    SliderFocusInherit,
    SliderFocusReplace,
    FocusDecorationDisabled,
    ExplicitFocusWithDecorationDisabled,
}

impl Component for StaticCase {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        match self {
            Self::ReplaceSelection => themed_list(
                theme_with_selection(selection_style(THEME_FG, THEME_BG).bold()),
                base_list().selection_style(Style::new().fg(REPLACE_FG)),
            ),
            Self::ExtendSelection => themed_list(
                theme_with_selection(selection_style(THEME_FG, THEME_BG).bold()),
                base_list().extend_selection_style(Style::new().fg(EXTEND_FG).italic()),
            ),
            Self::InheritSelection => themed_list(
                theme_with_selection(selection_style(THEME_FG, THEME_BG).underline()),
                base_list().inherit_selection_style(),
            ),
            Self::TextAreaReplaceSelection => ThemeProvider::new(theme_with_selection(
                selection_style(THEME_FG, THEME_BG).bold(),
            ))
            .child(
                TextArea::new("alpha")
                    .cursor(5)
                    .anchor(Some(0))
                    .selection_style(Style::new().fg(REPLACE_FG))
                    .width(Length::Px(10))
                    .height(Length::Px(1))
                    .border(false),
            )
            .into(),
            Self::TextAreaInheritTextSelection => ThemeProvider::new(
                Theme::default()
                    .selection(selection_style(THEME_FG, THEME_BG))
                    .text_selection(selection_style(ALT_THEME_FG, ALT_THEME_BG).underline()),
            )
            .child(
                TextArea::new("alpha")
                    .cursor(5)
                    .anchor(Some(0))
                    .inherit_selection_style()
                    .width(Length::Px(10))
                    .height(Length::Px(1))
                    .border(false),
            )
            .into(),
            Self::TextAreaDefaultUnfocusedSelectionVisible => ThemeProvider::new(
                Theme::default().text_selection(selection_style(ALT_THEME_FG, ALT_THEME_BG)),
            )
            .child(
                VStack::new()
                    .child(Button::new("focus").width(Length::Px(5)))
                    .child(
                        TextArea::default()
                            .value("alpha")
                            .cursor(5)
                            .anchor(Some(0))
                            .width(Length::Px(10))
                            .height(Length::Px(1))
                            .border(false),
                    ),
            )
            .into(),
            Self::TextAreaOptOutUnfocusedSelectionHidden => ThemeProvider::new(
                Theme::default()
                    .text_selection(selection_style(ALT_THEME_FG, ALT_THEME_BG).underline()),
            )
            .child(
                VStack::new()
                    .child(Button::new("focus").width(Length::Px(5)))
                    .child(
                        TextArea::default()
                            .value("alpha")
                            .cursor(5)
                            .anchor(Some(0))
                            .show_selection_when_unfocused(false)
                            .width(Length::Px(10))
                            .height(Length::Px(1))
                            .border(false),
                    ),
            )
            .into(),
            Self::TextAreaUnfocusedInheritTextSelection => ThemeProvider::new(
                Theme::default()
                    .selection(selection_style(THEME_FG, THEME_BG))
                    .text_selection(selection_style(ALT_THEME_FG, ALT_THEME_BG).underline()),
            )
            .child(
                VStack::new()
                    .child(Button::new("focus").width(Length::Px(5)))
                    .child(
                        TextArea::new("alpha")
                            .cursor(5)
                            .anchor(Some(0))
                            .inherit_selection_style()
                            .width(Length::Px(10))
                            .height(Length::Px(1))
                            .border(false),
                    ),
            )
            .into(),
            Self::UnfocusedMirrorsCustomSelection => themed_list(
                theme_with_selection(selection_style(THEME_FG, THEME_BG).bold()),
                base_list()
                    .selection_style(Style::new().fg(REPLACE_FG).bg(REPLACE_BG).italic())
                    .inherit_unfocused_selection_style(),
            ),
            Self::UnfocusedDefaultInheritsTheme => themed_list(
                theme_with_selection(selection_style(THEME_FG, THEME_BG).bold()),
                base_list()
                    .inherit_selection_style()
                    .inherit_unfocused_selection_style(),
            ),
            Self::NestedThemeProviders => VStack::new()
                .child(ThemeProvider::new(theme(THEME_FG, THEME_BG)).child(base_list()))
                .child(ThemeProvider::new(theme(ALT_THEME_FG, ALT_THEME_BG)).child(base_list()))
                .into(),
            Self::SelectDefaultButtonFocus => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(
                Select::new()
                    .options(["alpha"])
                    .selected(Some(0))
                    .button_variant(ButtonVariant::Filled)
                    .width(Length::Px(10)),
            )
            .into(),
            Self::CheckboxFocusInherit => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(Checkbox::new(true).label("check"))
            .into(),
            Self::CheckboxFocusReplace => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(Checkbox::new(true).focus_style(Style::new().fg(REPLACE_FG)))
            .into(),
            Self::ProgressFocusExtend => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(
                ProgressBar::new(1.0)
                    .focusable(true)
                    .extend_focus_style(Style::new().fg(EXTEND_FG))
                    .width(Length::Px(4)),
            )
            .into(),
            Self::ProgressFocusReplace => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(
                ProgressBar::new(1.0)
                    .focusable(true)
                    .focus_style(Style::new().fg(REPLACE_FG))
                    .width(Length::Px(4)),
            )
            .into(),
            Self::SliderFocusInherit => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(Slider::new(0.0).show_value(false).width(Length::Px(4)))
            .into(),
            Self::SliderFocusReplace => ThemeProvider::new(
                Theme::default().focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold()),
            )
            .child(
                Slider::new(0.0)
                    .show_value(false)
                    .focus_thumb_style(Style::new().fg(REPLACE_FG))
                    .width(Length::Px(4)),
            )
            .into(),
            Self::FocusDecorationDisabled => ThemeProvider::new(
                Theme::default()
                    .primary(Style::new().fg(ALT_THEME_FG))
                    .accent(Style::new().fg(ALT_THEME_FG))
                    .focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold())
                    .focus_decoration(false),
            )
            .child(Checkbox::new(true).label("check"))
            .into(),
            Self::ExplicitFocusWithDecorationDisabled => ThemeProvider::new(
                Theme::default()
                    .primary(Style::new().fg(ALT_THEME_FG))
                    .accent(Style::new().fg(ALT_THEME_FG))
                    .focus(Style::new().fg(THEME_FG).bg(THEME_BG).bold())
                    .focus_decoration(false),
            )
            .child(Checkbox::new(true).focus_style(Style::new().fg(REPLACE_FG)))
            .into(),
        }
    }
}

struct MutableTheme;

impl Component for MutableTheme {
    type Message = Msg;
    type Properties = ();
    type State = bool;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        false
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ToggleTheme => ctx.state = !ctx.state,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = if ctx.state {
            theme(ALT_THEME_FG, ALT_THEME_BG)
        } else {
            theme(THEME_FG, THEME_BG)
        };
        themed_list(theme, base_list().inherit_selection_style())
    }
}

#[derive(Clone, Copy)]
enum Msg {
    ToggleTheme,
}

fn selection_style(fg: Color, bg: Color) -> Style {
    Style::new().fg(fg).bg(bg)
}

fn theme_with_selection(selection: Style) -> Theme {
    Theme::default().selection(selection)
}

fn theme(fg: Color, bg: Color) -> Theme {
    theme_with_selection(selection_style(fg, bg))
}

fn base_list() -> List {
    List::new()
        .items([ListItem::new("alpha"), ListItem::new("beta")])
        .selected(0)
        .selection_full_width(true)
        .width(Length::Px(10))
        .height(Length::Px(1))
        .border(false)
}

fn themed_list(theme: Theme, list: List) -> Element {
    ThemeProvider::new(theme).child(list).into()
}

fn render_cell(case: StaticCase, focused: bool, x: u16, y: u16) -> CapturedCell {
    let mut backend = TestBackend::new(case);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    });
    if focused {
        backend.focus_next();
    }
    backend.render();
    backend.capture_frame().cell(x, y).clone()
}

fn render_first_symbol_cell(case: StaticCase, focused: bool, symbol: &str) -> CapturedCell {
    let mut backend = TestBackend::new(case);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    });
    if focused {
        backend.focus_next();
    }
    backend.render();
    backend
        .capture_frame()
        .cells
        .into_iter()
        .find(|cell| cell.symbol == symbol)
        .unwrap_or_else(|| panic!("expected symbol `{symbol}` in captured frame"))
}

#[test]
fn list_selection_replace_blocks_theme_selection_background_leak() {
    let cell = render_cell(StaticCase::ReplaceSelection, true, 0, 0);

    assert_eq!(cell.symbol, "a");
    assert_eq!(cell.fg, REPLACE_FG);
    assert_eq!(cell.bg, Color::Reset);
    assert!(!cell.modifiers.bold);
}

#[test]
fn list_extend_keeps_theme_selection_background_and_modifiers() {
    let cell = render_cell(StaticCase::ExtendSelection, true, 0, 0);

    assert_eq!(cell.fg, EXTEND_FG);
    assert_eq!(cell.bg, THEME_BG);
    assert!(cell.modifiers.bold);
    assert!(cell.modifiers.italic);
}

#[test]
fn list_inherit_uses_theme_verbatim() {
    let cell = render_cell(StaticCase::InheritSelection, true, 0, 0);

    assert_eq!(cell.fg, THEME_FG);
    assert_eq!(cell.bg, THEME_BG);
    assert!(cell.modifiers.underline);
}

#[test]
fn text_area_selection_replace_blocks_theme_selection_background_leak() {
    let cell = render_cell(StaticCase::TextAreaReplaceSelection, true, 0, 0);

    assert_eq!(cell.symbol, "a");
    assert_eq!(cell.fg, REPLACE_FG);
    assert_eq!(cell.bg, Color::Reset);
    assert!(!cell.modifiers.bold);
}

#[test]
fn text_area_inherit_uses_text_selection_theme_role() {
    let cell = render_cell(StaticCase::TextAreaInheritTextSelection, true, 0, 0);

    assert_eq!(cell.symbol, "a");
    assert_eq!(cell.fg, ALT_THEME_FG);
    assert_eq!(cell.bg, ALT_THEME_BG);
    assert!(cell.modifiers.underline);
}

#[test]
fn text_area_default_shows_unfocused_selection() {
    let cell = render_cell(
        StaticCase::TextAreaDefaultUnfocusedSelectionVisible,
        false,
        0,
        1,
    );

    assert_eq!(cell.symbol, "a");
    assert_eq!(cell.fg, ALT_THEME_FG);
    assert_eq!(cell.bg, ALT_THEME_BG);
}

#[test]
fn text_area_opt_out_hides_unfocused_selection() {
    let cell = render_cell(
        StaticCase::TextAreaOptOutUnfocusedSelectionHidden,
        false,
        0,
        1,
    );

    assert_eq!(cell.symbol, "a");
    assert_ne!(cell.bg, ALT_THEME_BG);
    assert!(!cell.modifiers.underline);
}

#[test]
fn text_area_unfocused_inherit_uses_text_selection_theme_role() {
    let cell = render_cell(
        StaticCase::TextAreaUnfocusedInheritTextSelection,
        false,
        0,
        1,
    );

    assert_eq!(cell.symbol, "a");
    assert_eq!(cell.fg, ALT_THEME_FG);
    assert_eq!(cell.bg, ALT_THEME_BG);
    assert!(cell.modifiers.underline);
}

#[test]
fn unfocused_list_selection_with_inherited_unfocused_slot_mirrors_custom_selection_slot() {
    let cell = render_cell(StaticCase::UnfocusedMirrorsCustomSelection, false, 0, 0);

    assert_eq!(cell.fg, REPLACE_FG);
    assert_eq!(cell.bg, REPLACE_BG);
    assert!(cell.modifiers.italic);
    assert!(!cell.modifiers.bold);
}

#[test]
fn unfocused_default_selection_uses_theme_when_both_slots_inherit() {
    let cell = render_cell(StaticCase::UnfocusedDefaultInheritsTheme, false, 0, 0);

    assert_eq!(cell.fg, THEME_FG);
    assert_eq!(cell.bg, THEME_BG);
    assert!(cell.modifiers.bold);
}

#[test]
fn nested_theme_provider_sibling_lists_resolve_selection_against_active_provider() {
    let first = render_cell(StaticCase::NestedThemeProviders, false, 0, 0);
    let second = render_cell(StaticCase::NestedThemeProviders, false, 0, 1);

    assert_eq!(first.fg, THEME_FG);
    assert_eq!(first.bg, THEME_BG);
    assert_eq!(second.fg, ALT_THEME_FG);
    assert_eq!(second.bg, ALT_THEME_BG);
}

#[test]
fn select_default_button_focus_inherits_theme_focus_style() {
    let cell = render_first_symbol_cell(StaticCase::SelectDefaultButtonFocus, true, "a");

    assert_eq!(cell.fg, THEME_FG);
    assert_eq!(cell.bg, THEME_BG);
    assert!(cell.modifiers.bold);
}

#[test]
fn checkbox_focus_slot_inherit_and_replace_render_distinctly() {
    let inherited = render_first_symbol_cell(StaticCase::CheckboxFocusInherit, true, "[");
    assert_eq!(inherited.fg, THEME_FG);
    assert_eq!(inherited.bg, THEME_BG);
    assert!(inherited.modifiers.bold);

    let replaced = render_first_symbol_cell(StaticCase::CheckboxFocusReplace, true, "[");
    assert_eq!(replaced.fg, REPLACE_FG);
    assert_eq!(replaced.bg, Color::Reset);
    assert!(!replaced.modifiers.bold);
}

#[test]
fn focused_render_suppresses_theme_decoration_when_disabled() {
    let cell = render_first_symbol_cell(StaticCase::FocusDecorationDisabled, true, "[");

    assert_eq!(cell.fg, ALT_THEME_FG);
    assert_eq!(cell.bg, Color::Reset);
    assert!(!cell.modifiers.bold);
}

#[test]
fn focused_render_keeps_explicit_focus_style_when_theme_decoration_is_disabled() {
    let cell = render_first_symbol_cell(StaticCase::ExplicitFocusWithDecorationDisabled, true, "[");

    assert_eq!(cell.fg, REPLACE_FG);
    assert_eq!(cell.bg, Color::Reset);
    assert!(!cell.modifiers.bold);
}

#[test]
fn progress_focus_and_hover_slots_render_with_slot_semantics() {
    let focused = render_cell(StaticCase::ProgressFocusExtend, true, 0, 0);
    assert_eq!(focused.fg, EXTEND_FG);
    assert_eq!(focused.bg, THEME_BG);
    assert!(focused.modifiers.bold);

    let replaced = render_cell(StaticCase::ProgressFocusReplace, true, 0, 0);
    assert_eq!(replaced.fg, REPLACE_FG);
    assert_ne!(replaced.bg, THEME_BG);
    assert!(!replaced.modifiers.bold);
}

#[test]
fn slider_focus_and_hover_thumb_slots_render_with_slot_semantics() {
    let focused = render_first_symbol_cell(StaticCase::SliderFocusInherit, true, "●");
    assert_eq!(focused.fg, THEME_FG);
    assert_eq!(focused.bg, THEME_BG);
    assert!(focused.modifiers.bold);

    let replaced = render_first_symbol_cell(StaticCase::SliderFocusReplace, true, "●");
    assert_eq!(replaced.fg, REPLACE_FG);
    assert_eq!(replaced.bg, Color::Reset);
    assert!(!replaced.modifiers.bold);
}

#[test]
fn top_level_theme_provider_change_refreshes_selection_style() {
    let mut backend = TestBackend::new(MutableTheme);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 2,
    });
    backend.render();

    let before = backend.capture_frame().cell(0, 0).clone();
    backend
        .dispatch(Msg::ToggleTheme)
        .expect("theme toggle dispatch should succeed");
    let after = backend.capture_frame().cell(0, 0).clone();

    assert_eq!(before.fg, THEME_FG);
    assert_eq!(before.bg, THEME_BG);
    assert_eq!(after.fg, ALT_THEME_FG);
    assert_eq!(after.bg, ALT_THEME_BG);
}
