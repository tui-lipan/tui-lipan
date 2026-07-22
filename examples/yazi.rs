//! A small Yazi-inspired file manager.
//!
//! Run with:
//!
//! ```text
//! cargo run --example yazi --features syntax-extra
//! ```

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use tui_lipan::language_from_path;
use tui_lipan::prelude::*;
use tui_lipan::style::FileIconPalette;
use tui_lipan::utils::{directory_icon_span, file_icon_span};

const MAX_PREVIEW_BYTES: u64 = 32 * 1024;
const MAX_PREVIEW_LINES: usize = 200;

struct Yazi;

struct Entry {
    name: String,
    path: PathBuf,
    is_dir: bool,
    len: u64,
    #[cfg(unix)]
    mode: u32,
}

struct Preview {
    body: String,
    language: Option<Arc<str>>,
    directory_entries: Option<Vec<Entry>>,
}

struct State {
    cwd: PathBuf,
    entries: Vec<Entry>,
    selected: usize,
    parent_entries: Vec<Entry>,
    parent_selected: usize,
    preview: Preview,
    split_weights: Vec<f32>,
    split_nonce: u32,
}

#[derive(Clone, Debug)]
enum Msg {
    SelectCurrent(usize),
    SelectParent(usize),
    Resize(SplitterResizeEvent),
}

impl State {
    fn new() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let entries = read_entries(&cwd);
        let selected = entries.iter().position(|entry| !entry.is_dir).unwrap_or(0);
        let preview = preview_for(entries.get(selected));
        let (parent_entries, parent_selected) = parent_pane_for(&cwd);
        Self {
            cwd,
            entries,
            selected,
            parent_entries,
            parent_selected,
            preview,
            split_weights: vec![1.0, 1.0, 2.0],
            split_nonce: 0,
        }
    }

    fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    fn refresh_preview(&mut self) {
        self.preview = preview_for(self.selected_entry());
    }

    fn refresh_parent_pane(&mut self) {
        let (parent_entries, parent_selected) = parent_pane_for(&self.cwd);
        self.parent_entries = parent_entries;
        self.parent_selected = parent_selected;
    }

    fn move_selection(&mut self, delta: i32) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        self.selected = if delta.is_negative() {
            self.selected.saturating_sub(delta.unsigned_abs() as usize)
        } else {
            self.selected.saturating_add(delta as usize).min(last)
        };
        self.refresh_preview();
    }

    fn open_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        if !entry.is_dir {
            return;
        }
        self.open_path(entry.path.clone());
    }

    fn open_parent_selection(&mut self, index: usize) {
        let Some(entry) = self.parent_entries.get(index) else {
            return;
        };
        if entry.is_dir {
            self.open_path(entry.path.clone());
        }
    }

    fn open_path(&mut self, path: PathBuf) {
        self.cwd = path;
        self.entries = read_entries(&self.cwd);
        self.selected = preferred_selection(&self.entries);
        self.refresh_parent_pane();
        self.refresh_preview();
    }

    fn go_parent(&mut self) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        let old_cwd = self.cwd.clone();
        self.cwd = parent;
        self.entries = read_entries(&self.cwd);
        self.selected = self
            .entries
            .iter()
            .position(|entry| entry.path == old_cwd)
            .unwrap_or(0);
        self.refresh_parent_pane();
        self.refresh_preview();
    }
}

