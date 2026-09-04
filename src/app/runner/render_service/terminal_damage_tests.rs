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
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect as RatatuiRect;

use super::AppRunner;
use super::terminal_damage::{DamageRejection, TerminalDamagePlan, patch_row};
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
    TerminalSelection, VStack,
};

const COLS: u16 = 40;
const ROWS: u16 = 12;

/// What the pane under test looks like. Each flag exists because it changes the cells the terminal
/// renderer produces, so a patch that ignored it would differ from a full paint.
#[derive(Clone, Copy, Default)]
struct PaneCfg {
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
    /// Run on an inline surface rather than the alternate screen.
    inline: bool,
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
        widget
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
        match &self.second {
            Some(second) => VStack::new()
                .child(self.terminal(&self.screen))
                .child(self.terminal(second))
                .into(),
            None => self.terminal(&self.screen).into(),
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
    term: Terminal<TestBackend>,
    screen: Rc<RefCell<TerminalScreen>>,
    second: Option<Rc<RefCell<TerminalScreen>>>,
    retained: Buffer,
}

fn build(cfg: PaneCfg, setup: &[u8]) -> Harness {
    let screen = Rc::new(RefCell::new(TerminalScreen::new(
        cfg.grid_rows(),
        COLS,
        500,
    )));
    screen.borrow_mut().process_bytes(setup);
    let second = cfg.second_terminal.then(|| {
        let screen = Rc::new(RefCell::new(TerminalScreen::new(
            cfg.grid_rows(),
            COLS,
            500,
        )));
        screen.borrow_mut().process_bytes(setup);
        screen
    });

    let component = || Pane {
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
    runner.core = RuntimeCore::new_test(
        component(),
        (),
        viewport(),
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(StdCell::new(false)),
    );
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
    if cfg.hovered {
        runner.mouse.hovered = Some(first);
        runner.mouse.last_mouse.set(Some((3, 1)));
    }
    if cfg.blink_hidden {
        runner.animation.blink_visible = false;
    }

    let mut term =
        Terminal::new(TestBackend::new(COLS, ROWS)).expect("test terminal should initialize");
    let retained = full_paint(&runner, &mut term);
    runner.last_frame_snapshot = Some(retained.clone());

    Harness {
        runner,
        term,
        screen,
        second,
        retained,
    }
}

/// The ordinary draw, verbatim: the production context, `render` over the whole frame.
fn full_paint(runner: &AppRunner<Pane>, term: &mut Terminal<TestBackend>) -> Buffer {
    let cursor_position = StdCell::new(None);
    runner.with_render_context(&cursor_position, |ctx| {
        term.draw(|f| render(f, ctx))
            .expect("a full paint should succeed")
            .buffer
            .clone()
    })
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

fn plan_for(harness: &mut Harness) -> Result<TerminalDamagePlan, DamageRejection> {
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    harness.runner.plan_terminal_damage(&refresh, frame_area())
}

/// Apply `mutation`, patch the damaged rows, and hold the result to a full paint.
fn assert_patch_matches_full_paint(cfg: PaneCfg, setup: &[u8], mutation: &[u8]) {
    let mut harness = build(cfg, setup);
    harness.screen.borrow_mut().process_bytes(mutation);

    let plan = plan_for(&mut harness).expect("this case is meant to be eligible");

    let mut patched = harness.retained.clone();
    let cursor_position = StdCell::new(None);
    let runner = &harness.runner;
    let term = &mut harness.term;
    runner.with_render_context(&cursor_position, |ctx| {
        for &row in &plan.rows {
            patch_row(term, ctx, &mut patched, frame_area(), row).expect("patching a row");
        }
    });

    let mut oracle = Terminal::new(TestBackend::new(COLS, ROWS)).expect("oracle terminal");
    let expected = full_paint(runner, &mut oracle);

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
}

const SETUP: &[u8] = b"first line here\r\nsecond line here\r\nthird line here\r\n";

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
                    patch_row(term, ctx, &mut patched, frame_area(), row).expect("patching a row");
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
        harness.runner.plan_terminal_damage(&refresh, frame_area()),
        Err(DamageRejection::ScrolledBack)
    );
}

#[test]
fn two_moved_terminals_are_rejected() {
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
    assert_eq!(
        plan_for(&mut harness),
        Err(DamageRejection::SeveralTerminalsMoved)
    );
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
                patch_row(term, ctx, &mut patched, frame_area(), row).expect("patching a row");
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
        super::terminal_damage::resync_host_patched_rows(term, &host_rows, &drawn)
            .expect("resync should succeed");
        drawn
    };

    assert_host_matches(harness.term.backend().buffer(), &expected, "host");
}

#[test]
fn planning_the_same_refresh_twice_agrees() {
    // The planner is pure with respect to the runner: nothing it inspects is consumed by
    // inspecting it, which is what lets a caller decide and then fall back without a second
    // refresh.
    let mut harness = build(PaneCfg::new(), SETUP);
    harness.screen.borrow_mut().process_bytes(b"\rX");
    let refresh = harness.runner.core.tree.refresh_live_terminals_detailed();
    let first = harness.runner.plan_terminal_damage(&refresh, frame_area());
    let second = harness.runner.plan_terminal_damage(&refresh, frame_area());
    assert_eq!(
        first.map(|plan| plan.rows),
        second.map(|plan| plan.rows),
        "planning twice should agree"
    );
}
