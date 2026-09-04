//! Does clipping a live terminal to one physical row make the renderer paint one row?
//!
//! The incremental repaint design turns on this. If `render_regions` with a one-row region
//! reproduces exactly that row of a full render and disturbs nothing else, then a partial repaint
//! is the ordinary tree walk at a narrower clip - real focus chain, real theme, real selection -
//! and needs no second renderer and no cached render context. If it does not, the design has to
//! change before any of it is built.

use std::cell::RefCell;
use std::rc::Rc;

use super::capture_render::{
    CaptureInteraction, render_regions_over_seeded_buffer,
    render_regions_to_buffer_with_interaction, render_to_buffer_with_interaction,
};
use crate::core::component::{Component, Context, Update};
use crate::style::Rect;
use crate::widgets::{Terminal, TerminalScreen};

const COLS: u16 = 40;
const ROWS: u16 = 10;

struct Pane {
    screen: Rc<RefCell<TerminalScreen>>,
}

impl Component for Pane {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Terminal::new()
            .screen(crate::widgets::TerminalScreenHandle::new(Rc::clone(
                &self.screen,
            )))
            .into()
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

fn interaction() -> CaptureInteraction {
    CaptureInteraction {
        focused: None,
        hovered: None,
        mouse_pos: None,
    }
}

/// Build a backend whose tree holds one live terminal, already rendered once.
fn pane_backend() -> (crate::TestBackend<Pane>, Rc<RefCell<TerminalScreen>>) {
    let screen = Rc::new(RefCell::new(TerminalScreen::new(ROWS, COLS, 500)));
    screen
        .borrow_mut()
        .process_bytes(b"first line\r\nsecond line\r\nthird line\r\n");
    let mut backend = crate::TestBackend::new(Pane {
        screen: Rc::clone(&screen),
    });
    backend.set_viewport(viewport());
    backend.render();
    (backend, screen)
}

fn full_render(backend: &crate::TestBackend<Pane>) -> ratatui::buffer::Buffer {
    render_to_buffer_with_interaction(&backend.core.tree, viewport(), interaction(), 0, None).buffer
}

/// The discriminator. A one-row clip must reproduce that row of a full render exactly, and must
/// leave every other row of its destination untouched.
#[test]
fn a_one_row_clip_reproduces_that_row_of_a_full_render() {
    let (mut backend, screen) = pane_backend();
    let before = full_render(&backend);

    // Rewrite one row in place, the way a spinner does.
    screen.borrow_mut().process_bytes(b"\rX");
    backend.core.tree.refresh_live_terminals();

    let expected = full_render(&backend);
    let changed_rows: Vec<u16> = (0..ROWS)
        .filter(|&row| (0..COLS).any(|col| before[(col, row)] != expected[(col, row)]))
        .collect();
    assert_eq!(
        changed_rows.len(),
        1,
        "a one-character rewrite should move one row, moved {changed_rows:?}"
    );
    let row = changed_rows[0];

    let region = Rect {
        x: 0,
        y: row as i16,
        w: COLS,
        h: 1,
    };
    let patched = render_regions_to_buffer_with_interaction(
        &backend.core.tree,
        viewport(),
        interaction(),
        0,
        None,
        &[region],
    )
    .buffer;

    for col in 0..COLS {
        assert_eq!(
            patched[(col, row)],
            expected[(col, row)],
            "clipped row {row} column {col} differs from a full render"
        );
    }

    // The other half of the claim: the clip has to have *restricted* painting, not merely produced
    // the right answer for that row while painting everything. A row the region excluded should
    // still hold the default the buffer started with, which for a populated terminal differs from
    // what a full render puts there.
    let blank = ratatui::buffer::Cell::default();
    let untouched: Vec<u16> = (0..ROWS)
        .filter(|&other| other != row)
        .filter(|&other| (0..COLS).all(|col| patched[(col, other)] == blank))
        .collect();
    assert_eq!(
        untouched.len() as u16,
        ROWS - 1,
        "every row outside the region should be unpainted; painted rows were {:?}",
        (0..ROWS)
            .filter(|other| *other != row && !untouched.contains(other))
            .collect::<Vec<_>>()
    );
}

/// The production fast path's exact mechanism: borrow a full-size buffer as scratch, seed only the
/// damaged row from the retained frame, clip the render to that row.
///
/// Every other row is poisoned first. A render that reproduces the damaged row while leaving the
/// poison untouched proves the incremental path needs no full-buffer clone and no full-buffer
/// diff - only the damaged row is read, written, or compared.
#[test]
fn seeding_only_the_damaged_row_reproduces_a_full_render() {
    let (mut backend, screen) = pane_backend();
    let before = full_render(&backend);

    screen.borrow_mut().process_bytes(b"\rX");
    backend.core.tree.refresh_live_terminals();

    let expected = full_render(&backend);
    let row = (0..ROWS)
        .find(|&row| (0..COLS).any(|col| before[(col, row)] != expected[(col, row)]))
        .expect("the rewrite moved a row");

    let mut poison = ratatui::buffer::Cell::default();
    poison.set_symbol("\u{2593}");
    let seed_row = before.clone();
    let seed = |buf: &mut ratatui::buffer::Buffer| {
        for y in 0..ROWS {
            for x in 0..COLS {
                if y == row {
                    // What the retained frame holds for this row, which is what the fast path
                    // copies in before painting over it.
                    buf[(x, y)] = seed_row[(x, y)].clone();
                } else {
                    buf[(x, y)] = poison.clone();
                }
            }
        }
    };

    let region = Rect {
        x: 0,
        y: row as i16,
        w: COLS,
        h: 1,
    };
    let patched = render_regions_over_seeded_buffer(
        &backend.core.tree,
        viewport(),
        interaction(),
        &[region],
        &seed,
    )
    .buffer;

    for col in 0..COLS {
        assert_eq!(
            patched[(col, row)],
            expected[(col, row)],
            "seeded row {row} column {col} differs from a full render"
        );
    }
    for y in (0..ROWS).filter(|&y| y != row) {
        for x in 0..COLS {
            assert_eq!(
                patched[(x, y)],
                poison,
                "row {y} column {x} was touched outside the damaged region"
            );
        }
    }
}