impl Component for Yazi {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::new()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            KeyCode::Up | KeyCode::Char('k') => {
                ctx.state.move_selection(-1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                ctx.state.move_selection(1);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                ctx.state.open_selected();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Left | KeyCode::Backspace | KeyCode::Char('h') => {
                ctx.state.go_parent();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SelectCurrent(index) => {
                if index < ctx.state.entries.len() {
                    ctx.state.selected = index;
                    ctx.state.refresh_preview();
                }
            }
            Msg::SelectParent(index) => ctx.state.open_parent_selection(index),
            Msg::Resize(event) => {
                ctx.state.split_weights = event.weights;
                ctx.state.split_nonce = ctx.state.split_nonce.wrapping_add(1);
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let theme = Theme::catppuccin_mocha();
        let parent_select = ctx
            .link()
            .callback(|event: ListEvent| Msg::SelectParent(event.index));
        let current_select = ctx
            .link()
            .callback(|event: ListEvent| Msg::SelectCurrent(event.index));
        let resize = ctx.link().callback(Msg::Resize);

        VStack::new()
            .child(
                Text::new(short_path(&ctx.state.cwd))
                    .height(Length::Px(1))
                    .overflow(Overflow::Clip)
                    .style(
                        Style::new().fg(theme.accent.fg.map(Paint::color).unwrap_or(Color::White)),
                    ),
            )
            .child(
                Splitter::vertical()
                    .weights(ctx.state.split_weights.clone())
                    .weights_nonce(ctx.state.split_nonce)
                    .handle_style(Style::new().fg(Color::DarkGray))
                    .handle_hover_style(
                        Style::new().fg(theme.accent.fg.map(Paint::color).unwrap_or(Color::White)),
                    )
                    .handle_active_style(Style::new().fg(Color::White).bold())
                    .on_resize(resize)
                    .child(parent_list(
                        &ctx.state.parent_entries,
                        ctx.state.parent_selected,
                        &theme.file_icons,
                        parent_select,
                    ))
                    .child(file_list(
                        &ctx.state.entries,
                        ctx.state.selected,
                        &theme.file_icons,
                        current_select,
                    ))
                    .child(preview(&ctx.state.preview, &theme.file_icons)),
            )
            .child(footer(&ctx.state))
            .into()
    }
}

fn outer_pill_bg() -> Color {
    Color::rgb(137, 180, 250)
}

fn inner_pill_bg() -> Color {
    Color::rgb(180, 190, 250)
}

fn directory_label_color() -> Color {
    Color::rgb(100, 130, 190)
}

fn parent_list(
    entries: &[Entry],
    selected: usize,
    palette: &FileIconPalette,
    on_select: Callback<ListEvent>,
) -> Element {
    list(
        entries.iter().map(clone_entry).collect(),
        Some(selected),
        palette,
        Some(on_select),
    )
}

fn file_list(
    entries: &[Entry],
    selected: usize,
    palette: &FileIconPalette,
    on_select: Callback<ListEvent>,
) -> Element {
    list(
        entries.iter().map(clone_entry).collect(),
        Some(selected),
        palette,
        Some(on_select),
    )
}

fn list(
    entries: Vec<Entry>,
    selected: Option<usize>,
    palette: &FileIconPalette,
    on_select: Option<Callback<ListEvent>>,
) -> Element {
    let panel_bg = Color::rgb(30, 30, 46);
    let selection_bg = selected.and_then(|index| {
        entries.get(index).map(|entry| {
            if entry.is_dir {
                outer_pill_bg()
            } else {
                inner_pill_bg()
            }
        })
    });

    let mut list = List::new()
        .items(
            entries
                .into_iter()
                .enumerate()
                .map(|(index, entry)| list_item(&entry, palette, selected == Some(index)))
                .collect::<Vec<_>>(),
        )
        .selected(selected)
        .border(false)
        .padding(0)
        .focusable(false)
        .tab_stop(false)
        .selection_full_width(true)
        .empty_text("Directory is empty");
    if let Some(selection_bg) = selection_bg {
        list = list
            .selection_symbol(Some("\u{e0b6}"))
            .selection_symbol_right(Some("\u{e0b4}"))
            .selection_symbol_style(Style::new().fg(selection_bg).bg(panel_bg))
            .selection_style(Style::new().bg(selection_bg).fg(panel_bg));
    }
    if let Some(on_select) = on_select {
        list = list.on_select(on_select);
    }
    list.into()
}

fn list_item(entry: &Entry, palette: &FileIconPalette, selected: bool) -> ListItem {
    let icon = if entry.is_dir {
        directory_icon_span(selected, palette)
    } else {
        file_icon_span(&entry.name, palette)
    };
    let label = Span::new(format!(" {}", entry.name));
    let label = if entry.is_dir {
        label.fg(directory_label_color())
    } else {
        label
    };
    ListItem::from_spans([icon, label])
}

fn preview(preview: &Preview, palette: &FileIconPalette) -> Element {
    if let Some(entries) = &preview.directory_entries {
        return list(
            entries.iter().map(clone_entry).collect(),
            None,
            palette,
            None,
        );
    }

    DocumentView::new(preview.body.clone())
        .formatter(SyntectDocumentFormatter::new(preview.language.clone()))
        .wrap(false)
        .border(false)
        .padding(0)
        .scrollbar(true)
        .focusable(false)
        .into()
}

fn footer(state: &State) -> Element {
    let panel_bg = Color::rgb(30, 30, 46);
    let mode_bg = Color::rgb(137, 180, 250);
    let size_bg = Color::rgb(180, 190, 250);
    let percent_bg = Color::rgb(137, 180, 250);
    let position_bg = Color::rgb(180, 190, 250);
    let entry = state.selected_entry();
    let size = entry
        .map(|entry| compact_size(entry.len))
        .unwrap_or_else(|| "0B".to_string());
    let percent = if state.entries.is_empty() {
        0
    } else {
        ((state.selected + 1) * 100 / state.entries.len()).min(100)
    };
    let position = if state.entries.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", state.selected + 1, state.entries.len())
    };

    let left = vec![
        Span::new("\u{e0b6}").fg(mode_bg).bg(panel_bg),
        Span::new(" NOR ").fg(panel_bg).bg(mode_bg).bold(),
        Span::new("\u{e0b6}").fg(size_bg).bg(mode_bg),
        Span::new(format!(" {size} ")).fg(panel_bg).bg(size_bg),
        Span::new("\u{e0b4}").fg(size_bg).bg(panel_bg),
    ];
    let selection_name = state
        .selected_entry()
        .map(|entry| entry.name.as_str())
        .unwrap_or("-");
    let mut right = vec![Span::new(format!("{} ", permission_string(entry))).fg(Color::DarkGray)];
    add_connected_pills(
        &mut right,
        &format!("{percent}%"),
        percent_bg,
        &position,
        position_bg,
        panel_bg,
    );

    HStack::new()
        .height(Length::Px(1))
        .child(
            Text::from_spans(left)
                .height(Length::Px(1))
                .overflow(Overflow::Clip),
        )
        .child(
            Text::new(format!(" {selection_name}"))
                .width(Length::Flex(1))
                .height(Length::Px(1))
                .overflow(Overflow::Clip),
        )
        .child(
            Text::from_spans(right)
                .height(Length::Px(1))
                .overflow(Overflow::Clip),
        )
        .into()
}

fn add_connected_pills(
    spans: &mut Vec<Span>,
    first_label: &str,
    first_bg: Color,
    second_label: &str,
    second_bg: Color,
    panel_bg: Color,
) {
    spans.push(Span::new("\u{e0b6}").fg(first_bg).bg(panel_bg));
    spans.push(
        Span::new(format!(" {first_label} "))
            .fg(panel_bg)
            .bg(first_bg)
            .bold(),
    );
    spans.push(Span::new("\u{e0b6}").fg(second_bg).bg(first_bg));
    spans.push(
        Span::new(format!(" {second_label} "))
            .fg(panel_bg)
            .bg(second_bg)
            .bold(),
    );
    spans.push(Span::new("\u{e0b4}").fg(second_bg).bg(panel_bg));
}

fn short_path(path: &Path) -> String {
    let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
        return path.display().to_string();
    };
    let Ok(relative) = path.strip_prefix(&home) else {
        return path.display().to_string();
    };
    if relative.as_os_str().is_empty() {
        "~".to_string()
    } else {
        format!("~/{}", relative.display())
    }
}

fn preferred_selection(entries: &[Entry]) -> usize {
    entries.iter().position(|entry| !entry.is_dir).unwrap_or(0)
}

fn permission_string(entry: Option<&Entry>) -> String {
    let Some(entry) = entry else {
        return "----------".to_string();
    };

    #[cfg(unix)]
    {
        let mode = entry.mode;
        let mut result = String::with_capacity(10);
        result.push(if entry.is_dir { 'd' } else { '-' });
        for (bit, symbol) in [
            (0o400, 'r'),
            (0o200, 'w'),
            (0o100, 'x'),
            (0o040, 'r'),
            (0o020, 'w'),
            (0o010, 'x'),
            (0o004, 'r'),
            (0o002, 'w'),
            (0o001, 'x'),
        ] {
            result.push(if mode & bit != 0 { symbol } else { '-' });
        }
        result
    }

    #[cfg(not(unix))]
    {
        let _ = entry;
        "-rw-r--r--".to_string()
    }
}

fn compact_size(bytes: u64) -> String {
    match bytes {
        0..=999 => format!("{bytes}B"),
        1_000..=999_999 => format!("{:.1}K", bytes as f64 / 1_000.0),
        _ => format!("{:.1}M", bytes as f64 / 1_000_000.0),
    }
}

fn read_entries(path: &Path) -> Vec<Entry> {
    let mut entries = fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(std::result::Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            Some(Entry {
                name,
                is_dir: metadata.is_dir(),
                path,
                len: metadata.len(),
                #[cfg(unix)]
                mode: metadata.permissions().mode(),
            })
        })
        .collect::<Vec<_>>();

