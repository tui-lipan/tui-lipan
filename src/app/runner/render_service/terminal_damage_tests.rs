//! Does an incrementally patched frame equal an ordinary full paint?
//!
//! The earlier discriminators ran through `render_headless`, whose context is deliberately
//! simplified: no images, contrast off, blink always visible, no drag or copy feedback. They
//! proved the clipping property. They could not prove it survives the context the runner actually
//! builds, and that is what these tests exist for: every case here goes through
//! `AppRunner::with_render_context`, the same construction `draw_current_tree` uses.
//!
//! Each accepted case asserts three things:
//!
//! - the patched retained frame equals a full paint of the same tree, cell for cell;
//! - what the *host* was sent equals it too, which is the assertion that catches a row the patch
//!   silently failed to send;
//! - every row that moved is in the plan, which is the assertion that catches the emulator
//!   under-reporting damage. Without it a patch that skips a changed row still passes the first
//!   two, because the row it never touched matches on both sides.
//!
//! Rejected cases assert the typed reason instead. There is nothing to compare when no patch was
//! ever going to happen, and a reason is a stronger statement than a `None`.

use std::cell::{Cell as StdCell, RefCell};
use std::rc::Rc;

use ratatui::Terminal;
use ratatui::backend::{Backend, TestBackend};
use ratatui::buffer::Buffer;
use ratatui::layout::{Position, Rect as RatatuiRect};

use super::AppRunner;
use super::row_damage::{DamageRejection, RowDamagePlan, patch_rows, place_caret};
use crate::app::App;
use crate::app::context::SurfaceMode;
use crate::backend::ratatui_backend::render;
use crate::core::component::{Component, Context, Update};
use crate::core::element::Element;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::runtime::RuntimeCore;
use crate::style::{Color, Rect, Style, Theme};
use crate::widgets::{
    Terminal as TerminalWidget, TerminalPos, TerminalScreen, TerminalScreenHandle,
    TerminalSelection, Text, TextSource, VStack,
};

const COLS: u16 = 40;
const ROWS: u16 = 12;

/// A `TestBackend` that also moves its cursor the way a real one does.
///
/// `TestBackend::draw` writes cells and leaves the cursor alone. No terminal behaves that way:
/// crossterm prints each cell where it sits, so a draw ends with the cursor one column past the
/// last symbol it sent. Every caret bug this file exists to catch is a caret some draw walked off
/// with, so a backend that does not model that cannot see any of them.
struct HostBackend {
    inner: TestBackend,
}

impl HostBackend {
    fn new(width: u16, height: u16) -> Self {
        Self {
            inner: TestBackend::new(width, height),
        }
    }

    fn buffer(&self) -> &Buffer {
        self.inner.buffer()
    }
}

impl Backend for HostBackend {
    type Error = <TestBackend as Backend>::Error;

    fn draw<'a, I>(&mut self, content: I) -> std::result::Result<(), Self::Error>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        let cells: Vec<(u16, u16, &ratatui::buffer::Cell)> = content.collect();
        let landed = cells.last().map(|(x, y, cell)| {
            let width = unicode_width::UnicodeWidthStr::width(cell.symbol()).max(1) as u16;
            Position::new(x.saturating_add(width), *y)
        });
        self.inner.draw(cells.into_iter())?;
        if let Some(position) = landed {
            self.inner.set_cursor_position(position)?;
        }
        Ok(())
    }

    fn hide_cursor(&mut self) -> std::result::Result<(), Self::Error> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> std::result::Result<(), Self::Error> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> std::result::Result<Position, Self::Error> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(
        &mut self,
        position: P,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> std::result::Result<(), Self::Error> {
        self.inner.clear()
    }

    fn clear_region(
        &mut self,
        clear_type: ratatui::backend::ClearType,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.clear_region(clear_type)
    }

    fn append_lines(&mut self, count: u16) -> std::result::Result<(), Self::Error> {
        self.inner.append_lines(count)
    }

    fn size(&self) -> std::result::Result<ratatui::layout::Size, Self::Error> {
        self.inner.size()
    }

    fn window_size(&mut self) -> std::result::Result<ratatui::backend::WindowSize, Self::Error> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> std::result::Result<(), Self::Error> {
        self.inner.flush()
    }

    fn scroll_region_up(
        &mut self,
        region: std::ops::Range<u16>,
        count: u16,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.scroll_region_up(region, count)
    }

    fn scroll_region_down(
        &mut self,
        region: std::ops::Range<u16>,
        count: u16,
    ) -> std::result::Result<(), Self::Error> {
        self.inner.scroll_region_down(region, count)
    }
}

/// What the pane under test looks like. Each flag exists because it changes the cells the terminal
/// renderer produces, so a patch that ignored it would differ from a full paint.
#[derive(Clone, Copy, Default)]
struct PaneCfg {
    live_title: bool,
    focused: bool,
    hovered: bool,
    themed: bool,
    selection: bool,
    scrollbar: bool,
    border: bool,
    padding: u16,
    blink_hidden: bool,
    /// Terminal rows. Fewer than `ROWS` leaves blank frame rows under the content.
    grid_rows: u16,
    /// Mount a second terminal beside the first.
    second_terminal: bool,
    /// Display the same screen in both terminal widgets.
    shared_screen: bool,
    /// Focus the second terminal rather than the first, so the caret sits on a row the first
    /// terminal's output never damages.
    focus_second: bool,
    /// Place each terminal's caret with `cursor_override`, as a copy-mode cursor does.
    cursor_override: Option<(u16, u16)>,
    /// Scroll the second terminal back this many rows before the first paint.
    second_scrollback: usize,
    /// Run on an inline surface rather than the alternate screen.
    inline: bool,
    image_overlay: u8,
    transparent_modal: bool,
    terminal_portal: bool,
    spinner: bool,
    /// Show a one-row notice in the middle of the screen over the pane: `1` on its own, `2` with a
    /// backdrop dimming everything else, `3` inside an `Animated` wrapper a test can move.
    toast: u8,
    /// Run the runtime on a controlled clock, so a test steps time itself.
    controlled_clock: bool,
}

impl PaneCfg {
    fn new() -> Self {
        Self {
            grid_rows: ROWS,
            ..Self::default()
        }
    }

    fn grid_rows(self) -> u16 {
        if self.grid_rows == 0 {
            ROWS
        } else {
            self.grid_rows
        }
    }
}

struct Pane {
    title: TextSource,
    screen: Rc<RefCell<TerminalScreen>>,
    second: Option<Rc<RefCell<TerminalScreen>>>,
    cfg: PaneCfg,
}

impl Pane {
    fn terminal(&self, screen: &Rc<RefCell<TerminalScreen>>) -> TerminalWidget {
        let mut widget = TerminalWidget::new().screen(TerminalScreenHandle::new(Rc::clone(screen)));
        if self.cfg.themed {
            widget = widget
                .style(Style::default().fg(Color::Yellow).bg(Color::Blue))
                .focus_content_style(Style::default().fg(Color::Green))
                .hover_style(Style::default().bg(Color::Magenta))
                .selection_style(Style::default().fg(Color::Black).bg(Color::Cyan));
        }
        if self.cfg.selection {
            widget = widget.selection(Some(TerminalSelection {
                anchor: TerminalPos { line: 1, col: 2 },
                cursor: TerminalPos { line: 1, col: 9 },
            }));
        }
        if self.cfg.scrollbar {
            widget = widget.scrollbar(true);
        }
        if self.cfg.border {
            widget = widget.border(true);
        }
        if self.cfg.padding > 0 {
            widget = widget.padding(self.cfg.padding);
        }
        widget.cursor_override(self.cfg.cursor_override)
    }
}

impl Component for Pane {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut body: Element = match &self.second {
            Some(second) => VStack::new()
                .child(self.terminal(&self.screen))
                .child(self.terminal(second))
                .into(),
            None => self.terminal(&self.screen).into(),
        };
        if self.cfg.spinner {
            body = VStack::new()
                .child(crate::widgets::Spinner::new().height(crate::style::Length::Px(1)))
                .child(body)
                .into();
        }
        if self.cfg.toast != 0 {
            use crate::core::element::ElementKind;
            use crate::overlay::{
                DismissPolicy, OverlayLayer, OverlayPlacement, PointerCapture, Portal,
            };
            let notice: Element = Text::new("NOTICE")
                .style(Style::default().fg(Color::Black).bg(Color::Yellow))
                .into();
            let notice = notice
                .min_width(crate::style::Length::Px(6))
                .max_width(crate::style::Length::Px(6))
                .min_height(crate::style::Length::Px(1))
                .max_height(crate::style::Length::Px(1));
            let notice: Element = if self.cfg.toast == 3 {
                crate::widgets::Animated::new(notice).into()
            } else {
                notice
            };
            let portal = Element::new(ElementKind::Portal(Portal {
                layer: OverlayLayer::Toast,
                content: Box::new(notice),
                placement: OverlayPlacement::Center {
                    reserve_height: None,
                },
                dismiss_policy: DismissPolicy::None,
                on_close: None,
                backdrop: (self.cfg.toast == 2).then(|| Style::default().bg(Color::Black)),
                captures_focus: false,
                auto_focus: false,
                captures_pointer: PointerCapture::None,
                animation: None,
            }));
            return crate::widgets::ZStack::new()
                .child(body)
                .child(portal)
                .into();
        }
        if matches!(self.cfg.image_overlay, 19 | 20) {
            use crate::core::element::ElementKind;
            use crate::overlay::{
                DismissPolicy, OverlayLayer, OverlayPlacement, PointerCapture, Portal,
            };
            let border: Element = crate::widgets::Frame::new()
                .padding(0)
                .style(Style::default().fg(Color::White))
                .child(Text::new(""))
                .into();
            let (portal_content, main_content) = if self.cfg.image_overlay == 19 {
                (border, body)
            } else {
                (body, border)
            };
            let portal = Element::new(ElementKind::Portal(Portal {
                layer: OverlayLayer::Modal,
                content: Box::new(
                    portal_content
                        .min_width(crate::style::Length::Px(COLS))
                        .max_width(crate::style::Length::Px(COLS))
                        .min_height(crate::style::Length::Px(ROWS))
                        .max_height(crate::style::Length::Px(ROWS)),
                ),
                placement: OverlayPlacement::Center {
                    reserve_height: None,
                },
                dismiss_policy: DismissPolicy::None,
                on_close: None,
                backdrop: None,
                captures_focus: false,
                auto_focus: false,
                captures_pointer: PointerCapture::None,
                animation: None,
            }));
            return crate::widgets::ZStack::new()
                .child(portal)
                .child(main_content)
                .into();
        }
        if self.cfg.terminal_portal {
            return crate::widgets::Modal::new()
                .width(crate::style::Length::Px(COLS))
                .height(crate::style::Length::Px(ROWS))
                .border(false)
                .padding(0)
                .frame_style(Style::default().bg(Color::Transparent))
                .child(body)
                .into();
        }
        if self.cfg.transparent_modal {
            return crate::widgets::ZStack::new()
                .child(body)
                .child(crate::widgets::Animated::new(
                    crate::widgets::Modal::new()
                        .width(crate::style::Length::Px(5))
                        .height(crate::style::Length::Px(1))
                        .border(false)
                        .padding(0)
                        .frame_style(Style::default().bg(Color::Transparent))
                        .child(Text::new("COVER")),
                ))
                .into();
        }
        if matches!(
            self.cfg.image_overlay,
            8 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 21
        ) {
            let border = crate::widgets::Frame::new()
                .padding(0)
                .style(Style::default().fg(Color::White))
                .child(Text::new(""));
            let border = match self.cfg.image_overlay {
                12 | 14 => crate::widgets::Animated::new(border).opacity(0.5),
                15 => crate::widgets::Animated::new(border).opacity(0.0),
                16 | 21 => crate::widgets::Animated::new(border).opacity(0.25),
                17 => crate::widgets::Animated::new(border).opacity(0.75),
                18 => crate::widgets::Animated::new(border)
                    .opacity(0.0)
                    .opacity_fg_only(true),
                13 => crate::widgets::Animated::new(border)
                    .opacity(0.5)
                    .opacity_target(Color::Black),
                _ => crate::widgets::Animated::new(border),
            };
            let body = if matches!(self.cfg.image_overlay, 14..=18) {
                body
            } else {
                crate::widgets::Frame::new().padding(0).child(body).into()
            };
            return crate::widgets::Canvas::new()
                .child_at(viewport(), body)
                .child_at(viewport(), border)
                .into();
        }
        if self.cfg.image_overlay == 10 {
            return crate::widgets::Canvas::new()
                .child_at(viewport(), body)
                .child_at(
                    Rect {
                        x: 5,
                        y: 2,
                        w: 0,
                        h: 3,
                    },
                    crate::widgets::PanView::new().clamp(false).child(
                        crate::widgets::Frame::new()
                            .width(crate::style::Length::Px(COLS))
                            .height(crate::style::Length::Px(ROWS))
                            .style(Style::default().bg(Color::Red)),
                    ),
                )
                .into();
        }
        if self.cfg.image_overlay != 0 {
            let overlay: Element = if self.cfg.image_overlay == 4 {
                crate::widgets::Canvas::new().into()
            } else if self.cfg.image_overlay == 5 {
                crate::widgets::Animated::new(crate::widgets::Canvas::new()).into()
            } else if self.cfg.image_overlay == 6 {
                crate::widgets::Animated::new(
                    Text::new("COVER").style(Style::default().bg(Color::Red)),
                )
                .into()
            } else if self.cfg.image_overlay == 7 {
                crate::widgets::Animated::new(crate::widgets::Canvas::new())
                    .opacity(0.5)
                    .into()
            } else if self.cfg.image_overlay == 9 {
                crate::widgets::Frame::new()
                    .border(false)
                    .padding(0)
                    .footer(crate::widgets::BorderLabels::new().center("COVER"))
                    .child(Text::new(""))
                    .into()
            } else if self.cfg.image_overlay == 11 {
                crate::widgets::Frame::new()
                    .border(false)
                    .padding(0)
                    .child(Text::new(""))
                    .into()
            } else if self.cfg.image_overlay != 2 {
                let style = if self.cfg.image_overlay == 1 {
                    Style::default().bg(Color::Red)
                } else {
                    Style::default().fg(Color::White)
                };
                Text::new("COVER").style(style).into()
            } else {
                crate::widgets::Frame::new()
                    .border(false)
                    .padding(0)
                    .child(Text::new("COVER"))
                    .style(Style::default().bg(Color::Red))
                    .into()
            };
            return crate::widgets::Canvas::new()
                .child_at(viewport(), body)
                .child_at(
                    Rect {
                        x: 5,
                        y: 2,
                        w: 5,
                        h: 3,
                    },
                    overlay,
                )
                .into();
        }
        if self.cfg.live_title {
            VStack::new()
                .child(Text::from_source(self.title.clone()))
                .child(body)
                .into()
        } else {
            body
        }
    }
}

