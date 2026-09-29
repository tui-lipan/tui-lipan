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
