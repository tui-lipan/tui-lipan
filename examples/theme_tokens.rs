//! Theme token showcase: renders the syntax, diff, and markdown palettes of a
//! theme so the derived semantic styles can be judged visually.
//!
//! Run with:
//!   cargo run --example theme_tokens --features ui-snapshot-png

use std::fs;
use std::path::PathBuf;

use tui_lipan::prelude::*;
use tui_lipan::{TestBackend, UiSnapshotOptions};

struct Themed {
    theme: Theme,
}

impl Component for Themed {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}
    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        ThemeProvider::new(self.theme.clone())
            .child(tui_lipan::child::<Screen, _>(|| Screen, ()))
            .into()
    }
}

struct Screen;

impl Component for Screen {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}
    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let t = ctx.theme();
        let s = t.syntax;
        let d = t.diff;
        let doc = t.document;

        let sp = |text: &str, style: Style| Span::new(text.to_string()).style(style);
        let line = |spans: Vec<Span>| Text::from_spans(spans);

        // ── Syntax ──────────────────────────────────────────────────────────
        let code = VStack::new()
            .padding(1)
            .child(line(vec![sp(
                "// derive a full theme from three tokens",
                s.comment,
            )]))
            .child(line(vec![
                sp("pub fn ", s.keyword),
                sp("brand", s.function),
                sp("() -> ", s.operator),
                sp("Theme", s.type_name),
                sp(" {", s.variable),
            ]))
            .child(line(vec![
                sp("    let ", s.keyword),
                sp("accent ", s.variable),
                sp("= ", s.operator),
                sp("Color", s.type_name),
                sp("::", s.operator),
                sp("hex", s.builtin),
                sp("(", s.variable),
                sp("0xC084FC", s.number),
                sp(");", s.variable),
            ]))
            .child(line(vec![
                sp("    ThemePalette", s.type_name),
                sp("::", s.operator),
                sp("new", s.function),
                sp("(", s.variable),
                sp("text", s.parameter),
                sp(", ", s.variable),
                sp("bg", s.parameter),
                sp(", accent)", s.variable),
                sp(".", s.operator),
                sp("into", s.function),
                sp("()", s.variable),
            ]))
            .child(line(vec![
                sp("    ", s.variable),
                sp("// ", s.comment),
                sp("\"violet brand\"", s.string),
                sp(" → ", s.operator),
                sp("true", s.constant),
            ]))
            .child(line(vec![sp("}", s.variable)]));
        let source = Frame::new()
            .title("Syntax")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(Style::new().bg(t.surface.panel))
            .child(code);

        // ── Diff ────────────────────────────────────────────────────────────
        let diff_line = |marker: &str,
                         marker_style: Style,
                         num: &str,
                         num_style: Style,
                         text: &str,
                         row: Style| {
            line(vec![
                sp(marker, marker_style),
                sp(num, num_style),
                sp(text, row),
            ])
        };
        let diff = VStack::new()
            .padding(1)
            .child(diff_line(
                "  ",
                d.context_line_number,
                "41 ",
                d.context_line_number,
                "let muted = text.blend(bg, 0.42);",
                d.context,
            ))
            .child(diff_line(
                "- ",
                d.removed_marker,
                "42 ",
                d.removed_line_number,
                "let panel = bg.lighten_by(0.04);",
                d.removed,
            ))
            .child(diff_line(
                "+ ",
                d.added_marker,
                "42 ",
                d.added_line_number,
                "let panel = bg.elevate(0.07);",
                d.added,
            ))
            .child(diff_line(
                "+ ",
                d.added_marker,
                "43 ",
                d.added_line_number,
                "let menu = bg.elevate(0.12);",
                d.added,
            ))
            .child(diff_line(
                "  ",
                d.context_line_number,
                "44 ",
                d.context_line_number,
                "let border = chrome(bg);",
                d.context,
            ));
        let diff_frame = Frame::new()
            .title("Diff")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(Style::new().bg(t.surface.panel))
            .child(diff);

        let left = VStack::new()
            .gap(1)
            .width(Length::Flex(1))
            .child(source)
            .child(diff_frame);

        // ── Markdown ────────────────────────────────────────────────────────
        let md = VStack::new()
            .padding(1)
            .gap(0)
            .child(line(vec![sp("tui-lipan", doc.heading_styles[0])]))
            .child(line(vec![sp("Theming", doc.heading_styles[1])]))
            .child(line(vec![sp("Surfaces", doc.heading_styles[2])]))
            .child(Spacer::new().height(Length::Px(1)))
            .child(line(vec![
                sp("Use ", t.primary),
                sp("Theme::lipan()", doc.code_inline),
                sp(" for the brand look.", t.primary),
            ]))
            .child(line(vec![
                sp("Docs: ", t.primary),
                sp("https://lipan.rs/theme", doc.link),
            ]))
            .child(Spacer::new().height(Length::Px(1)))
            .child(line(vec![
                sp("│ ", doc.blockquote_bar),
                sp("Great defaults on every app.", t.muted),
            ]))
            .child(line(vec![
                sp("• ", doc.list_item),
                sp("luminance-aware surfaces", t.primary),
            ]))
            .child(line(vec![
                sp("1. ", doc.list_enumeration),
                sp("pick three core colors", t.primary),
            ]));
        let markdown = Frame::new()
            .title("Markdown")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .style(Style::new().bg(t.surface.panel))
            .width(Length::Flex(1))
            .child(md);

        HStack::new()
            .padding(1)
            .gap(1)
            .child(left)
            .child(markdown)
            .into()
    }
}

fn out_dir() -> PathBuf {
    PathBuf::from("/tmp/tui-lipan-theme-audit")
}

fn render(name: &str, theme: Theme) {
    let mut backend = TestBackend::new(Themed { theme });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 96,
        h: 26,
    });
    backend.render();
    let snapshot = backend.capture_ui_snapshot_with_options(&UiSnapshotOptions::default());
    let dir = out_dir();
    fs::create_dir_all(&dir).unwrap();
    #[cfg(feature = "ui-snapshot-png")]
    {
        let path = dir.join(format!("tokens_{name}.png"));
        fs::write(&path, snapshot.to_png_default()).unwrap();
        println!("Wrote {}", path.display());
    }
    #[cfg(not(feature = "ui-snapshot-png"))]
    {
        let path = dir.join(format!("tokens_{name}.md"));
        fs::write(&path, snapshot.to_markdown()).unwrap();
        println!("Wrote {}", path.display());
    }
}

fn main() {
    render("lipan", Theme::lipan());
    render("default", Theme::default());
    render("gruvbox", Theme::gruvbox());
}