fn viewport() -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: COLS,
        h: ROWS,
    }
}

fn frame_area() -> RatatuiRect {
    RatatuiRect::new(0, 0, COLS, ROWS)
}

fn terminal_ids(tree: &NodeTree) -> Vec<NodeId> {
    tree.iter()
        .filter(|node| matches!(node.kind, NodeKind::Terminal(_)))
        .map(|node| node.id)
        .collect()
}

/// A runner with a laid-out tree, a live terminal, and a retained frame from a full paint.
struct Harness {
    runner: AppRunner<Pane>,
    term: Terminal<HostBackend>,
    screen: Rc<RefCell<TerminalScreen>>,
    second: Option<Rc<RefCell<TerminalScreen>>>,
    retained: Buffer,
    title: TextSource,
}

fn build(cfg: PaneCfg, setup: &[u8]) -> Harness {
    let screen = Rc::new(RefCell::new(TerminalScreen::new(
        cfg.grid_rows(),
        COLS,
        500,
    )));
    screen.borrow_mut().process_bytes(setup);
    let second = cfg.second_terminal.then(|| {
        if cfg.shared_screen {
            return Rc::clone(&screen);
        }
        let screen = Rc::new(RefCell::new(TerminalScreen::new(
            cfg.grid_rows(),
            COLS,
            500,
        )));
        screen.borrow_mut().process_bytes(setup);
        screen.borrow_mut().set_scrollback(cfg.second_scrollback);
        screen
    });

    let title = TextSource::new([crate::style::Span::new("initial title")]);
    let component = || Pane {
        title: title.clone(),
        screen: Rc::clone(&screen),
        second: second.clone(),
        cfg,
    };

    let app = if cfg.inline {
        App::new().mouse(false).inline_ephemeral(ROWS)
    } else {
        App::new().mouse(false)
    };
    let mut runner = AppRunner::new(app, component(), ());
    runner.core = RuntimeCore::new_test_with_clock(
        component(),
        (),
        viewport(),
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
        if cfg.controlled_clock {
            crate::automation::ClockMode::Controlled
        } else {
            crate::automation::ClockMode::Realtime
        },
    )
    .into();
    runner.core.init();
    runner.core.render_element(viewport(), None, None, None);
    runner.core.tree.refresh_live_terminals();
    // The first reconcile fills the node straight from the live snapshot and marks it current
    // without taking damage, so the accumulator still holds everything the setup wrote - starting
    // with the `Full` a freshly created emulator reports. A real session spends one full paint on
    // that; here it just has to be out of the way before the case under test writes anything.
    screen.borrow_mut().take_damage();
    if let Some(second) = second.as_ref() {
        second.borrow_mut().take_damage();
    }

    let first = *terminal_ids(&runner.core.tree)
        .first()
        .expect("the pane mounts a terminal");
    if cfg.focused {
        runner.focus.focused = Some(first);
    }
    if cfg.focus_second {
        runner.focus.focused = Some(
            *terminal_ids(&runner.core.tree)
                .get(1)
                .expect("the pane mounts a second terminal"),
        );
    }
    if cfg.hovered {
        runner.mouse.hovered = Some(first);
        runner.mouse.last_mouse.set(Some((3, 1)));
    }
    if cfg.blink_hidden {
        runner.animation.blink_visible = false;
    }

    let mut term =
        Terminal::new(HostBackend::new(COLS, ROWS)).expect("test terminal should initialize");
    let (retained, _) = full_paint(&runner, &mut term);
    runner.last_frame_snapshot = Some(retained.clone());

    Harness {
        runner,
        term,
        screen,
        second,
        retained,
        title,
    }
}

/// The ordinary draw, verbatim: the production context, `render` over the whole frame.
///
/// Returns the caret it placed as well as the cells, because a partial repaint owes the host both.
fn full_paint(
    runner: &AppRunner<Pane>,
    term: &mut Terminal<HostBackend>,
) -> (Buffer, Option<Position>) {
    let cursor_position = StdCell::new(None);
    let buffer = runner.with_render_context(&cursor_position, |ctx| {
        term.draw(|f| render(f, ctx))
            .expect("a full paint should succeed")
            .buffer
            .clone()
    });
    (buffer, cursor_position.get())
}

fn changed_rows(before: &Buffer, after: &Buffer) -> Vec<u16> {
    (before.area.y..before.area.bottom())
        .filter(|&y| (before.area.x..before.area.right()).any(|x| before[(x, y)] != after[(x, y)]))
        .collect()
}

/// Is there anything left to send to bring the host to `expected`?
///
/// Cell-by-cell equality is the wrong question for a host. Ratatui never addresses the trailing
/// half of a double-width glyph - writing the glyph covers both columns - so that column keeps
/// whatever a simulated backend had there, and a real terminal keeps nothing. `diff` is the rule
/// the protocol actually follows, and an empty diff is the strongest true statement available:
/// a full paint starting from here would send nothing.
fn assert_host_matches(host: &Buffer, expected: &Buffer, what: &str) {
    let outstanding = expected.diff(host);
    assert!(
        outstanding.is_empty(),
        "{what}: {} cell(s) still differ from a full paint, first at {:?}",
        outstanding.len(),
        outstanding.first().map(|(x, y, _)| (*x, *y))
    );
}

fn assert_same(actual: &Buffer, expected: &Buffer, what: &str) {
    assert_eq!(actual.area, expected.area, "{what}: geometry differs");
    for y in expected.area.y..expected.area.bottom() {
        for x in expected.area.x..expected.area.right() {
            assert_eq!(
                actual[(x, y)],
                expected[(x, y)],
                "{what}: cell ({x}, {y}) differs from a full paint"
            );
        }
    }
}

fn plan_for(harness: &mut Harness) -> Result<RowDamagePlan, DamageRejection> {
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    let nodes = std::mem::take(&mut harness.runner.animation.spinner_damage);
    harness
        .runner
        .plan_live_damage(&refresh, &nodes, frame_area())
}

/// Apply `mutation`, patch the damaged rows, and hold the result to a full paint.
fn assert_patch_matches_full_paint(cfg: PaneCfg, setup: &[u8], mutation: &[u8]) {
    let mut harness = build(cfg, setup);
    harness.screen.borrow_mut().process_bytes(mutation);

    assert_harness_patch_matches_full_paint(&mut harness);
}

/// Patch the planned damage, hold it to a full paint, and return the plan that was patched.
fn assert_harness_patch_matches_full_paint(harness: &mut Harness) -> RowDamagePlan {
    let plan = plan_for(harness).expect("this case is meant to be eligible");

    let mut patched = harness.retained.clone();
    let cursor_position = StdCell::new(None);
    let runner = &harness.runner;
    let term = &mut harness.term;
    runner.with_render_context(&cursor_position, |ctx| {
        for band in plan.bands(frame_area().height) {
            patch_rows(term, ctx, &mut patched, frame_area(), band).expect("patching a band");
        }
        place_caret(term, cursor_position.get()).expect("placing the caret");
    });

    let mut oracle = Terminal::new(HostBackend::new(COLS, ROWS)).expect("oracle terminal");
    let (expected, expected_caret) = full_paint(runner, &mut oracle);

    // Damage has to be complete, not merely correct where it was reported. A row that moved and
    // was not planned is left stale by the patch and matches on both sides of the comparison.
    let moved = changed_rows(&harness.retained, &expected);
    for row in &moved {
        assert!(
            plan.rows.contains(row),
            "row {row} changed but the plan did not name it; planned {:?}, moved {moved:?}",
            plan.rows
        );
    }

    assert_same(&patched, &expected, "retained frame");
    assert_host_matches(term.backend().buffer(), &expected, "host");

    // The caret is the other half of what a draw owes the host, and the half no cell comparison
    // can see: every backend write leaves it one column past the run it sent, so a patch that
    // never places it leaves it wherever the last patched row happened to end.
    assert_eq!(
        cursor_position.get(),
        expected_caret,
        "the patch placed the caret somewhere a full paint did not"
    );
    if let Some(caret) = expected_caret {
        assert_eq!(
            term.backend_mut()
                .get_cursor_position()
                .expect("host caret"),
            caret,
            "the host caret is not where the patch asked for it"
        );
    }
    harness.retained = patched;
    harness.runner.last_frame_snapshot = Some(harness.retained.clone());
    plan
}

const SETUP: &[u8] = b"first line here\r\nsecond line here\r\nthird line here\r\n";

#[test]
fn spinner_rows_and_two_terminals_share_a_damage_frame_and_match_full_paints() {
    let cfg = PaneCfg {
        spinner: true,
        second_terminal: true,
        focused: true,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    for tick in (2..=24).step_by(2) {
        harness.runner.animation.spinner_frame = tick;
        assert!(harness.runner.update_spinner_frames());
        let mutation = format!("\x1b[2;1H\x1b[3{}mframe {tick}", tick % 7);
        harness
            .screen
            .borrow_mut()
            .process_bytes(mutation.as_bytes());
        harness
            .second
            .as_ref()
            .unwrap()
            .borrow_mut()
            .process_bytes(format!("\x1b[1;1H界 {tick}").as_bytes());
        assert_harness_patch_matches_full_paint(&mut harness);
    }
}

#[test]
fn spinner_only_damage_matches_full_paints_without_terminal_output() {
    let mut harness = build(
        PaneCfg {
            spinner: true,
            ..PaneCfg::new()
        },
        SETUP,
    );
    for tick in (2..=24).step_by(2) {
        harness.runner.animation.spinner_frame = tick;
        assert!(harness.runner.update_spinner_frames());
        assert_harness_patch_matches_full_paint(&mut harness);
    }
}

/// Damaged rows a few rows apart are painted in one walk, together with the unchanged rows between
/// them, and the result is still exactly a full paint.
#[test]
fn rows_banded_with_unchanged_rows_between_them_match_a_full_paint() {
    let mut harness = build(
        PaneCfg {
            spinner: true,
            ..PaneCfg::new()
        },
        SETUP,
    );
    for tick in (2..=12).step_by(2) {
        harness.runner.animation.spinner_frame = tick;
        assert!(harness.runner.update_spinner_frames());
        // The spinner is frame row 0 and the terminal starts on row 1, so this damages row 3.
        harness
            .screen
            .borrow_mut()
            .process_bytes(format!("\x1b[3;1Hthird {tick}").as_bytes());
        let plan = assert_harness_patch_matches_full_paint(&mut harness);
        // The cursor leaving its row damages that row too, so the band ends wherever it was.
        let end = plan.rows.last().expect("damaged rows") + 1;
        assert_eq!(plan.bands(ROWS), vec![0..end], "rows {:?}", plan.rows);
        assert!(
            !plan.rows.contains(&1) && !plan.rows.contains(&2),
            "rows 1 and 2 must be painted only as part of the band; rows {:?}",
            plan.rows
        );
    }
}

#[test]
fn bands_join_rows_at_most_the_band_gap_apart() {
    let bands = |rows: &[u16]| {
        RowDamagePlan {
            rows: rows.to_vec(),
        }
        .bands(80)
    };
    assert_eq!(bands(&[]), Vec::<std::ops::Range<u16>>::new());
    assert_eq!(bands(&[7]), vec![7..8]);
    // The sidebar shape that motivated bands: four spinners in one list, three in another.
    assert_eq!(bands(&[2, 3, 5, 7, 37, 38, 39]), vec![2..8, 37..40]);
    // Four unchanged rows between two damaged rows still join them; five split them.
    assert_eq!(bands(&[0, 5]), vec![0..6]);
    assert_eq!(bands(&[0, 6]), vec![0..1, 6..7]);
}

/// Joining is transitive, so damage every `BAND_GAP + 1` rows would chain into one band covering
/// most of the frame. Past half the frame, each damaged row is painted on its own again.
#[test]
fn evenly_spread_damage_does_not_band_across_most_of_the_frame() {
    let plan = RowDamagePlan {
        rows: (0..80).step_by(5).collect(),
    };
    assert_eq!(plan.rows.len(), 16);
    let bands = plan.bands(80);
    assert_eq!(
        bands,
        plan.rows
            .iter()
            .map(|&row| row..row + 1)
            .collect::<Vec<_>>()
    );
    // The same spacing over fewer rows stays within budget and still joins.
    let short = RowDamagePlan {
        rows: vec![0, 5, 10],
    };
    assert_eq!(short.bands(80), vec![0..11]);
    assert_eq!(short.bands(20), vec![0..1, 5..6, 10..11]);
}

#[test]
fn one_changed_row_matches_a_full_paint() {
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, b"\rX");
}

