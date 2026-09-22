//! Effects that read their backdrop composite an `EffectScope` over whatever lay beneath it.
//!
//! The backdrop is the lower `ZStack` layer as it stood before the scope's children painted, so a
//! reveal can show it through cell by cell - which a post-processing effect, seeing only the
//! scope's own output, never could.

use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

/// Reveals the backdrop in every column left of `split`, keeping the scope's content elsewhere.
#[derive(Debug)]
struct Wipe {
    split: i16,
    reads_backdrop: bool,
}

impl CellEffect for Wipe {
    fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

    fn uses_backdrop(&self) -> bool {
        self.reads_backdrop
    }

    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        if ctx.x - ctx.bounds.x < self.split {
            *cell = backdrop.clone();
        }
    }
}

/// The same wipe, routed through the prepared path.
#[derive(Debug)]
struct PreparedWipe(i16);

impl CellEffect for PreparedWipe {
    fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

    fn uses_backdrop(&self) -> bool {
        true
    }

    fn prepare(&self, _ctx: &EffectPrepareContext) -> Option<Box<dyn PreparedCellEffect>> {
        Some(Box::new(Wipe {
            split: self.0,
            reads_backdrop: true,
        }))
    }
}

impl PreparedCellEffect for Wipe {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        CellEffect::apply(self, cell, ctx);
    }

    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        CellEffect::apply_with_backdrop(self, cell, backdrop, ctx);
    }
}

/// Reveals the backdrop from `split` onward, so the frontier can bisect a wide grapheme.
#[derive(Debug)]
struct ReverseWipe(i16);

impl CellEffect for ReverseWipe {
    fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

    fn uses_backdrop(&self) -> bool {
        true
    }

    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        if ctx.x - ctx.bounds.x >= self.0 {
            *cell = backdrop.clone();
        }
    }
}

#[derive(Clone, Copy)]
enum Effect {
    Wipe { split: i16, reads_backdrop: bool },
    Prepared(i16),
}

struct Layers(Effect);

impl Component for Layers {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let scope = EffectScope::new();
        let scope = match self.0 {
            Effect::Wipe {
                split,
                reads_backdrop,
            } => scope.custom_effect(Wipe {
                split,
                reads_backdrop,
            }),
            Effect::Prepared(split) => scope.custom_effect(PreparedWipe(split)),
        };
        ZStack::new()
            .child(Text::new("AAAAAAAA").style(Style::new().bg(Color::rgb(10, 20, 30))))
            .child(scope.child(Text::new("BBBBBBBB").style(Style::new().bg(Color::rgb(200, 0, 0)))))
            .into()
    }
}

fn render(effect: Effect) -> CapturedFrame {
    let mut backend = TestBackend::new(Layers(effect));
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 8,
        h: 1,
    });
    backend.render();
    backend.capture_frame()
}

fn row(frame: &CapturedFrame) -> String {
    (0..8).map(|x| frame.cell(x, 0).symbol.as_str()).collect()
}

#[test]
fn a_backdrop_effect_reveals_the_layer_beneath_its_scope() {
    let frame = render(Effect::Wipe {
        split: 3,
        reads_backdrop: true,
    });
    assert_eq!(row(&frame), "AAABBBBB");
    // The whole cell comes through, not just its symbol.
    assert_eq!(frame.cell(0, 0).bg, Color::rgb(10, 20, 30));
    assert_eq!(frame.cell(5, 0).bg, Color::rgb(200, 0, 0));
}

#[test]
fn a_prepared_backdrop_effect_reveals_the_layer_beneath_its_scope() {
    assert_eq!(row(&render(Effect::Prepared(5))), "AAAAABBB");
}

#[test]
fn an_effect_that_does_not_ask_for_its_backdrop_never_sees_one() {
    assert_eq!(
        row(&render(Effect::Wipe {
            split: 3,
            reads_backdrop: false,
        })),
        "BBBBBBBB"
    );
}

struct WideLayers {
    backdrop: &'static str,
    incoming: &'static str,
    reverse: bool,
}

impl Component for WideLayers {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let scope = if self.reverse {
            EffectScope::new().custom_effect(ReverseWipe(1))
        } else {
            EffectScope::new().custom_effect(Wipe {
                split: 1,
                reads_backdrop: true,
            })
        };
        ZStack::new()
            .child(Text::new(self.backdrop))
            .child(scope.child(Text::new(self.incoming)))
            .into()
    }
}

fn render_wide(backdrop: &'static str, incoming: &'static str, reverse: bool) -> CapturedFrame {
    let mut backend = TestBackend::new(WideLayers {
        backdrop,
        incoming,
        reverse,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 1,
    });
    backend.render();
    backend.capture_frame()
}

#[test]
fn a_wipe_clears_a_backdrop_cell_beneath_an_incoming_wide_grapheme() {
    let frame = render_wide("ABx", "你x", true);

    assert_eq!(frame.cell(0, 0).symbol, "你");
    assert_eq!(frame.cell(1, 0).symbol, " ");
    assert_eq!(frame.cell(2, 0).symbol, "x");
}

#[test]
fn a_wipe_clears_an_incoming_cell_beneath_a_backdrop_wide_grapheme() {
    let frame = render_wide("你x", "ABx", false);

    assert_eq!(frame.cell(0, 0).symbol, "你");
    assert_eq!(frame.cell(1, 0).symbol, " ");
    assert_eq!(frame.cell(2, 0).symbol, "x");
}
