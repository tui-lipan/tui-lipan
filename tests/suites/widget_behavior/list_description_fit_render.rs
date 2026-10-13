use tui_lipan::prelude::*;
use tui_lipan::{CapturedFrame, TestBackend};

const W: u16 = 24;

#[derive(Clone)]
struct Rows {
    items: Vec<ListItem>,
}

impl Component for Rows {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .items(self.items.clone())
            .symbol_column(false)
            .width(Length::Px(W))
            .height(Length::Px(4))
            .focusable(false)
            .into()
    }
}

fn rows(items: Vec<ListItem>) -> Vec<String> {
    let mut backend = TestBackend::new(Rows { items });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: W,
        h: 4,
    });
    backend.render();
    let captured: CapturedFrame = backend.capture_frame();
    (0..4)
        .map(|y| (0..W).map(|x| captured.cell(x, y).symbol.clone()).collect())
        .collect()
}

fn row(item: ListItem) -> String {
    rows(vec![item]).remove(0)
}

#[test]
fn start_truncation_keeps_the_tail_of_the_description() {
    let row = row(ListItem::new("project")
        .description("feat/pricing-v2-final")
        .primary_truncate_description_first(true)
        .primary_description_truncation(ListTruncation::Start));

    assert_eq!(row, "project…pricing-v2-final");
}

#[test]
fn end_truncation_stays_the_default() {
    let row = row(ListItem::new("project")
        .description("feat/pricing-v2-final")
        .primary_truncate_description_first(true));

    assert_eq!(row, "projectfeat/pricing-v2-…");
}

#[test]
fn the_gap_holds_a_label_priority_description_off_the_label() {
    let row = row(ListItem::new("project")
        .description("feat/pricing-v2-final")
        .primary_truncate_description_first(true)
        .primary_description_truncation(ListTruncation::Start)
        .primary_description_gap(3));

    assert_eq!(row, "project   …cing-v2-final");
}

#[test]
fn the_gap_is_taken_from_the_label_when_the_description_has_priority() {
    let row = row(ListItem::new("a-very-long-branch-name")
        .description("new session")
        .primary_description_gap(3));

    assert_eq!(row, "a-very-lo…   new session");
}

#[test]
fn a_roomy_row_is_unchanged_by_the_gap() {
    let row = row(ListItem::new("a")
        .description("b")
        .primary_description_gap(3));

    assert_eq!(row, format!("a{}b", " ".repeat(usize::from(W) - 2)));
}

#[test]
fn a_description_cut_to_a_bare_ellipsis_is_hidden_with_its_gap() {
    // 20 columns of label leave one for the description after the gap: only `…` would fit.
    let label = "twenty-columns-label";
    let row = row(ListItem::new(label)
        .description("branch")
        .primary_truncate_description_first(true)
        .primary_description_gap(3));

    assert_eq!(row.trim_end(), label);
}

#[test]
fn without_a_gap_a_bare_ellipsis_still_renders() {
    // 23 columns of label leave exactly one for the description.
    let label = "twenty-three-cols-label";
    let row = row(ListItem::new(label)
        .description("branch")
        .primary_truncate_description_first(true));

    assert_eq!(row.trim_end(), format!("{label}…"));
}

#[test]
fn extra_lines_take_their_own_truncation_and_gap() {
    let rows = rows(vec![
        ListItem::new("title").line(
            ListItemLine::new("dir")
                .description("~/src/project/worktrees/long")
                .truncate_description_first(true)
                .description_truncation(ListTruncation::Start)
                .description_gap(3),
        ),
    ]);

    assert_eq!(rows[1], "dir   …ct/worktrees/long");
}

#[test]
fn a_wrapped_description_keeps_the_gap_on_its_first_line_only() {
    let rows = rows(vec![
        ListItem::new("Label")
            .description("alpha beta gamma delta epsilon")
            .primary_wrap_description(true)
            .primary_description_gap(3),
    ]);

    assert!(rows[0].starts_with("Label   "), "rows = {rows:#?}");
    assert!(!rows[0].contains('…'), "rows = {rows:#?}");
    assert!(
        rows[1].trim().len() > usize::from(W) - 8,
        "rows = {rows:#?}"
    );
}

#[test]
fn a_wide_grapheme_that_misses_the_first_line_wraps_whole() {
    // Three columns: `L`, a one-column gap, and one column left - too narrow for `你`, which
    // must move to the continuation rather than be cut to an ellipsis and hidden.
    let mut backend = TestBackend::new(Narrow);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 3,
        h: 2,
    });
    backend.render();
    let captured = backend.capture_frame();
    let rows: Vec<String> = (0..2)
        .map(|y| (0..3).map(|x| captured.cell(x, y).symbol.clone()).collect())
        .collect();

    assert_eq!(rows[0].trim_end(), "L", "rows = {rows:#?}");
    assert!(rows[1].contains('你'), "rows = {rows:#?}");
}

#[derive(Clone, Copy)]
struct Narrow;

impl Component for Narrow {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        List::new()
            .items([ListItem::new("L")
                .description("你")
                .primary_wrap_description(true)
                .primary_truncate_description_first(true)
                .primary_description_gap(1)])
            .symbol_column(false)
            .width(Length::Px(3))
            .height(Length::Px(2))
            .focusable(false)
            .into()
    }
}

fn long_label() -> ListItem {
    ListItem::new("codex-reviewing-focus").primary_description_gap(2)
}

#[test]
fn a_minimum_label_share_is_kept_before_the_description_is() {
    // Without it, a label-first cut leaves nothing of the label.
    assert_eq!(
        row(long_label().description("working-on-the-picker")),
        "…  working-on-the-picker"
    );
    // Half of the 24 columns stays with the label; the description takes the rest.
    assert_eq!(
        row(long_label()
            .description("working-on-the-picker")
            .primary_min_label_width(Length::Percent(50))),
        "codex-revie…  working-o…"
    );
}

#[test]
fn a_minimum_label_width_never_cuts_a_description_that_fits() {
    assert_eq!(
        row(long_label()
            .description("idle")
            .primary_min_label_width(Length::Percent(33))),
        "codex-reviewing-f…  idle"
    );
}

#[test]
fn a_short_label_leaves_the_rest_of_its_share_to_the_description() {
    assert_eq!(
        row(ListItem::new("codex")
            .primary_description_gap(2)
            .description("working-on-the-picker-row")
            .primary_min_label_width(Length::Percent(33))),
        "codex  working-on-the-p…"
    );
}

#[test]
fn a_minimum_label_width_takes_columns_or_the_whole_label() {
    assert_eq!(
        row(long_label()
            .description("working-on-the-picker")
            .primary_min_label_width(Length::Px(8))),
        "codex-r…  working-on-th…"
    );
    // `Auto` keeps the label whole, as cutting the description first would.
    assert_eq!(
        row(long_label()
            .description("working-on-the-picker")
            .primary_min_label_width(Length::Auto)),
        row(long_label()
            .description("working-on-the-picker")
            .primary_truncate_description_first(true)),
    );
}

#[test]
fn extra_lines_take_their_own_minimum_label_width() {
    let rows = rows(vec![
        ListItem::new("x").line(
            ListItemLine::new("codex-reviewing-focus")
                .description("working-on-the-picker")
                .description_gap(2)
                .min_label_width(Length::Percent(50)),
        ),
    ]);
    assert_eq!(rows[1], "codex-revie…  working-o…");
}