#[test]
fn several_writes_coalescing_into_one_row_match_a_full_paint() {
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, b"\rone\rtwo\rthree");
}

#[test]
fn several_damaged_rows_match_a_full_paint() {
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, b"\x1b[1;1Ha\x1b[3;1Hb\x1b[5;1Hc");
}

#[test]
fn the_first_and_last_visible_rows_match_a_full_paint() {
    let mutation = format!("\x1b[1;1HT\x1b[{ROWS};1HB");
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, mutation.as_bytes());
}

#[test]
fn a_viewport_taller_than_the_terminal_matches_a_full_paint() {
    let cfg = PaneCfg {
        grid_rows: 4,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn a_style_only_change_matches_a_full_paint() {
    // Same glyphs, different SGR: nothing about the text moves, only how it is painted.
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, b"\x1b[1;1H\x1b[31;1mfirst line here");
}

#[test]
fn a_wide_glyph_matches_a_full_paint() {
    assert_patch_matches_full_paint(PaneCfg::new(), SETUP, "\x1b[2;1H日本語テキスト".as_bytes());
}

#[test]
fn a_cursor_move_alone_matches_a_full_paint() {
    let cfg = PaneCfg {
        focused: true,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\x1b[2;5H");
}

#[test]
fn a_change_and_its_restore_match_a_full_paint() {
    let mut harness = build(PaneCfg::new(), SETUP);
    let original = harness.retained.clone();

    // Overwrite the first five columns, then put them back exactly.
    for mutation in [b"\x1b[1;1HXXXXX".as_slice(), b"\x1b[1;1Hfirst".as_slice()] {
        harness.screen.borrow_mut().process_bytes(mutation);
        let plan = plan_for(&mut harness).expect("both writes are eligible");
        let mut patched = harness.retained.clone();
        let cursor_position = StdCell::new(None);
        {
            let runner = &harness.runner;
            let term = &mut harness.term;
            runner.with_render_context(&cursor_position, |ctx| {
                for &row in &plan.rows {
                    patch_rows(term, ctx, &mut patched, frame_area(), row..row + 1)
                        .expect("patching a row");
                }
            });
        }
        harness.retained = patched;
        harness.runner.last_frame_snapshot = Some(harness.retained.clone());
    }

    // Back where it started, and the host has to agree - this is the case a stale diff baseline
    // gets wrong, because the second write restores exactly what the baseline still remembers.
    assert_same(&harness.retained, &original, "retained frame after restore");
    assert_host_matches(
        harness.term.backend().buffer(),
        &original,
        "host after restore",
    );
}

#[test]
fn a_focused_terminal_matches_a_full_paint() {
    let cfg = PaneCfg {
        focused: true,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn a_themed_terminal_matches_a_full_paint() {
    let cfg = PaneCfg {
        themed: true,
        focused: true,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn an_active_selection_matches_a_full_paint() {
    let cfg = PaneCfg {
        themed: true,
        selection: true,
        ..PaneCfg::new()
    };
    // Write into the selected row, so the patch has to reproduce selection styling, not skip it.
    assert_patch_matches_full_paint(cfg, SETUP, b"\x1b[2;1Hrewritten line");
}

#[test]
fn a_scrollbar_matches_a_full_paint() {
    let cfg = PaneCfg {
        scrollbar: true,
        ..PaneCfg::new()
    };
    // Enough output to fill scrollback, so the scrollbar is actually drawn.
    let mut setup = Vec::new();
    for line in 0..(ROWS as usize * 3) {
        setup.extend_from_slice(format!("line {line}\r\n").as_bytes());
    }
    assert_patch_matches_full_paint(cfg, &setup, b"\rX");
}

#[test]
fn a_hovered_terminal_matches_a_full_paint() {
    let cfg = PaneCfg {
        themed: true,
        hovered: true,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn a_hidden_blink_matches_a_full_paint() {
    let cfg = PaneCfg {
        focused: true,
        blink_hidden: true,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn a_caret_outside_the_damaged_rows_is_still_placed() {
    // The focused terminal is the one that did *not* move, so no damaged row carries its caret.
    // Nothing else would ever place it, and the host would keep the caret one column past the end
    // of the row the other terminal just patched.
    let cfg = PaneCfg {
        second_terminal: true,
        focus_second: true,
        grid_rows: ROWS / 2,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\rX");
}

#[test]
fn the_caret_row_is_planned_even_when_the_terminal_did_not_damage_it() {
    let cfg = PaneCfg {
        second_terminal: true,
        focus_second: true,
        grid_rows: ROWS / 2,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");

    let caret = harness
        .runner
        .incremental_cursor_position()
        .expect("the focused terminal wants a caret");
    let plan = plan_for(&mut harness).expect("eligible");
    assert!(
        plan.rows.contains(&caret.y),
        "caret row {} missing from planned rows {:?}",
        caret.y,
        plan.rows
    );
    assert!(
        plan.rows.windows(2).all(|pair| pair[0] < pair[1]),
        "planned rows should stay ascending and unique: {:?}",
        plan.rows
    );
}

#[test]
fn padding_offsets_the_patched_row_and_matches_a_full_paint() {
    let cfg = PaneCfg {
        padding: 1,
        ..PaneCfg::new()
    };
    assert_patch_matches_full_paint(cfg, SETUP, b"\x1b[2;1Hrewritten");
}

// ─── Rejections ──────────────────────────────────────────────────────────────

#[test]
fn a_quiet_frame_is_rejected_as_nothing_moved() {
    let mut harness = build(PaneCfg::new(), SETUP);
    assert_eq!(plan_for(&mut harness), Err(DamageRejection::NothingMoved));
}

#[test]
fn a_screen_swap_is_rejected_as_full_damage() {
    let mut harness = build(PaneCfg::new(), SETUP);
    // The alternate screen replaces the whole grid, which the emulator reports as full damage.
    harness.screen.borrow_mut().process_bytes(b"\x1b[?1049h");
    assert_eq!(plan_for(&mut harness), Err(DamageRejection::FullDamage));
}

#[test]
fn a_bordered_terminal_is_rejected() {
    let cfg = PaneCfg {
        border: true,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::BorderedTerminal)
    );
}

#[test]
fn a_scrolled_back_terminal_is_rejected() {
    let cfg = PaneCfg::new();
    let mut setup = Vec::new();
    for line in 0..(ROWS as usize * 3) {
        setup.extend_from_slice(format!("line {line}\r\n").as_bytes());
    }
    let mut harness = build(cfg, &setup);

    harness.screen.borrow_mut().process_bytes(b"\rX");
    // After the refresh, because the refresh reapplies the live snapshot and would overwrite it.
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    let id = *terminal_ids(&harness.runner.core.tree)
        .first()
        .expect("a terminal node");
    if let NodeKind::Terminal(node) = &mut harness.runner.core.tree.node_mut(id).kind {
        node.scrollback_offset = 3;
    }
    assert_eq!(
        harness.runner.plan_live_damage(&refresh, &[], frame_area()),
        Err(DamageRejection::ScrolledBack)
    );
}

/// An app-placed caret (a copy-mode cursor) shows through scrollback and a selection, so output in
/// another terminal must still repaint its row, or the caret drops out until the next full draw.
#[test]
fn an_app_placed_caret_survives_another_terminals_damage() {
    let mut setup = Vec::new();
    for line in 0..(ROWS as usize * 3) {
        setup.extend_from_slice(format!("line {line}\r\n").as_bytes());
    }
    for (scrolled_back, selection) in [(true, false), (false, true), (true, true)] {
        let cfg = PaneCfg {
            second_terminal: true,
            focus_second: true,
            grid_rows: ROWS / 2,
            cursor_override: Some((2, 3)),
            second_scrollback: if scrolled_back { 3 } else { 0 },
            selection,
            ..PaneCfg::new()
        };
        let mut harness = build(cfg, &setup);
        harness.screen.borrow_mut().process_bytes(b"\rX");

        let caret = harness.runner.incremental_cursor_position();
        assert!(
            caret.is_some(),
            "the app-placed caret is wanted (scrolled back: {scrolled_back}, selection: \
             {selection})"
        );
        // The incremental scroll and inline paths ask the same question of the focused widget.
        assert!(
            harness.runner.focused_node_requests_cursor()
                && harness.runner.focused_node_has_cursor_anchor(),
            "every draw path wants the app-placed caret (scrolled back: {scrolled_back}, \
             selection: {selection})"
        );
        let plan = assert_harness_patch_matches_full_paint(&mut harness);
        assert!(
            plan.rows.contains(&caret.expect("caret").y),
            "the caret row is repainted (scrolled back: {scrolled_back}, selection: {selection})"
        );
    }
}

#[test]
fn two_moved_terminals_match_a_full_paint() {
    let cfg = PaneCfg {
        second_terminal: true,
        grid_rows: ROWS / 2,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");
    harness
        .second
        .as_ref()
        .expect("a second screen")
        .borrow_mut()
        .process_bytes(b"\rY");
    assert_harness_patch_matches_full_paint(&mut harness);
}

#[test]
fn shared_screen_updates_fall_back_and_match_a_full_paint() {
    let cfg = PaneCfg {
        second_terminal: true,
        shared_screen: true,
        grid_rows: ROWS / 2,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    assert!(Rc::ptr_eq(
        &harness.screen,
        harness.second.as_ref().unwrap()
    ));
    harness
        .screen
        .borrow_mut()
        .process_bytes(b"\x1b[1;1HX\x1b[3;1HY");
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    assert_eq!(refresh.damage.len(), 2);
    assert!(
        refresh
            .damage
            .iter()
            .any(|(_, damage)| matches!(damage, crate::widgets::internal::TerminalDamage::None))
    );
    assert_eq!(
        harness.runner.plan_live_damage(&refresh, &[], frame_area()),
        Err(DamageRejection::MissingDamage)
    );

    // The production planner's None result selects the ordinary draw of the refreshed tree.
    assert!(
        harness
            .runner
            .prepare_terminal_damage_plan(&refresh, frame_area())
            .is_none()
    );
    let mut oracle = Terminal::new(HostBackend::new(COLS, ROWS)).unwrap();
    let (expected, _) = full_paint(&harness.runner, &mut oracle);
    let moved = changed_rows(&harness.retained, &expected);
    assert!(moved.iter().any(|row| *row < ROWS / 2));
    assert!(moved.iter().any(|row| *row >= ROWS / 2));
    let (painted, _) = full_paint(&harness.runner, &mut harness.term);
    assert_same(&painted, &expected, "fallback frame");
    assert_host_matches(harness.term.backend().buffer(), &expected, "fallback host");
}

#[test]
fn a_live_title_and_terminal_update_match_a_full_paint() {
    for title in [
        "a much longer title than the original with overflow",
        "短 e\u{301}",
        "",
    ] {
        let cfg = PaneCfg {
            live_title: true,
            focused: true,
            grid_rows: ROWS - 1,
            ..PaneCfg::new()
        };
        let mut harness = build(cfg, SETUP);
        harness.title.set([crate::style::Span::new(title)]);
        harness.screen.borrow_mut().process_bytes(b"\rX");
        assert_harness_patch_matches_full_paint(&mut harness);
    }
}

#[test]
fn a_live_title_without_terminal_output_matches_a_full_paint() {
    let cfg = PaneCfg {
        live_title: true,
        focused: true,
        grid_rows: ROWS - 1,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness
        .title
        .set([crate::style::Span::new("updated title")]);
    assert_harness_patch_matches_full_paint(&mut harness);
}

#[test]
fn dense_row_damage_uses_an_ordinary_paint() {
    let mut harness = build(PaneCfg::new(), SETUP);
    let updates: String = (1..=ROWS).map(|row| format!("\x1b[{row};1HX")).collect();
    harness
        .screen
        .borrow_mut()
        .process_bytes(updates.as_bytes());
    assert_eq!(plan_for(&mut harness), Err(DamageRejection::DenseDamage));
}

#[test]
fn a_retained_frame_of_the_wrong_size_is_rejected() {
    let mut harness = build(PaneCfg::new(), SETUP);
    harness.runner.last_frame_snapshot =
        Some(Buffer::empty(RatatuiRect::new(0, 0, COLS, ROWS - 1)));
    harness.screen.borrow_mut().process_bytes(b"\rX");
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::NoMatchingRetainedFrame)
    );
}

#[test]
fn no_retained_frame_at_all_is_rejected() {
    let mut harness = build(PaneCfg::new(), SETUP);
    harness.runner.last_frame_snapshot = None;
    harness.screen.borrow_mut().process_bytes(b"\rX");
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::NoMatchingRetainedFrame)
    );
}

#[test]
fn an_inline_surface_is_rejected_as_a_composite_surface() {
    let cfg = PaneCfg {
        inline: true,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::CompositeSurface)
    );
}

/// The frame row the centred notice sits on.
fn notice_row(harness: &Harness) -> u16 {
    let tree = &harness.runner.core.tree;
    let root = tree.overlay_roots().first().expect("the notice is mounted");
    let rect = tree.node(root.id).rect;
    assert_eq!(rect.h, 1, "the notice is one row");
    u16::try_from(rect.y).expect("the notice is on screen")
}

/// A notice over one row leaves every other row as the base tree paints it, so damage elsewhere
/// is repainted by rows while it stays on screen.
#[test]
fn rows_beside_a_notice_match_a_full_paint() {
    let cfg = PaneCfg {
        toast: 1,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    let notice = notice_row(&harness);
    assert!(
        notice > 3,
        "the notice must sit below the rows this test damages"
    );
    for tick in 0..4 {
        harness
            .screen
            .borrow_mut()
            .process_bytes(format!("\x1b[1;1Hline {tick}").as_bytes());
        let plan = assert_harness_patch_matches_full_paint(&mut harness);
        assert!(
            !plan.rows.contains(&notice),
            "rows {:?} include the notice",
            plan.rows
        );
    }
}

#[test]
fn a_row_under_a_notice_is_rejected_as_a_composite_surface() {
    let cfg = PaneCfg {
        toast: 1,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    let notice = notice_row(&harness);
    harness
        .screen
        .borrow_mut()
        .process_bytes(format!("\x1b[{};1Hunder the notice", notice + 1).as_bytes());
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::CompositeSurface)
    );
}

/// Pin the moved notice's `Animated` wrapper `rows` above where layout put it, as a frame partway
/// through a move would, and retake the retained frame with it there. Returns the resting row.
fn move_notice_up(harness: &mut Harness, rows: u16) -> u16 {
    let rest = notice_row(harness);
    let tree = &mut harness.runner.core.tree;
    let root = tree
        .overlay_roots()
        .first()
        .expect("the notice is mounted")
        .id;
    let NodeKind::Animated(animated) = &mut tree.node_mut(root).kind else {
        panic!("the moved notice is wrapped in Animated");
    };
    animated.current_y_offset = -f32::from(rows);
    let (retained, _) = full_paint(&harness.runner, &mut harness.term);
    harness.runner.last_frame_snapshot = Some(retained.clone());
    harness.retained = retained;
    rest
}

/// An overlay's extent follows its `Animated` move, not its layout: the row it left is repainted
/// beside it, matching a full paint, and the row it moved onto is not.
#[test]
fn a_moved_notice_covers_the_row_it_moved_to_not_the_one_it_left() {
    let cfg = PaneCfg {
        toast: 3,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    let rest = move_notice_up(&mut harness, 3);
    let moved = rest - 3;
    assert!(moved > 0, "the notice must stay on screen");
    // The terminal starts on frame row 0, so terminal row `n + 1` is frame row `n`.
    harness
        .screen
        .borrow_mut()
        .process_bytes(format!("\x1b[{};1Hwhere it was", rest + 1).as_bytes());
    let plan = assert_harness_patch_matches_full_paint(&mut harness);
    assert!(plan.rows.contains(&rest), "rows {:?}", plan.rows);

    let mut harness = build(cfg, SETUP);
    move_notice_up(&mut harness, 3);
    harness
        .screen
        .borrow_mut()
        .process_bytes(format!("\x1b[{};1Hwhere it is", moved + 1).as_bytes());
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::CompositeSurface)
    );
}

#[test]
fn a_notice_with_a_backdrop_is_rejected_as_a_composite_surface() {
    let cfg = PaneCfg {
        toast: 2,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);
    harness.screen.borrow_mut().process_bytes(b"\x1b[1;1HX");
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::CompositeSurface)
    );
}

// ─── Ratatui bookkeeping ─────────────────────────────────────────────────────

#[test]
fn a_patched_row_survives_the_next_ordinary_draw() {
    // Ratatui's previous buffer never saw the patch. If the next full draw diffs against it
    // alone, a cell whose new content matches what Ratatui last drew there is skipped, and the
    // patch stays on screen. This is the whole reason `host_patched_rows` exists.
    let mut harness = build(PaneCfg::new(), SETUP);

    harness.screen.borrow_mut().process_bytes(b"\rXXXXX");
    let plan = plan_for(&mut harness).expect("eligible");
    let mut patched = harness.retained.clone();
    let cursor_position = StdCell::new(None);
    {
        let runner = &harness.runner;
        let term = &mut harness.term;
        runner.with_render_context(&cursor_position, |ctx| {
            for &row in &plan.rows {
                patch_rows(term, ctx, &mut patched, frame_area(), row..row + 1)
                    .expect("patching a row");
            }
        });
    }
    let host_rows: Vec<Buffer> = plan
        .rows
        .iter()
        .map(|&row| {
            let mut buffer = Buffer::empty(RatatuiRect::new(0, row, COLS, 1));
            for x in 0..COLS {
                buffer[(x, row)] = patched[(x, row)].clone();
            }
            buffer
        })
        .collect();
    harness.retained = patched;

    // Restore the original text, so the next full frame equals what Ratatui's previous buffer
    // still holds and its own diff has nothing to say about the row.
    harness.screen.borrow_mut().process_bytes(b"\rfirst");
    harness.runner.core.tree.refresh_live_terminals();

    let expected = {
        let runner = &harness.runner;
        let term = &mut harness.term;
        let cursor_position = StdCell::new(None);
        let drawn = runner.with_render_context(&cursor_position, |ctx| {
            term.draw(|f| render(f, ctx))
                .expect("full paint")
                .buffer
                .clone()
        });
        super::row_damage::resync_host_patched_rows(
            term,
            &host_rows,
            &drawn,
            cursor_position.get(),
        )
        .expect("resync should succeed");
        drawn
    };

    assert_host_matches(harness.term.backend().buffer(), &expected, "host");
}

#[test]
fn the_resync_puts_the_caret_back_after_re_sending_a_row() {
    // The draw places the caret and the resync runs after it, so the cells it re-sends walk the
    // caret to the end of the last row it touched. In a real session that is what a pane's border
    // transition looks like: every cell in a patched row differs, the whole row is re-sent, and
    // the caret sits at the right edge until the animation stops producing ordinary draws.
    let cfg = PaneCfg {
        focused: true,
        ..PaneCfg::new()
    };
    let mut harness = build(cfg, SETUP);

    harness.screen.borrow_mut().process_bytes(b"\rXXXXX");
    let plan = plan_for(&mut harness).expect("eligible");
    let mut patched = harness.retained.clone();
    {
        let cursor_position = StdCell::new(None);
        let runner = &harness.runner;
        let term = &mut harness.term;
        runner.with_render_context(&cursor_position, |ctx| {
            for &row in &plan.rows {
                patch_rows(term, ctx, &mut patched, frame_area(), row..row + 1)
                    .expect("patching a row");
            }
            place_caret(term, cursor_position.get()).expect("placing the caret");
        });
    }
    let host_rows: Vec<Buffer> = plan
        .rows
        .iter()
        .map(|&row| {
            let mut buffer = Buffer::empty(RatatuiRect::new(0, row, COLS, 1));
            for x in 0..COLS {
                buffer[(x, row)] = patched[(x, row)].clone();
            }
            buffer
        })
        .collect();
    harness.retained = patched;

    // Rewrite the row long, then send the cursor home. The resync now has cells to re-send well to
    // the right of where the caret belongs, which is the arrangement that tells the two apart: a
    // re-sent run that happens to end on the caret proves nothing.
    harness
        .screen
        .borrow_mut()
        .process_bytes(b"\r\x1b[KZZZZZZZZZZZZZZZZZZZZ\x1b[1;1H");
    harness.runner.core.tree.refresh_live_terminals();

    let caret = {
        let runner = &harness.runner;
        let term = &mut harness.term;
        let cursor_position = StdCell::new(None);
        let drawn = runner.with_render_context(&cursor_position, |ctx| {
            term.draw(|f| render(f, ctx))
                .expect("full paint")
                .buffer
                .clone()
        });
        super::row_damage::resync_host_patched_rows(
            term,
            &host_rows,
            &drawn,
            cursor_position.get(),
        )
        .expect("resync should succeed");
        cursor_position
            .get()
            .expect("the focused terminal wants a caret")
    };

    assert_eq!(
        harness
            .term
            .backend_mut()
            .get_cursor_position()
            .expect("host caret"),
        caret,
        "the resync left the caret where its last re-sent cell ended"
    );
}

#[test]
fn planning_the_same_refresh_twice_agrees() {
    // The planner is pure with respect to the runner: nothing it inspects is consumed by
    // inspecting it, which is what lets a caller decide and then fall back without a second
    // refresh.
    let mut harness = build(PaneCfg::new(), SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    let first = harness.runner.plan_live_damage(&refresh, &[], frame_area());
    let second = harness.runner.plan_live_damage(&refresh, &[], frame_area());
    assert_eq!(
        first.map(|plan| plan.rows),
        second.map(|plan| plan.rows),
        "planning twice should agree"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn an_unchanged_image_pane_survives_an_unrelated_terminal_update() {
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(
        PaneCfg {
            second_terminal: true,
            grid_rows: ROWS / 2,
            ..PaneCfg::new()
        },
        SETUP,
    );
    harness
        .screen
        .borrow_mut()
        .process_bytes(b"\x1b_Ga=T,f=32,s=1,v=1,i=919;/////w==\x1b\\");
    harness.runner.core.tree.refresh_live_terminals_detailed();
    harness.screen.borrow_mut().take_damage();
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let cursor = StdCell::new(None);
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let initial = terminal.backend().test_output().len();
    harness
        .second
        .as_ref()
        .unwrap()
        .borrow_mut()
        .process_bytes(b"\rX");
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    let plan = harness.runner.plan_live_damage(&refresh, &[], frame_area());
    match &plan {
        Ok(plan) => harness.runner.draw_row_damage(&mut terminal, plan).unwrap(),
        Err(_) => {
            harness
                .runner
                .with_render_context(&cursor, |ctx| terminal.draw(|f| render(f, ctx)).unwrap());
        }
    }
    let output = terminal.backend().test_output();
    let update = String::from_utf8_lossy(&output[initial..]);
    assert!(update.contains('X'));
    assert!(
        !update.contains("\x1b_G"),
        "unchanged native graphics must emit no commands: {update:?}"
    );
    assert!(
        plan.is_ok(),
        "unchanged native images must permit partial cell paints: {plan:?}"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn incremental_host_scroll_with_terminal_images_requires_a_full_paint() {
    let mut harness = build(PaneCfg::new(), SETUP);
    let previous = crate::app::runner::ScrollFrameSnapshot {
        node_id: harness.runner.core.tree.root,
        scroll_offset: 0,
        content_height: 30,
        content_hash: Some(7),
        viewport_height: ROWS,
        scroll_rows: 0..ROWS,
        scrollbar_rect: None,
        show_scroll_indicators: false,
    };
    let current = crate::app::runner::ScrollFrameSnapshot {
        scroll_offset: 1,
        ..previous.clone()
    };
    harness.runner.last_scroll_frames = vec![previous];
    harness.runner.last_frame_snapshot = Some(Buffer::empty(frame_area()));
    let plan = |harness: &Harness| {
        harness.runner.prepare_incremental_scroll_plan(
            super::DrawMode::LayoutOnly,
            frame_area(),
            std::slice::from_ref(&current),
        )
    };
    assert!(
        plan(&harness).is_some(),
        "text-only scrolls retain the fast path"
    );
    harness
        .screen
        .borrow_mut()
        .process_bytes(b"\x1b_Ga=T,f=32,s=1,v=1,i=919;/////w==\x1b\\");
    harness.runner.core.tree.refresh_live_terminals_detailed();
    assert!(
        plan(&harness).is_none(),
        "host scroll must not move retained image placements behind the scene reconciler"
    );
}

/// The scroll fast path ends a cursorless frame with the hidden cursor on a fixed cell, not after
/// the last cell it sent. Kitty's cursor trail follows even a hidden cursor, so a cursor left
/// where each scroll's writes ended would trail across the screen on every scroll.
#[cfg(feature = "terminal-images")]
#[test]
fn a_cursorless_incremental_scroll_parks_the_hidden_cursor() {
    // The child hides its cursor, so no caret is requested and none was ever shown.
    let mut harness = build(PaneCfg::new(), b"\x1b[?25l");
    let previous = crate::app::runner::ScrollFrameSnapshot {
        node_id: harness.runner.core.tree.root,
        scroll_offset: 0,
        content_height: 30,
        content_hash: Some(7),
        viewport_height: ROWS,
        scroll_rows: 0..ROWS,
        scrollbar_rect: None,
        show_scroll_indicators: false,
    };
    let current = crate::app::runner::ScrollFrameSnapshot {
        scroll_offset: 1,
        ..previous.clone()
    };
    harness.runner.last_scroll_frames = vec![previous];
    let plan = harness
        .runner
        .prepare_incremental_scroll_plan(
            super::DrawMode::LayoutOnly,
            frame_area(),
            std::slice::from_ref(&current),
        )
        .expect("a text-only scroll takes the fast path");

    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let mut last = Buffer::empty(frame_area());
    let mut diff = Buffer::empty(frame_area());
    let cursor = StdCell::new(None);
    harness.runner.with_render_context(&cursor, |ctx| {
        AppRunner::<Pane>::draw_incremental_scroll(
            &mut terminal,
            ctx,
            &plan,
            &mut last,
            &mut diff,
            false,
        )
        .unwrap();
    });

    let output = terminal.backend().test_output();
    let sent = String::from_utf8_lossy(&output);
    // The exposed bottom row is the last thing sent, which leaves the cursor at the far corner.
    assert!(
        sent.contains(&format!("\x1b[{ROWS};1H")),
        "the scroll repainted its exposed row: {sent:?}"
    );
    assert!(
        output.ends_with(b"\x1b[?25l\x1b[1;1H"),
        "the hidden cursor parks on the origin: {sent:?}"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn composed_terminal_images_reach_the_host_in_partial_frames() {
    use base64::Engine as _;

    // Unmeasured host cells select composition even on a Kitty host.
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), false);
    let mut harness = build(PaneCfg::new(), b"\x1b[?25l\x1b[H");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let upload = |color| {
        let payload =
            base64::engine::general_purpose::STANDARD.encode([color, 0, 0].repeat(20 * 60));
        harness.screen.borrow_mut().process_bytes(
            format!("\x1b[H\x1b_Ga=T,i=7,p=9,f=24,s=20,v=60,z=-1,q=2,C=1;{payload}\x1b\\")
                .as_bytes(),
        );
    };
    upload(1u8);
    harness.runner.core.tree.refresh_live_terminals_detailed();
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let output = terminal.backend().test_output();
    assert!(String::from_utf8_lossy(&output).contains("a=p,"));
    let initial = output.len();
    let initial_commands = String::from_utf8_lossy(&output);
    let placements: Vec<_> = initial_commands
        .split("\x1b_Ga=p,")
        .skip(1)
        .map(|command| {
            let header = command.split(';').next().unwrap();
            let field = |key: &str| {
                header
                    .split(',')
                    .find_map(|field| field.strip_prefix(key))
                    .unwrap()
                    .to_owned()
            };
            (field("i="), field("p="), field("y="))
        })
        .collect();
    assert_eq!(placements.len(), 3);

    upload(2u8);
    harness.runner.core.tree.refresh_live_terminals_detailed();
    let cursor = StdCell::new(None);
    let _plans = terminal.backend().native_plan_scope();
    let mut snapshot = harness.runner.last_frame_snapshot.clone().unwrap();
    harness.runner.with_render_context(&cursor, |ctx| {
        patch_rows(&mut terminal, ctx, &mut snapshot, frame_area(), 1..2).unwrap();
    });
    Backend::flush(terminal.backend_mut()).unwrap();
    let output = terminal.backend().test_output();
    let update = String::from_utf8_lossy(&output[initial..]);
    for (image, placement, y) in &placements {
        if y != "20" {
            assert!(
                !update.contains(&format!("d=i,i={image},p={placement};")),
                "partial repaint deleted an untouched image row: {update:?}"
            );
        }
    }
    assert!(
        update.contains("a=p,"),
        "composed image row must reach the host: {update:?}"
    );
    assert!(
        update.contains("c="),
        "replacement must use the composed path: {update:?}"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn per_cell_crops_of_one_source_share_a_single_upload() {
    use base64::Engine as _;

    // Unmeasured host cells select composition even on a Kitty host.
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((14, 32), false);
    let mut harness = build(PaneCfg::new(), b"\x1b[?25l\x1b[H");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(14, 32));
    // Yazi's per-cell encoding: 137 pixels over 10 cells gives crops of 13 and 14 pixels.
    let (cols, rows, width, height) = (10u32, 4u32, 137u32, 128u32);
    let payload = base64::engine::general_purpose::STANDARD
        .encode([100, 150, 200].repeat((width * height) as usize));
    let mut stream = format!("\x1b_Ga=t,f=24,s={width},v={height},i=7,q=2;{payload}\x1b\\");
    for row in 0..rows {
        for col in 0..cols {
            let x = col * width / cols;
            let end = (col + 1) * width / cols;
            stream.push_str(&format!(
                "\x1b[{};{}H\x1b_Ga=p,i=7,p={},x={x},y={},w={},h=32,c=1,r=1,z=-1,C=1,q=2;\x1b\\",
                row + 1,
                col + 1,
                row * cols + col + 1,
                row * 32,
                end - x,
            ));
        }
    }
    harness.screen.borrow_mut().process_bytes(stream.as_bytes());
    harness.runner.core.tree.refresh_live_terminals_detailed();
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let output = terminal.backend().test_output();
    let initial = String::from_utf8_lossy(&output);
    let uploads = initial.matches("a=T,").count() + initial.matches(",a=t,").count();
    assert_eq!(uploads, 1, "every crop must share one upload: {initial:?}");
    let placements: Vec<_> = initial.match_indices("\x1b_Ga=p,").collect();
    assert_eq!(placements.len(), (cols * rows) as usize);
    let ids: std::collections::HashSet<_> = placements
        .iter()
        .map(|(start, _)| {
            initial[*start..]
                .split("i=")
                .nth(1)
                .and_then(|rest| rest.split(',').next())
                .unwrap()
        })
        .collect();
    assert_eq!(ids.len(), 1, "crops must place the shared image: {ids:?}");
    assert!(initial.contains("w=13,h=32,c=1,r=1"));
    assert!(initial.contains("w=14,h=32,c=1,r=1"));
    let before = output.len();

    harness.runner.render_paint_only(&mut terminal).unwrap();
    let output = terminal.backend().test_output();
    assert!(
        !String::from_utf8_lossy(&output[before..]).contains("\x1b_G"),
        "an unchanged per-cell image must stay quiet"
    );
    let before = output.len();

    // A new producer frame re-sends the source, which removes every placement of it on the host.
    let payload = base64::engine::general_purpose::STANDARD
        .encode([30, 60, 90].repeat((width * height) as usize));
    let frame = stream.replacen(
        stream.split("\x1b\\").next().unwrap(),
        &format!("\x1b_Ga=t,f=24,s={width},v={height},i=7,q=2;{payload}"),
        1,
    );
    harness.screen.borrow_mut().process_bytes(frame.as_bytes());
    harness.runner.core.tree.refresh_live_terminals_detailed();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let output = terminal.backend().test_output();
    let update = String::from_utf8_lossy(&output[before..]);
    assert_eq!(
        update.matches("a=T,").count() + update.matches(",a=t,").count(),
        1,
        "one upload per producer frame: {update:?}"
    );
    assert_eq!(
        update.matches("\x1b_Ga=p,").count(),
        (cols * rows) as usize,
        "the re-upload removed every crop, so every crop is placed again"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn a_lone_crop_uploads_its_whole_source_only_when_the_source_is_small() {
    use base64::Engine as _;

    let upload_size = |source: (u32, u32), crop: (u32, u32)| {
        let _host =
            crate::backend::ratatui_backend::image_support::test_kitty_host((14, 32), false);
        let mut harness = build(PaneCfg::new(), b"\x1b[?25l\x1b[H");
        harness
            .screen
            .borrow_mut()
            .set_cell_size(crate::widgets::TerminalCellSize::new(14, 32));
        let payload = base64::engine::general_purpose::STANDARD
            .encode([100, 150, 200].repeat((source.0 * source.1) as usize));
        harness.screen.borrow_mut().process_bytes(
            format!(
                "\x1b_Ga=t,f=24,s={},v={},i=7,q=2;{payload}\x1b\\\x1b[H\x1b_Ga=p,i=7,p=1,x=0,y=0,w={},h={},c=1,r=1,z=-1,C=1,q=2;\x1b\\",
                source.0, source.1, crop.0, crop.1,
            )
            .as_bytes(),
        );
        harness.runner.core.tree.refresh_live_terminals_detailed();
        let mut terminal =
            crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
        harness.runner.render_paint_only(&mut terminal).unwrap();
        let output = String::from_utf8_lossy(&terminal.backend().test_output()).into_owned();
        let header = output
            .split("\x1b_G")
            .find(|command| command.contains("a=T,") || command.contains(",a=t,"))
            .unwrap_or_else(|| panic!("no upload: {output:?}"))
            .to_owned();
        let key = |name: &str| -> u32 {
            header
                .split([',', ';'])
                .find_map(|pair| pair.strip_prefix(name))
                .unwrap()
                .parse()
                .unwrap()
        };
        (key("s="), key("v="))
    };
    // A small crop of a large image encodes just the crop.
    assert_eq!(upload_size((400, 320), (14, 32)), (14, 32));
    // A crop that is most of its source places from the whole source.
    assert_eq!(upload_size((28, 32), (14, 32)), (28, 32));
}

#[cfg(feature = "terminal-images")]
#[test]
fn ordinary_overlays_cover_image_pixels_without_covering_the_anchor() {
    for mode in 0..=14 {
        for stacked in [false, true] {
            assert_ordinary_overlay_image_coverage(mode, stacked);
        }
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn crossfading_borders_match_visible_cells_at_zero_low_and_high_opacity() {
    for mode in [15, 16, 17, 18, 21] {
        for stacked in [false, true] {
            assert_ordinary_overlay_image_coverage(mode, stacked);
        }
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn portal_border_declared_before_main_terminal_still_clips_its_native_picture() {
    for stacked in [false, true] {
        assert_ordinary_overlay_image_coverage(19, stacked);
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn main_frame_declared_after_portal_terminal_does_not_clip_its_native_picture() {
    for stacked in [false, true] {
        assert_ordinary_overlay_image_coverage(20, stacked);
    }
}

#[cfg(feature = "terminal-images")]
fn assert_ordinary_overlay_image_coverage(image_overlay: u8, stacked: bool) {
    use base64::Engine as _;
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(
        PaneCfg {
            image_overlay,
            ..PaneCfg::new()
        },
        b"\x1b[?25l\x1b[H",
    );
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let payload =
        base64::engine::general_purpose::STANDARD.encode([0, 255, 0, 255].repeat(120 * 100));
    harness
        .screen
        .borrow_mut()
        .process_bytes(format!("\x1b_Ga=T,f=32,s=120,v=100,i=920,z=1;{payload}\x1b\\").as_bytes());
    if stacked {
        harness
            .screen
            .borrow_mut()
            .process_bytes(b"\x1b[H\x1b_Ga=p,i=920,p=2,z=2;\x1b\\");
    }
    harness.runner.core.tree.refresh_live_terminals_detailed();
    if matches!(image_overlay, 19 | 20) {
        let tree = &harness.runner.core.tree;
        let owner = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Terminal(_)))
            .unwrap();
        let frame = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Frame(_)))
            .unwrap();
        let owner_rank = tree.iter().position(|node| node.id == owner.id).unwrap();
        let frame_rank = tree.iter().position(|node| node.id == frame.id).unwrap();
        assert_eq!(
            frame_rank < owner_rank,
            image_overlay == 19,
            "the declarative order must oppose the real portal paint order"
        );
        let area = ratatui::layout::Rect::new(0, 0, COLS, ROWS);
        assert!(
            crate::backend::ratatui_backend::render::terminal_native_layers_supported(
                tree, owner.id, area
            )
        );
        let holes = crate::backend::ratatui_backend::render::terminal_image_layer_occlusions(
            tree, owner.id, area,
        );
        let expected = if image_overlay == 19 {
            vec![
                ratatui::layout::Rect::new(0, 0, COLS, 1),
                ratatui::layout::Rect::new(0, ROWS - 1, COLS, 1),
                ratatui::layout::Rect::new(0, 1, 1, ROWS - 2),
                ratatui::layout::Rect::new(COLS - 1, 1, 1, ROWS - 2),
            ]
        } else {
            Vec::new()
        };
        assert_eq!(
            holes, expected,
            "native eligibility and border occlusion must agree on portal paint order"
        );
        return;
    }
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let cursor = StdCell::new(None);
    let rendered = harness.runner.with_render_context(&cursor, |ctx| {
        terminal.draw(|f| render(f, ctx)).unwrap().buffer.clone()
    });
    let output = terminal.backend().test_output();
    let output = String::from_utf8_lossy(&output);
    let native = output
        .split("\x1b_Ga=p,")
        .skip(1)
        .any(|command| !command.split("\x1b\\").next().unwrap().contains("c="));
    if image_overlay != 0 || !stacked {
        assert_eq!(
            native,
            matches!(
                image_overlay,
                0 | 4 | 5 | 7 | 8 | 10 | 11 | 12 | 14 | 15 | 17 | 18 | 21
            ),
            "native placements may not cover a later widget"
        );
    }
    if matches!(image_overlay, 14 | 17 | 18) {
        assert!(
            output.contains("x=10,y=20,w=110,h=80"),
            "a fading border must cut the picture's top and left edge: {output:?}"
        );
    }
    if matches!(image_overlay, 15 | 20 | 21) {
        assert!(
            output.contains("x=0,y=0,w=120,h=100"),
            "an invisible or non-overlapping frame must not crop the picture (mode {image_overlay}): {output:?}"
        );
        assert!(!output.contains("x=10,y=20,w=110,h=80"));
    }
    if image_overlay == 9 {
        assert!(
            output.contains("COVER"),
            "the plain frame footer must reach the host: {output:?}"
        );
    }
    if matches!(image_overlay, 1..=3 | 6) {
        assert!(
            output.contains("COVER"),
            "the overlay must reach the host: {output:?}"
        );
        let symbols: String = (5..10)
            .map(|x| rendered[(x, 2)].symbol().to_owned())
            .collect();
        assert_eq!(symbols, "COVER", "widget {image_overlay}: {output:?}");
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn transparent_modal_text_reaches_the_host_above_positive_z_images() {
    assert_transparent_modal_text_above_positive_z_images(false);
}

#[cfg(feature = "terminal-images")]
#[test]
fn transparent_modal_ignores_translation_above_its_portal_root() {
    assert_transparent_modal_text_above_positive_z_images(true);
}

#[cfg(feature = "terminal-images")]
fn assert_transparent_modal_text_above_positive_z_images(translated: bool) {
    use base64::Engine as _;
    // Measured cells allow native forwarding; unmeasured cells force the composed reference.
    for measured in [true, false] {
        let _host =
            crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), measured);
        let mut harness = build(
            PaneCfg {
                transparent_modal: true,
                ..PaneCfg::new()
            },
            b"\x1b[?25l\x1b[H",
        );
        harness
            .screen
            .borrow_mut()
            .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        let payload =
            base64::engine::general_purpose::STANDARD.encode([0, 255, 0, 255].repeat(240 * 120));
        harness.screen.borrow_mut().process_bytes(
            format!("\x1b_Ga=T,f=32,s=240,v=120,i=921,z=2;{payload}\x1b\\").as_bytes(),
        );
        harness.runner.core.tree.refresh_live_terminals_detailed();
        if translated {
            let animated = harness
                .runner
                .core
                .tree
                .iter()
                .find(|node| matches!(node.kind, NodeKind::Animated(_)))
                .unwrap()
                .id;
            let NodeKind::Animated(animated) =
                &mut harness.runner.core.tree.node_mut(animated).kind
            else {
                unreachable!();
            };
            animated.current_x_offset = 100.0;
        }
        let overlays = harness.runner.core.tree.overlay_roots();
        assert_eq!(overlays.len(), 1);
        assert!(overlays[0].backdrop.is_none_or(|style| style.is_empty()));
        let overlay = harness.runner.core.tree.node(overlays[0].id);
        assert!(matches!(
            crate::backend::ratatui_backend::render::overlay_clear_restore_mode(overlay),
            crate::backend::ratatui_backend::render::OverlayClearRestoreMode::PreserveForeground,
        ));
        let rect = overlay.rect;
        let mut terminal =
            crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
        let cursor = StdCell::new(None);
        let rendered = harness.runner.with_render_context(&cursor, |ctx| {
            terminal.draw(|f| render(f, ctx)).unwrap().buffer.clone()
        });
        let output = terminal.backend().test_output();
        let output = String::from_utf8_lossy(&output);
        assert!(
            output.contains("COVER"),
            "modal text must reach the host: {output:?}"
        );
        let symbols: String = (rect.x as u16..rect.x as u16 + 5)
            .map(|x| rendered[(x, rect.y as u16)].symbol().to_owned())
            .collect();
        assert_eq!(symbols, "COVER");
        assert!(
            output.contains("a=p,") || output.contains('\u{10EEEE}'),
            "the image must still reach the host: {output:?}"
        );
        assert!(
            !output
                .split("\x1b_Ga=p,")
                .skip(1)
                .any(|command| { !command.split("\x1b\\").next().unwrap().contains("c=") }),
            "positive native placements must not cover modal text: {output:?}"
        );
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn terminal_inside_declarative_portal_emits_native_placements() {
    use base64::Engine as _;
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(
        PaneCfg {
            terminal_portal: true,
            ..PaneCfg::new()
        },
        b"\x1b[?25l\x1b[H",
    );
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let payload =
        base64::engine::general_purpose::STANDARD.encode([0, 255, 0, 255].repeat(120 * 100));
    harness
        .screen
        .borrow_mut()
        .process_bytes(format!("\x1b_Ga=T,f=32,s=120,v=100,i=922,z=2;{payload}\x1b\\").as_bytes());
    harness.runner.core.tree.refresh_live_terminals_detailed();
    assert_eq!(harness.runner.core.tree.overlay_roots().len(), 1);
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let cursor = StdCell::new(None);
    harness.runner.with_render_context(&cursor, |ctx| {
        terminal.draw(|f| render(f, ctx)).unwrap();
    });
    let output = terminal.backend().test_output();
    let output = String::from_utf8_lossy(&output);
    assert!(
        output
            .split("\x1b_Ga=p,")
            .skip(1)
            .any(|command| { !command.split("\x1b\\").next().unwrap().contains("c=") }),
        "eligible portal terminal must emit native placements: {output:?}"
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn parser_queries_do_not_write_a_terminal_paint_frame() {
    let mut harness = build(PaneCfg::new(), b"stable");
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let cursor = StdCell::new(None);
    harness.runner.with_render_context(&cursor, |ctx| {
        terminal.draw(|f| render(f, ctx)).unwrap();
    });
    let initial = terminal.backend().test_output().len();
    let mutation = harness.screen.borrow_mut().process_bytes(b"\x1b[c");
    assert!(mutation.responses);
    assert!(!mutation.is_visual());
    harness
        .runner
        .render_terminal_paint_only(&mut terminal)
        .unwrap();
    assert_eq!(terminal.backend().test_output().len(), initial);
}

#[cfg(feature = "terminal-images")]
#[test]
fn graphics_only_updates_use_committed_plan_without_repainting_cells() {
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(PaneCfg::new(), b"stable\x1b[?25l");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let retained = harness.runner.last_frame_snapshot.clone();
    for command in [
        b"\x1b_Ga=T,f=32,s=1,v=1,i=919,C=1,q=2;/////w==\x1b\\".as_slice(),
        b"\x1b_Ga=T,f=32,s=1,v=1,i=919,C=1,q=2;AAAA/w==\x1b\\".as_slice(),
        b"\x1b_Ga=d,d=I,i=919,q=2;\x1b\\".as_slice(),
    ] {
        let start = terminal.backend().test_output().len();
        let mutation = harness.screen.borrow_mut().process_bytes(command);
        assert_eq!(mutation.cells, crate::widgets::TerminalDamage::None);
        let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
        assert_eq!(refresh.graphics_only.len(), 1);
        assert!(
            harness
                .runner
                .try_present_terminal_graphics(&mut terminal, &refresh)
                .unwrap()
        );
        assert_eq!(harness.runner.last_frame_snapshot, retained);
        let output = terminal.backend().test_output();
        let update = String::from_utf8_lossy(&output[start..]);
        assert!(update.contains("\x1b_G"), "{update:?}");
        assert!(!update.contains("stable"));
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn graphics_plan_rejects_changed_host_geometry_and_layout() {
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(PaneCfg::new(), b"stable\x1b[?25l");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    harness
        .screen
        .borrow_mut()
        .process_bytes(b"\x1b_Ga=T,f=32,s=1,v=1,i=919,C=1,q=2;/////w==\x1b\\");
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    {
        let _changed_host =
            crate::backend::ratatui_backend::image_support::test_kitty_host((11, 20), true);
        assert!(
            !harness
                .runner
                .try_present_terminal_graphics(&mut terminal, &refresh)
                .unwrap()
        );
    }
    harness.runner.graphics_plan_epoch = None;
    assert!(
        !harness
            .runner
            .try_present_terminal_graphics(&mut terminal, &refresh)
            .unwrap()
    );
}

#[cfg(feature = "terminal-images")]
#[test]
fn presentation_tokens_follow_successful_host_frames_and_ignore_failed_updates() {
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    for discard in [false, true] {
        let mut harness = build(PaneCfg::new(), b"stable\x1b[?25l");
        harness
            .screen
            .borrow_mut()
            .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
        harness.screen.borrow_mut().set_presentation_token(1);
        let reports = Rc::new(RefCell::new(Vec::new()));
        let received = reports.clone();
        let _subscription =
            harness
                .runner
                .core
                .ctx
                .observe_terminal_presentations(crate::Callback::new(move |batch| {
                    received.borrow_mut().push(batch)
                }));
        let mut terminal =
            crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
        harness.runner.render_paint_only(&mut terminal).unwrap();
        assert_eq!(reports.borrow().len(), 1);
        assert_eq!(reports.borrow()[0][0].token, 1);
        let id = harness.screen.borrow().stable_terminal_id();
        assert_eq!(reports.borrow()[0][0].stable_terminal_id, id);
        let _capture = harness.runner.capture_painted_frame();
        assert_eq!(reports.borrow().len(), 1);
        harness
            .screen
            .borrow_mut()
            .process_bytes(b"\x1b_Ga=T,f=32,s=1,v=1,i=919,C=1,q=2;/////w==\x1b\\");
        harness.screen.borrow_mut().set_presentation_token(2);
        let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
        harness.runner.host_frame_presented = false;
        terminal.backend_mut().test_fail_next_flush(discard);
        let result = harness
            .runner
            .try_present_terminal_graphics(&mut terminal, &refresh);
        if discard {
            assert!(result.unwrap());
            assert!(!harness.runner.host_frame_presented);
        } else {
            assert!(result.is_err());
        }
        assert_eq!(
            reports.borrow().len(),
            1,
            "failed or discarded output must never acknowledge tokens"
        );
        assert!(
            harness
                .runner
                .try_present_terminal_graphics(&mut terminal, &refresh)
                .unwrap()
        );
        assert_eq!(reports.borrow().len(), 2);
        assert_eq!(reports.borrow()[1][0].token, 2);
        assert_eq!(reports.borrow()[1][0].stable_terminal_id, id);
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn metadata_modes_and_unchanged_graphics_do_not_write_or_present_frames() {
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(PaneCfg::new(), b"stable\x1b[?25l");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let start = terminal.backend().test_output().len();
    for bytes in [b"\x1b[?2004h".as_slice(), b"\x1b[?1003h", b"\x1b[?1h"] {
        let mutation = harness.screen.borrow_mut().process_bytes(bytes);
        assert!(mutation.modes_changed);
        assert!(!mutation.is_visual(), "{mutation:?}");
        harness.runner.frame_rendered = false;
        harness.runner.host_frame_presented = false;
        harness
            .runner
            .render_terminal_paint_only(&mut terminal)
            .unwrap();
        assert!(!harness.runner.frame_rendered);
        assert!(!harness.runner.host_frame_presented);
        assert_eq!(terminal.backend().test_output().len(), start);
    }
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    assert!(
        harness
            .runner
            .try_present_terminal_graphics(&mut terminal, &refresh)
            .unwrap()
    );
    assert!(!harness.runner.host_frame_presented);
    assert_eq!(terminal.backend().test_output().len(), start);
}

#[cfg(feature = "terminal-images")]
#[derive(Clone, Debug)]
struct ImageMaskProbe {
    columns: std::sync::Arc<std::sync::atomic::AtomicU16>,
}

#[cfg(feature = "terminal-images")]
impl crate::style::CellEffect for ImageMaskProbe {
    fn apply(&self, cell: &mut crate::style::EffectCell, ctx: &crate::style::EffectContext) {
        if ctx.x - ctx.bounds.x >= self.columns.load(std::sync::atomic::Ordering::Relaxed) as i16 {
            cell.set_symbol("*");
        }
    }

    fn image_mask(&self, ctx: &crate::style::EffectPrepareContext) -> Option<crate::CellMask> {
        let columns = self.columns.load(std::sync::atomic::Ordering::Relaxed);
        let count = usize::from(ctx.bounds.w) * usize::from(ctx.bounds.h);
        let mut bits = vec![0u64; count.div_ceil(64)];
        for y in 0..ctx.bounds.h {
            for x in 0..columns.min(ctx.bounds.w) {
                let bit = usize::from(y) * usize::from(ctx.bounds.w) + usize::from(x);
                bits[bit / 64] |= 1 << (bit % 64);
            }
        }
        Some(crate::CellMask {
            origin: (0, 0),
            w: ctx.bounds.w,
            h: ctx.bounds.h,
            bits: bits.into(),
        })
    }
}

#[cfg(feature = "terminal-images")]
#[derive(Clone)]
struct DimmedImagePane {
    screen: Rc<RefCell<TerminalScreen>>,
}

#[cfg(feature = "terminal-images")]
impl Component for DimmedImagePane {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        crate::widgets::EffectScope::new()
            .dim_by(0.25)
            .child(
                crate::widgets::Terminal::new()
                    .screen(self.screen.clone())
                    .scrollbar(false),
            )
            .into()
    }
}

/// Dims a terminal showing `setup`, returning what it paints and what the host receives.
#[cfg(feature = "terminal-images")]
fn paint_dimmed_terminal(setup: String) -> (Buffer, String) {
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 20,
    };
    let screen = Rc::new(RefCell::new(TerminalScreen::new(20, 40, 0)));
    screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    screen.borrow_mut().process_bytes(setup.as_bytes());
    let component = DimmedImagePane { screen };
    let mut runner = AppRunner::new(App::new(), component.clone(), ());
    runner.core = RuntimeCore::new_test(
        component,
        (),
        bounds,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    )
    .into();
    runner.core.init();
    runner.core.render_element(bounds, None, None, None);
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(40, 20).unwrap();
    let cursor = StdCell::new(None);
    let rendered = runner.with_render_context(&cursor, |ctx| {
        terminal.draw(|f| render(f, ctx)).unwrap().buffer.clone()
    });
    let output = terminal.backend().test_output();
    (rendered, String::from_utf8_lossy(&output).into_owned())
}

#[cfg(feature = "terminal-images")]
fn assert_text_survives_as_a_placement(
    rendered: &Buffer,
    output: &str,
    text: &str,
    (x, y): (u16, u16),
) {
    let drawn: String = (x..x + text.len() as u16)
        .map(|x| rendered[(x, y)].symbol().to_owned())
        .collect();
    assert!(
        drawn.ends_with(&text[text.len() - 1..]),
        "{text}: {drawn:?}"
    );
    assert!(
        output.contains(text),
        "{text} must reach the host: {output:?}"
    );
    assert!(
        !output.contains('\u{10EEEE}'),
        "a placeholder would replace the text in its cell"
    );
    assert!(output.contains("a=p,"), "the picture must still be placed");
}

/// A picture that is not a whole number of cells is composed into fixed tiles, which reach well
/// past it. Drawn as Unicode placeholders, every tile replaced its cells and erased the text around
/// the picture, wherever the tile was transparent.
#[cfg(feature = "terminal-images")]
#[test]
fn a_dimmed_unaligned_image_leaves_the_text_around_it() {
    use base64::Engine as _;
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let pixels =
        base64::engine::general_purpose::STANDARD.encode([255, 0, 0, 255].repeat(105 * 105));
    let (rendered, output) = paint_dimmed_terminal(format!(
        "\x1b[?25l\x1b[2;2Habove\x1b[6;1H\x1b_Ga=T,f=32,s=105,v=105,i=7,q=2;{pixels}\x1b\\\
         \x1b[13;2Hbelow"
    ));
    assert_text_survives_as_a_placement(&rendered, &output, "above", (1, 1));
    assert_text_survives_as_a_placement(&rendered, &output, "below", (1, 12));
}

/// A picture exactly on the cell grid skips the tiles, but a placeholder would still replace the
/// text inside its rectangle, where its transparent pixels should leave that text showing.
#[cfg(feature = "terminal-images")]
#[test]
fn a_dimmed_cell_aligned_image_leaves_the_text_under_its_transparent_pixels() {
    use base64::Engine as _;
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let pixels = base64::engine::general_purpose::STANDARD.encode([255, 0, 0, 0].repeat(100 * 100));
    let (rendered, output) = paint_dimmed_terminal(format!(
        "\x1b[?25l\x1b[7;2Hinside\x1b[6;1H\x1b_Ga=T,f=32,s=100,v=100,i=7,q=2;{pixels}\x1b\\"
    ));
    assert_text_survives_as_a_placement(&rendered, &output, "inside", (1, 6));
}

#[cfg(feature = "terminal-images")]
#[derive(Clone)]
struct MaskedImagePane {
    screen: Rc<RefCell<TerminalScreen>>,
    effect: ImageMaskProbe,
}

#[cfg(feature = "terminal-images")]
impl Component for MaskedImagePane {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        crate::widgets::EffectScope::new()
            .custom_effect(self.effect.clone())
            .child(
                crate::widgets::Terminal::new()
                    .screen(self.screen.clone())
                    .scrollbar(false),
            )
            .into()
    }
}

#[cfg(feature = "terminal-images")]
#[test]
fn a_cell_effect_removes_native_picture_fragments_on_paint_only_frames() {
    use base64::Engine as _;
    use std::sync::{
        Arc,
        atomic::{AtomicU16, Ordering},
    };
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 5,
    };
    let screen = Rc::new(RefCell::new(TerminalScreen::new(5, 12, 0)));
    screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let pixels =
        base64::engine::general_purpose::STANDARD.encode([0, 255, 0, 255].repeat(120 * 100));
    screen.borrow_mut().process_bytes(
        format!("\x1b[?25l\x1b[H\x1b_Ga=T,f=32,s=120,v=100,i=7,p=9,C=1,q=2,z=1;{pixels}\x1b\\")
            .as_bytes(),
    );
    let columns = Arc::new(AtomicU16::new(12));
    let component = MaskedImagePane {
        screen,
        effect: ImageMaskProbe {
            columns: columns.clone(),
        },
    };
    let capture_component = component.clone();
    let mut runner = AppRunner::new(App::new(), component.clone(), ());
    runner.core = RuntimeCore::new_test(
        component,
        (),
        bounds,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    )
    .into();
    runner.core.init();
    runner.core.render_element(bounds, None, None, None);
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(12, 5).unwrap();
    let cursor = StdCell::new(None);
    let mut offset = 0;
    for visible in [12, 6, 0] {
        columns.store(visible, Ordering::Relaxed);
        let _plans = terminal.backend().native_plan_scope();
        let buffer = runner.with_render_context(&cursor, |ctx| {
            terminal.draw(|f| render(f, ctx)).unwrap().buffer.clone()
        });
        let output = terminal.backend().test_output();
        let commands = String::from_utf8_lossy(&output[offset..]);
        if visible > 0 {
            assert!(
                commands.contains(&format!("w={},h=100", visible * 10)),
                "mask must clip the host picture: {commands:?}"
            );
        } else {
            assert!(
                commands.contains("a=d"),
                "the image must disappear before the effect is unmounted: {commands:?}"
            );
        }
        if visible != 12 {
            assert!(
                !commands.contains("a=t"),
                "moving the mask must not reupload the bitmap"
            );
        }
        for x in visible..12 {
            assert_eq!(buffer[(x, 0)].symbol(), "*");
        }
        offset = output.len();
    }
    columns.store(6, Ordering::Relaxed);
    let mut capture = crate::TestBackend::new(capture_component);
    capture.set_viewport(bounds);
    capture.render();
    let frame = capture.capture_frame();
    let visible_cells = frame
        .images
        .iter()
        .flat_map(|image| image.visible.iter())
        .filter(|&&visible| visible)
        .count();
    assert_eq!(
        visible_cells, 30,
        "captures must retain only the same picture fragments"
    );
}

/// A masked picture above a status row that lies outside the effect.
#[cfg(feature = "terminal-images")]
#[derive(Clone)]
struct MaskedImageWithStatus {
    screen: Rc<RefCell<TerminalScreen>>,
    effect: ImageMaskProbe,
}

#[cfg(feature = "terminal-images")]
impl Component for MaskedImageWithStatus {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let picture = crate::widgets::EffectScope::new()
            .custom_effect(self.effect.clone())
            .child(
                crate::widgets::Terminal::new()
                    .screen(self.screen.clone())
                    .scrollbar(false),
            );
        crate::widgets::Canvas::new()
            .child_at(
                Rect {
                    x: 0,
                    y: 0,
                    w: 12,
                    h: 5,
                },
                picture,
            )
            .child_at(
                Rect {
                    x: 0,
                    y: 5,
                    w: 12,
                    h: 1,
                },
                Text::new("status"),
            )
            .into()
    }
}

/// A partial paint whose damage reaches only part of an effect's scope, such as a sidebar spinner
/// ticking beside a closing pane, still lays out the whole committed native scene of the terminal
/// inside it. Cutting the effect's mask to the damaged region left the rest of that picture without
/// holes, so every such paint put it back on the host for one frame while the effect hid it.
#[cfg(feature = "terminal-images")]
#[test]
fn a_partial_paint_elsewhere_keeps_an_effect_hiding_native_picture_fragments() {
    use base64::Engine as _;
    use std::sync::{Arc, atomic::AtomicU16};
    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 6,
    };
    let screen = Rc::new(RefCell::new(TerminalScreen::new(5, 12, 0)));
    screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let pixels =
        base64::engine::general_purpose::STANDARD.encode([0, 255, 0, 255].repeat(120 * 100));
    screen.borrow_mut().process_bytes(
        format!("\x1b[?25l\x1b[H\x1b_Ga=T,f=32,s=120,v=100,i=7,p=9,C=1,q=2,z=1;{pixels}\x1b\\")
            .as_bytes(),
    );
    let component = MaskedImageWithStatus {
        screen,
        effect: ImageMaskProbe {
            columns: Arc::new(AtomicU16::new(6)),
        },
    };
    let mut runner = AppRunner::new(App::new(), component.clone(), ());
    runner.core = RuntimeCore::new_test(
        component,
        (),
        bounds,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    )
    .into();
    runner.core.init();
    runner.core.render_element(bounds, None, None, None);
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(12, 6).unwrap();
    let cursor = StdCell::new(None);
    {
        let _plans = terminal.backend().native_plan_scope();
        runner.with_render_context(&cursor, |ctx| {
            terminal.draw(|f| render(f, ctx)).unwrap();
        });
    }
    let full = terminal.backend().test_output();
    assert!(
        String::from_utf8_lossy(&full).contains("w=60,h=100"),
        "the full paint must place only the unmasked half"
    );
    let offset = full.len();
    // Damage that reaches one row into the picture, as a sidebar or a status line beside a pane
    // can.
    let damage = Rect {
        x: 0,
        y: 4,
        w: 12,
        h: 2,
    };
    {
        let _plans = terminal.backend().native_plan_scope();
        runner.with_render_context(&cursor, |ctx| {
            terminal
                .draw(|f| crate::backend::ratatui_backend::render_regions(f, ctx, &[damage]))
                .unwrap();
        });
    }
    let output = terminal.backend().test_output();
    let commands = String::from_utf8_lossy(&output[offset..]);
    assert!(
        !commands.contains("w=120"),
        "a partial paint must not put back the fragments the effect hides: {commands:?}"
    );
}

/// The production trail, end to end, for both fills: a focus change between two terminals starts
/// it, full draws paint it frame by frame as it moves, terminal output during it takes the full
/// draw instead of a row patch, a capture shows what the host shows, a solid smear hides the host
/// caret while it draws its own, and the frame after it settles puts every cell and the caret back.
#[test]
fn a_cursor_trail_paints_over_full_draws_and_clears_once_it_settles() {
    use crate::app::interaction_state::{DirtyLevel, DirtyTracker};
    use crate::app::{CursorTrail, CursorTrailFill, CursorTrailTrigger};

    const TRAIL: ratatui::style::Color = ratatui::style::Color::Rgb(255, 0, 0);
    const FRAME: std::time::Duration = std::time::Duration::from_millis(16);
    for fill in [CursorTrailFill::Solid, CursorTrailFill::Tint] {
        let mut harness = build(
            PaneCfg {
                second_terminal: true,
                grid_rows: ROWS / 2,
                focused: true,
                controlled_clock: true,
                ..PaneCfg::new()
            },
            b"stable",
        );
        let env = harness.runner.core.ctx.env().clone();
        env.cursor_trail.set(Some(
            CursorTrail::new()
                .trigger(CursorTrailTrigger::FocusChange)
                .fill(fill)
                .color(Color::Rgb(255, 0, 0)),
        ));
        let mut terminal =
            crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
        let dirty_level = |runner: &mut AppRunner<Pane>| {
            let mut dirty = DirtyTracker::default();
            runner.update_animation_cycle(&mut dirty);
            dirty.level()
        };

        harness.runner.render_paint_only(&mut terminal).unwrap();
        assert_eq!(dirty_level(&mut harness.runner), DirtyLevel::None);

        let second = terminal_ids(&harness.runner.core.tree)[1];
        harness.runner.focus.focused = Some(second);
        harness.runner.render_paint_only(&mut terminal).unwrap();
        let (_, caret) = full_paint(&harness.runner, &mut harness.term);
        let caret = caret.expect("the second terminal shows its caret");
        assert_eq!(caret, Position::new(6, ROWS / 2));

        let mut trail_rows = std::collections::BTreeSet::new();
        let mut hidden_frames = 0;
        let mut frames = 0;
        let mut output_checked = false;
        loop {
            env.advance_clock(FRAME);
            if dirty_level(&mut harness.runner) == DirtyLevel::None {
                break;
            }
            frames += 1;
            assert!(frames < 120, "{fill:?}: the trail never settled");
            let before = terminal.backend().test_output().len();
            harness.runner.render_paint_only(&mut terminal).unwrap();
            let sent =
                String::from_utf8_lossy(&terminal.backend().test_output()[before..]).into_owned();
            let painted = harness
                .runner
                .last_frame_snapshot
                .clone()
                .expect("retained frame");
            let trailed: Vec<u16> = (0..ROWS)
                .filter(|&y| {
                    (caret.x.saturating_sub(1)..=caret.x + 1).any(|x| {
                        let cell = &painted[(x, y)];
                        cell.fg == TRAIL || cell.bg == TRAIL
                    })
                })
                .collect();
            if trailed.is_empty() {
                continue;
            }
            trail_rows.extend(trailed);
            let caret_hidden = sent.rfind("\x1b[?25l") > sent.rfind("\x1b[?25h");
            match fill {
                CursorTrailFill::Solid => {
                    // Never shown and hidden again: a terminal that renders partway through the
                    // frame would catch the caret at its destination.
                    assert!(
                        !sent.contains("\x1b[?25h"),
                        "a frame that draws the caret itself never shows the host's: {sent:?}"
                    );
                    assert!(
                        caret_hidden,
                        "a solid smear draws the caret itself: {sent:?}"
                    );
                    hidden_frames += 1;
                    assert!(
                        harness.runner.capture_painted_frame().cursor.is_none(),
                        "a capture shows the caret hidden, as the host does"
                    );
                }
                CursorTrailFill::Tint => {
                    assert!(!caret_hidden, "a tint leaves the caret where it is");
                    let (untrailed, _) = full_paint(&harness.runner, &mut harness.term);
                    for y in 0..ROWS {
                        for x in 0..COLS {
                            assert_eq!(
                                painted[(x, y)].symbol(),
                                untrailed[(x, y)].symbol(),
                                "a tint keeps the text at ({x}, {y})"
                            );
                        }
                    }
                }
            }
            let captured = harness.runner.capture_painted_frame();
            let (x, y) = (0..ROWS)
                .flat_map(|y| (0..COLS).map(move |x| (x, y)))
                .find(|&(x, y)| painted[(x, y)].fg == TRAIL || painted[(x, y)].bg == TRAIL)
                .expect("a trail cell");
            let cell = captured.cell(x, y);
            assert!(
                cell.fg == Color::Rgb(255, 0, 0) || cell.bg == Color::Rgb(255, 0, 0),
                "a capture shows the trail the host shows"
            );

            if !output_checked {
                // Output while the trail runs repaints through the full draw, never a row patch
                // over it.
                output_checked = true;
                harness.screen.borrow_mut().process_bytes(b"\r\nmore");
                harness
                    .runner
                    .render_terminal_paint_only(&mut terminal)
                    .unwrap();
                assert!(
                    harness.runner.host_patched_rows.is_empty(),
                    "a row patch ran while the trail was on screen"
                );
            }
        }
        assert!(output_checked, "{fill:?}: a trail was painted");
        assert!(
            trail_rows.iter().any(|&y| y > 0 && y < caret.y),
            "{fill:?}: the smear crossed the rows between the carets: {trail_rows:?}"
        );
        if fill == CursorTrailFill::Solid {
            assert!(hidden_frames > 1);
        }
        let (clean, _) = full_paint(&harness.runner, &mut harness.term);
        assert_eq!(
            harness.runner.last_frame_snapshot.as_ref(),
            Some(&clean),
            "{fill:?}: the cleared frame matches a paint with no trail"
        );
        let sent = String::from_utf8_lossy(&terminal.backend().test_output()).into_owned();
        assert!(
            sent.rfind("\x1b[?25h") > sent.rfind("\x1b[?25l"),
            "{fill:?}: the caret is back once the trail settles"
        );
    }
}

/// `Context::skip_cursor_trail` makes the caret's next move draw no trail, however many frames
/// later it comes, and stops one in flight, for that move only: the move after it trails again.
#[test]
fn a_skipped_cursor_trail_draws_nothing_for_the_next_move_only() {
    use crate::app::{CursorTrail, CursorTrailTrigger};

    const FRAME: std::time::Duration = std::time::Duration::from_millis(16);
    let mut harness = build(
        PaneCfg {
            second_terminal: true,
            grid_rows: ROWS / 2,
            focused: true,
            controlled_clock: true,
            ..PaneCfg::new()
        },
        b"stable",
    );
    let env = harness.runner.core.ctx.env().clone();
    env.cursor_trail.set(Some(
        CursorTrail::new().trigger(CursorTrailTrigger::FocusChange),
    ));
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    let [first, second] = terminal_ids(&harness.runner.core.tree)[..] else {
        panic!("two terminals");
    };
    harness.runner.render_paint_only(&mut terminal).unwrap();

    // Skipped: a frame that still shows the caret where it was keeps the skip pending, and the
    // focus change after it draws nothing, on its frame or after.
    env.cursor_trail_skip.set(true);
    env.advance_clock(FRAME);
    harness.runner.render_paint_only(&mut terminal).unwrap();
    harness.runner.focus.focused = Some(second);
    for _ in 0..3 {
        harness.runner.render_paint_only(&mut terminal).unwrap();
        assert!(
            !harness.runner.cursor_trail_requires_full_draw(),
            "a skipped move started a trail"
        );
        env.advance_clock(FRAME);
    }

    // Not skipped: the next focus change trails again.
    harness.runner.focus.focused = Some(first);
    harness.runner.render_paint_only(&mut terminal).unwrap();
    assert!(
        harness.runner.cursor_trail_requires_full_draw(),
        "the skip outlived the move it was for"
    );

    // A skip mid-flight stops that trail with the next frame.
    env.cursor_trail_skip.set(true);
    env.advance_clock(FRAME);
    harness.runner.render_paint_only(&mut terminal).unwrap();
    env.advance_clock(FRAME);
    harness.runner.render_paint_only(&mut terminal).unwrap();
    assert!(
        !harness.runner.cursor_trail_requires_full_draw(),
        "a skip left the trail in flight running"
    );
    let (clean, _) = full_paint(&harness.runner, &mut harness.term);
    assert_eq!(
        harness.runner.last_frame_snapshot.as_ref(),
        Some(&clean),
        "the stopped trail's cells are cleared"
    );
}

/// Video streamed as anonymous frames, the way mpv's `--vo=kitty` sends it, stacks every frame
/// over the last at the same cell. Only the newest can show, so it reaches a Kitty host natively,
/// as one placement, instead of the whole stack being composed into tiles for every frame.
#[cfg(feature = "terminal-images")]
#[test]
fn stacked_video_frames_reach_the_host_natively() {
    use base64::Engine as _;

    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(PaneCfg::new(), b"\x1b[?25l\x1b[H");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    for shade in 0..10u8 {
        let payload =
            base64::engine::general_purpose::STANDARD.encode([shade, 0, 0].repeat(105 * 45));
        harness.screen.borrow_mut().process_bytes(
            format!("\x1b[H\x1b_Ga=T,f=24,s=105,v=45,C=1,q=2;{payload}\x1b\\").as_bytes(),
        );
    }
    harness.runner.core.tree.refresh_live_terminals_detailed();
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let output = String::from_utf8_lossy(&terminal.backend().test_output()).into_owned();
    let headers: Vec<_> = output
        .split("\x1b_G")
        .skip(1)
        .map(|command| command.split(';').next().unwrap().to_owned())
        .collect();
    let placements: Vec<_> = headers.iter().filter(|h| h.starts_with("a=p,")).collect();
    assert_eq!(
        placements.len(),
        1,
        "one placement for the newest frame: {headers:#?}"
    );
    assert!(
        placements[0].contains("w=105,h=45"),
        "the frame itself, uncomposed: {headers:#?}"
    );
    assert!(
        headers.iter().all(|h| !h.contains("a=T")),
        "no composed tile: {headers:#?}"
    );
}

/// After the first paint, each new video frame is a graphics-only update. It stacks on the frames
/// before it, and still reaches the host as one native placement without repainting cells.
#[cfg(feature = "terminal-images")]
#[test]
fn a_new_video_frame_presents_natively_without_a_repaint() {
    use base64::Engine as _;

    let _host = crate::backend::ratatui_backend::image_support::test_kitty_host((10, 20), true);
    let mut harness = build(PaneCfg::new(), b"stable\x1b[?25l\x1b[H");
    harness
        .screen
        .borrow_mut()
        .set_cell_size(crate::widgets::TerminalCellSize::new(10, 20));
    let frame = |shade: u8| {
        let payload =
            base64::engine::general_purpose::STANDARD.encode([shade, 0, 0].repeat(105 * 45));
        format!("\x1b_Ga=T,f=24,s=105,v=45,C=1,q=2;{payload}\x1b\\")
    };
    for shade in 0..5u8 {
        harness
            .screen
            .borrow_mut()
            .process_bytes(frame(shade).as_bytes());
    }
    harness.runner.core.tree.refresh_live_terminals_detailed();
    let mut terminal = crate::backend::ratatui_backend::create_test_terminal(COLS, ROWS).unwrap();
    harness.runner.render_paint_only(&mut terminal).unwrap();
    let retained = harness.runner.last_frame_snapshot.clone();

    for shade in 5..8u8 {
        let start = terminal.backend().test_output().len();
        let mutation = harness
            .screen
            .borrow_mut()
            .process_bytes(frame(shade).as_bytes());
        assert_eq!(mutation.cells, crate::widgets::TerminalDamage::None);
        let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
        assert_eq!(refresh.graphics_only.len(), 1);
        assert!(
            harness
                .runner
                .try_present_terminal_graphics(&mut terminal, &refresh)
                .unwrap(),
            "the graphics-only path takes the frame"
        );
        assert_eq!(harness.runner.last_frame_snapshot, retained);
        let output = terminal.backend().test_output();
        let update = String::from_utf8_lossy(&output[start..]).into_owned();
        assert!(!update.contains("stable"), "no cell repaint: {update:?}");
        let headers: Vec<_> = update
            .split("\x1b_G")
            .skip(1)
            .map(|command| command.split(';').next().unwrap().to_owned())
            .collect();
        let placements: Vec<_> = headers.iter().filter(|h| h.starts_with("a=p,")).collect();
        assert_eq!(placements.len(), 1, "{headers:#?}");
        assert!(placements[0].contains("w=105,h=45"), "{headers:#?}");
        assert!(headers.iter().all(|h| !h.contains("a=T")), "{headers:#?}");
    }
}

/// An input that a sidebar docks beside once `docked` is set, wrapping it in a new container.
#[derive(Clone)]
struct DockingInput {
    docked: Rc<StdCell<bool>>,
}

impl Component for DockingInput {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let input = Element::from(crate::widgets::Input::new("")).key("input");
        if self.docked.get() {
            crate::widgets::HStack::new()
                .child(Text::new("side").width(crate::style::Length::Px(6)))
                .child(input)
                .into()
        } else {
            input
        }
    }
}

/// Docking a sidebar beside the focused widget rebuilds its node under a new container. The
/// caret's widget is still the same one, so its move is the widget resizing, not a focus change
/// with a trail from where the caret was.
#[test]
fn a_rebuilt_keyed_widget_keeps_its_caret_owner() {
    use crate::app::cursor_trail::{CaretLook, CursorTrailState};
    use crate::app::{CursorTrail, CursorTrailTrigger};

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 3,
    };
    let docked = Rc::new(StdCell::new(false));
    let component = DockingInput {
        docked: Rc::clone(&docked),
    };
    let mut runner = AppRunner::new(App::new(), component.clone(), ());
    runner.core = RuntimeCore::new_test(
        component,
        (),
        bounds,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    )
    .into();
    runner.core.init();
    let focus_input = |runner: &mut AppRunner<DockingInput>| {
        runner.core.render_element(bounds, None, None, None);
        let id = runner
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref().is_some_and(|key| key.as_ref() == "input"))
            .map(|node| node.id)
            .expect("the input");
        runner.focus.focused = Some(id);
        (id, runner.caret_owner().expect("an owner"))
    };
    let (before_id, before) = focus_input(&mut runner);
    docked.set(true);
    let (after_id, after) = focus_input(&mut runner);
    assert_ne!(before_id, after_id, "the dock rebuilds the input's node");
    assert_eq!(before.id, after.id, "and it is still the same widget");
    assert_ne!(before.size, after.size);

    let trail = Some(CursorTrail::new().trigger(CursorTrailTrigger::FocusChange));
    let now = web_time::Instant::now();
    let mut state = CursorTrailState::default();
    let look = CaretLook::default();
    state.observe(trail, Some(Position::new(0, 0)), Some(before), look, now);
    state.observe(trail, Some(Position::new(6, 0)), Some(after), look, now);
    state.advance(trail, now);
    assert!(state.frame(trail).is_none(), "the dock draws no trail");
}

/// Every mouse dispatch tells the trail about pointer input, whichever path of the run loop
/// delivers it (a click coalesced behind pointer motion among them): a press or a scroll in the
/// focused widget quiets its caret's next jump, and one outside it does not.
#[test]
fn mouse_dispatch_quiets_the_focused_caret_s_jump_only_for_input_inside_it() {
    use crate::app::{CursorTrail, CursorTrailTrigger};
    use crate::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};

    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 3,
    };
    let component = DockingInput {
        docked: Rc::new(StdCell::new(true)),
    };
    let mut runner = AppRunner::new(App::new(), component.clone(), ());
    runner.core = RuntimeCore::new_test(
        component,
        (),
        bounds,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    )
    .into();
    runner.core.init();
    runner.core.render_element(bounds, None, None, None);
    let input = runner
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref().is_some_and(|key| key.as_ref() == "input"))
        .map(|node| node.id)
        .expect("the input");
    let trail = Some(CursorTrail::new().trigger(CursorTrailTrigger::jump()));
    let env = runner.core.ctx.env().clone();
    env.cursor_trail.set(trail);
    let mouse = |x, kind| MouseEvent {
        x,
        y: 0,
        kind,
        mods: KeyMods::default(),
    };
    // Whether a 20-column jump of the input's caret, just after `input`, draws a trail.
    let jump_trails = |runner: &mut AppRunner<DockingInput>,
                       pointer: &dyn Fn(&mut AppRunner<DockingInput>)| {
        runner.cursor_trail = Default::default();
        runner.focus.focused = Some(input);
        let owner = runner.caret_owner();
        let look = crate::app::cursor_trail::CaretLook::default();
        let now = env.now();
        runner
            .cursor_trail
            .observe(trail, Some(Position::new(8, 0)), owner, look, now);
        pointer(runner);
        runner.focus.focused = Some(input);
        let owner = runner.caret_owner();
        runner
            .cursor_trail
            .observe(trail, Some(Position::new(28, 0)), owner, look, now);
        runner.cursor_trail.advance(trail, now);
        runner.cursor_trail.frame(trail).is_some()
    };
    assert!(
        !jump_trails(&mut runner, &|runner| {
            runner.dispatch_mouse(mouse(20, MouseKind::Down(MouseButton::Left)));
        }),
        "a press in the input"
    );
    assert!(
        !jump_trails(&mut runner, &|runner| {
            runner.dispatch_mouse_scroll_level(mouse(20, MouseKind::ScrollDown), 1);
        }),
        "a scroll in the input"
    );
    assert!(
        jump_trails(&mut runner, &|runner| {
            runner.dispatch_mouse(mouse(2, MouseKind::Down(MouseButton::Left)));
        }),
        "a press in the sidebar beside it"
    );
}