    entries.sort_by_key(|entry| (!entry.is_dir, entry.name.to_lowercase()));
    entries
}

fn parent_pane_for(cwd: &Path) -> (Vec<Entry>, usize) {
    let parent = cwd.parent().unwrap_or(cwd);
    let parent_entries = read_entries(parent);
    let parent_selected = parent_entries
        .iter()
        .position(|entry| entry.path == *cwd)
        .unwrap_or(0);
    (parent_entries, parent_selected)
}

fn clone_entry(entry: &Entry) -> Entry {
    Entry {
        name: entry.name.clone(),
        path: entry.path.clone(),
        is_dir: entry.is_dir,
        len: entry.len,
        #[cfg(unix)]
        mode: entry.mode,
    }
}

fn preview_for(entry: Option<&Entry>) -> Preview {
    let Some(entry) = entry else {
        return Preview {
            body: String::new(),
            language: None,
            directory_entries: Some(Vec::new()),
        };
    };

    if entry.is_dir {
        return Preview {
            body: String::new(),
            language: None,
            directory_entries: Some(read_entries(&entry.path)),
        };
    }

    match read_text_preview(&entry.path) {
        Ok(body) => Preview {
            language: language_from_path(&entry.path),
            body,
            directory_entries: None,
        },
        Err(message) => Preview {
            body: format!("{message}\n\n{}", file_status(Some(entry))),
            language: None,
            directory_entries: None,
        },
    }
}

fn read_text_preview(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(MAX_PREVIEW_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.contains(&0) {
        return Ok("Binary file".to_string());
    }
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    if text.lines().count() > MAX_PREVIEW_LINES {
        text = text
            .lines()
            .take(MAX_PREVIEW_LINES)
            .collect::<Vec<_>>()
            .join("\n");
        text.push_str("\n…");
    }
    Ok(text)
}

fn file_status(entry: Option<&Entry>) -> String {
    let Some(entry) = entry else {
        return "empty".to_string();
    };
    if entry.is_dir {
        return "directory".to_string();
    }
    match fs::metadata(&entry.path) {
        Ok(metadata) => format_size(metadata.len()),
        Err(_) => "unavailable".to_string(),
    }
}

fn format_size(bytes: u64) -> String {
    match bytes {
        0..=999 => format!("{bytes} B"),
        1_000..=999_999 => format!("{:.1} KB", bytes as f64 / 1_000.0),
        _ => format!("{:.1} MB", bytes as f64 / 1_000_000.0),
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Yazi")
        .theme(Theme::catppuccin_mocha())
        .mount(Yazi)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tui_lipan::core::event::{KeyMods, MouseButton, MouseEvent, MouseKind};
    use tui_lipan::{TestBackend, UiSnapshotOptions};

    #[test]
    fn renders_three_pane_layout() {
        let mut backend = TestBackend::new(Yazi);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        });
        backend.render();

        let snapshot = backend.capture_ui_snapshot_with_margin(0, 0, &UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        assert!(markdown.contains("NOR"));

        #[cfg(feature = "ui-snapshot-png")]
        if std::env::var_os("TUI_LIPAN_CAPTURE_YAZI").is_some() {
            std::fs::write("/tmp/tui-lipan-yazi.png", snapshot.to_png_default())
                .expect("write yazi preview capture");

            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 64,
                h: 20,
            });
            backend.render();
            let narrow = backend.capture_ui_snapshot();
            std::fs::write("/tmp/tui-lipan-yazi-narrow.png", narrow.to_png_default())
                .expect("write narrow yazi preview capture");

            if let Some(index) = backend
                .state()
                .entries
                .iter()
                .position(|entry| entry.is_dir)
            {
                {
                    let state = backend.state_mut();
                    state.selected = index;
                    state.refresh_preview();
                }
                backend.render();
                let directory = backend.capture_ui_snapshot();
                std::fs::write(
                    "/tmp/tui-lipan-yazi-directory.png",
                    directory.to_png_default(),
                )
                .expect("write directory yazi preview capture");
            }
        }
    }

    /// A fixture directory with a known shape, removed on drop.
    ///
    /// The preview assertion must not depend on the shape of whatever directory
    /// the test happens to run from.
    struct Fixture(PathBuf);

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn fixture() -> Fixture {
        let root = std::env::temp_dir().join(format!(
            "tui-lipan-yazi-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("child_dir")).expect("create fixture directory");
        fs::write(root.join("a_file.txt"), "hello\n").expect("write fixture file");
        Fixture(root)
    }

    #[test]
    fn directory_preview_pane_has_no_selection() {
        let fixture = fixture();
        let mut backend = TestBackend::new(Yazi);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        });

        {
            let state = backend.state_mut();
            state.open_path(fixture.0.clone());
            // Entries sort directories first, so index 0 is `child_dir`.
            assert!(state.entries[0].is_dir, "expected a directory first");
            state.selected = 0;
            state.refresh_preview();
        }
        backend.render();

        let snapshot = backend.capture_ui_snapshot_with_margin(0, 0, &UiSnapshotOptions::default());
        let lists: Vec<_> = snapshot
            .widgets
            .iter()
            .filter(|w| w.kind == tui_lipan::UiWidgetKind::List)
            .collect();
        assert_eq!(
            lists.len(),
            3,
            "expected parent, current, and preview lists, got {}",
            lists.len()
        );
        // Parent and current panes keep a cursor; the preview pane is read-only.
        assert!(lists[1].selected_index.is_some());
        assert_eq!(lists[2].selected_index, None);
    }

    #[test]
    fn mouse_selects_file_list_row() {
        let mut backend = TestBackend::new(Yazi);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        });
        backend.render();

        assert!(backend.state().entries.len() > 1);
        backend
            .send_mouse(MouseEvent {
                x: 30,
                y: 1,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .expect("dispatch file list click");

        assert_eq!(backend.state().selected, 0);
    }
}
