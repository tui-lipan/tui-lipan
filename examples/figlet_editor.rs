use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use tui_lipan::TextEditor;
use tui_lipan::prelude::*;

struct FigletEditor {
    characters: HashMap<char, Vec<String>>,
    current_char: char,
    editor: TextEditor,
    all_chars_editor: TextEditor,
    available_chars: Vec<String>,
    selected_index: Option<usize>,
    select_expanded: bool,
    active_tab: usize,
    font_name: String,
    font_height: usize,
    preview_text: String,
    preview_text_cursor: usize,
    preview_text_anchor: Option<usize>,
    status_message: String,
    show_export_modal: bool,
    export_filename: String,
    export_filename_cursor: usize,
    export_filename_anchor: Option<usize>,
    show_import_modal: bool,
    import_filename: String,
    import_filename_cursor: usize,
    import_filename_anchor: Option<usize>,
    show_help_modal: bool,
}

#[derive(Clone, Debug)]
enum Msg {
    EditorChanged(TextAreaEvent),
    AllCharsChanged(TextAreaEvent),
    PreviewTextChanged(InputEvent),
    ExportFilenameChanged(InputEvent),
    ImportFilenameChanged(InputEvent),
    SelectChar(usize),
    SelectChange(usize),
    SelectToggle(bool),
    PrevChar,
    NextChar,
    SaveCurrentChar,
    ClearCurrentChar,
    ShowExportModal,
    CloseExportModal,
    ConfirmExport,
    ShowImportModal,
    CloseImportModal,
    ConfirmImport,
    NewFont,
    ShowHelp,
    CloseHelp,
    ScrollTo(usize),
}

impl Default for FigletEditor {
    fn default() -> Self {
        let available_chars: Vec<String> = ('A'..='Z')
            .chain('a'..='z')
            .chain('0'..='9')
            .chain([
                '!', '@', '#', '$', '%', '^', '&', '*', '(', ')', '-', '+', '=', '[', ']', '{',
                '}', '|', '\\', ';', ':', '\'', '"', ',', '.', '<', '>', '/', '?', '`', '~', ' ',
            ])
            .map(|c| c.to_string())
            .collect();

        let initial_char = 'A';

        let mut characters = HashMap::new();
        characters.insert(
            'A',
            vec![
                "  ##  ".to_string(),
                " #  # ".to_string(),
                "######".to_string(),
                "#    #".to_string(),
                "#    #".to_string(),
            ],
        );
        characters.insert(
            'B',
            vec![
                "##### ".to_string(),
                "#    #".to_string(),
                "##### ".to_string(),
                "#    #".to_string(),
                "##### ".to_string(),
            ],
        );
        characters.insert(
            'C',
            vec![
                " #####".to_string(),
                "#     ".to_string(),
                "#     ".to_string(),
                "#     ".to_string(),
                " #####".to_string(),
            ],
        );

        let mut editor = TextEditor::new("");
        if let Some(lines) = characters.get(&initial_char) {
            editor.set_text(lines.join("\n"));
        }

        let mut all_chars_editor = TextEditor::new("");
        let preview_text = Self::build_all_chars_preview(&characters, "MyFont", 5);
        all_chars_editor.set_text(&preview_text);

        Self {
            characters,
            current_char: initial_char,
            editor,
            all_chars_editor,
            available_chars,
            selected_index: Some(0),
            select_expanded: false,
            active_tab: 0,
            font_name: "MyFont".to_string(),
            font_height: 5,
            preview_text: "ABC".to_string(),
            preview_text_cursor: 3,
            preview_text_anchor: None,
            status_message: "Welcome! Press ? for help".to_string(),
            show_export_modal: false,
            export_filename: "myfont.flf".to_string(),
            export_filename_cursor: 10,
            export_filename_anchor: None,
            show_import_modal: false,
            import_filename: "".to_string(),
            import_filename_cursor: 0,
            import_filename_anchor: None,
            show_help_modal: false,
        }
    }
}

impl FigletEditor {
    fn build_all_chars_preview(
        characters: &HashMap<char, Vec<String>>,
        font_name: &str,
        height: usize,
    ) -> String {
        let mut preview = String::new();
        preview.push_str(&format!("Font: {}\n", font_name));
        preview.push_str(&format!("Characters: {}\n", characters.len()));
        preview.push_str(&format!("Height: {} lines\n", height));
        preview.push_str(&"=".repeat(60));
        preview.push('\n');

        let mut chars: Vec<char> = characters.keys().copied().collect();
        chars.sort();

        for ch in chars {
            if let Some(lines) = characters.get(&ch) {
                preview.push_str(&format!("\n'{}':\n", ch));
                for line in lines {
                    preview.push_str("  ");
                    preview.push_str(line);
                    preview.push('\n');
                }
            }
        }

        if characters.is_empty() {
            preview.push_str("\nNo characters created yet.\n");
            preview.push_str("Use the Editor tab to create characters!\n");
        }

        preview
    }

    fn update_all_chars_preview(&mut self) {
        let preview =
            Self::build_all_chars_preview(&self.characters, &self.font_name, self.font_height);
        self.all_chars_editor.set_text(&preview);
    }

    fn save_current_char(&mut self, ctx: &mut Context<Self>) {
        let content = self.editor.text();
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        if lines.iter().all(|l| l.is_empty()) {
            self.status_message = "Cannot save empty character!".to_string();
            ctx.toast().push(Toast::new("Cannot save empty character!"));
            return;
        }

        self.characters.insert(self.current_char, lines);
        self.status_message = format!("Saved character '{}'", self.current_char);
        ctx.toast().push(Toast::new(format!(
            "Saved character '{}'!",
            self.current_char
        )));
    }

    fn clear_current_char(&mut self, ctx: &mut Context<Self>) {
        self.editor.set_text("");
        self.editor.set_cursor(0);
        self.editor.set_anchor(None);
        self.status_message = "Editor cleared".to_string();
        ctx.toast().push(Toast::new("Cleared editor"));
    }

    fn load_char(&mut self, ch: char) {
        self.current_char = ch;
        if let Some(lines) = self.characters.get(&ch) {
            let content = lines.join("\n");
            self.editor.set_text(&content);
        } else {
            self.editor.set_text("");
        }
        self.editor.set_cursor(0);
        self.editor.set_anchor(None);

        if let Some(idx) = self
            .available_chars
            .iter()
            .position(|s| s == &ch.to_string())
        {
            self.selected_index = Some(idx);
        }
    }

    fn prev_char(&mut self, ctx: &mut Context<Self>) {
        self.save_current_char(ctx);
        let chars: Vec<char> = self.characters.keys().copied().collect();
        if let Some(pos) = chars.iter().position(|&c| c == self.current_char)
            && pos > 0
        {
            self.load_char(chars[pos - 1]);
        }
    }

    fn next_char(&mut self, ctx: &mut Context<Self>) {
        self.save_current_char(ctx);
        let chars: Vec<char> = self.characters.keys().copied().collect();
        if let Some(pos) = chars.iter().position(|&c| c == self.current_char)
            && pos + 1 < chars.len()
        {
            self.load_char(chars[pos + 1]);
        }
    }

    fn generate_figlet_font(&self) -> String {
        let mut font = String::new();

        let max_len = self
            .characters
            .values()
            .flat_map(|lines| lines.iter().map(|l| l.len()))
            .max()
            .unwrap_or(15);

        font.push_str(&format!("flf2a$ {} 0 {} -1 1\n", self.font_height, max_len));
        font.push_str(&format!("{}\n", self.font_name));

        for code in 32..=126u8 {
            let ch = code as char;
            let lines = self.characters.get(&ch);

            for row in 0..self.font_height {
                if let Some(char_lines) = lines {
                    if row < char_lines.len() {
                        let line = &char_lines[row];
                        let trimmed = line.trim_end();
                        font.push_str(trimmed);
                        if row < self.font_height - 1 {
                            font.push('$');
                        }
                    } else if row < self.font_height - 1 {
                        font.push('$');
                    }
                } else if row < self.font_height - 1 {
                    font.push('$');
                }
                font.push('%');
            }
            font.push('\n');
        }

        font
    }

    fn export_to_file(&mut self, ctx: &mut Context<Self>) {
        if self.characters.is_empty() {
            ctx.toast().push(Toast::new("No characters to export!"));
            self.status_message = "Nothing to export".to_string();
            return;
        }

        let font = self.generate_figlet_font();
        let path = PathBuf::from(&self.export_filename);

        match fs::write(&path, &font) {
            Ok(_) => {
                self.status_message = format!("Exported to {}", self.export_filename);
                ctx.toast().push(Toast::new(format!(
                    "Exported {} characters to {}!",
                    self.characters.len(),
                    self.export_filename
                )));
                self.show_export_modal = false;
            }
            Err(e) => {
                self.status_message = format!("Export failed: {}", e);
                ctx.toast()
                    .push(Toast::new(format!("Failed to export: {}", e)));
            }
        }
    }

    fn import_from_file(&mut self, ctx: &mut Context<Self>) {
        let path = PathBuf::from(&self.import_filename);

        match fs::read_to_string(&path) {
            Ok(content) => {
                if let Err(e) = self.parse_figlet_font(&content) {
                    self.status_message = format!("Import failed: {}", e);
                    ctx.toast()
                        .push(Toast::new(format!("Failed to parse font: {}", e)));
                } else {
                    self.status_message = format!("Imported from {}", self.import_filename);
                    ctx.toast().push(Toast::new(format!(
                        "Imported {} characters!",
                        self.characters.len()
                    )));
                    self.show_import_modal = false;
                    self.update_all_chars_preview();
                    self.load_char(self.current_char);
                }
            }
            Err(e) => {
                self.status_message = format!("Import failed: {}", e);
                ctx.toast()
                    .push(Toast::new(format!("Failed to read file: {}", e)));
            }
        }
    }

    fn parse_figlet_font(&mut self, content: &str) -> std::result::Result<(), String> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return Err("Empty file".to_string());
        }

        let header = lines[0];
        if !header.starts_with("flf2a$") {
            return Err("Invalid FIGlet header".to_string());
        }

        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.len() < 2 {
            return Err("Invalid header format".to_string());
        }

        let height: usize = parts[1].parse().map_err(|_| "Invalid height")?;
        self.font_height = height;

        if lines.len() > 1 {
            self.font_name = lines[1].trim().to_string();
        }

        self.characters.clear();
        let mut line_idx = 2;

        for code in 32..=126u8 {
            let ch = code as char;
            let mut char_lines = Vec::new();

            for _ in 0..height {
                if line_idx >= lines.len() {
                    break;
                }

                let line = lines[line_idx];
                let cleaned = line
                    .trim_end_matches('%')
                    .trim_end_matches('$')
                    .replace('$', "");
                char_lines.push(cleaned);
                line_idx += 1;
            }

            if !char_lines.iter().all(|l| l.is_empty()) {
                self.characters.insert(ch, char_lines);
            }
        }

        Ok(())
    }

    fn new_font(&mut self, ctx: &mut Context<Self>) {
        self.characters.clear();
        self.font_name = "NewFont".to_string();
        self.current_char = 'A';
        self.editor.set_text("");
        self.status_message = "New font created".to_string();
        ctx.toast().push(Toast::new("Created new font"));
        self.update_all_chars_preview();
    }

    fn render_bigtext_preview(&self) -> Element {
        if self.characters.is_empty() {
            return rsx! {
                Center {
                    Text {
                        content: "Create some characters first!",
                        style: Style::new().fg(Color::Yellow).dim(),
                    },
                }
            };
        }

        let custom_figlet = self.generate_figlet_font();

        rsx! {
            ScrollView {
                scrollbar: true,
                VStack {
                    gap: 2,
                    BigText {
                        text: self.preview_text.clone(),
                        custom_figlet: custom_figlet,
                        style: Style::new().fg(Color::Cyan),
                    },
                },
            }
        }
    }
}

impl Component for FigletEditor {
    type Message = Msg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
        self.update_all_chars_preview();
        None
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let accent = Color::Cyan;
        let success = Color::Green;
        let warning = Color::Yellow;

        let main_content = if self.active_tab == 0 {
            rsx! {
                HStack {
                    gap: 2,
                    height: Length::Flex(1),
                    VStack {
                        gap: 1,
                        width: Length::Flex(1),
                        Frame {
                            title: "Select Character",
                            border: true,
                            height: Length::Auto,
                            Select {
                                options: self.available_chars.clone(),
                                selected: self.selected_index,
                                placeholder: "Select a character...",
                                expanded: self.select_expanded,
                                on_toggle: ctx.link().callback(Msg::SelectToggle),
                                on_change: ctx.link().callback(Msg::SelectChange),
                                on_select: ctx.link().callback(Msg::SelectChar),
                            },
                        },
                        Frame {
                            title: format!("Edit '{}'", self.current_char),
                            border: true,
                            height: Length::Flex(1),
                            VStack {
                                gap: 1,
                                Text {
                                    content: "Draw ASCII art (use # or any chars):",
                                    style: Style::new().fg(accent).dim(),
                                },
                                TextArea {
                                    value: self.editor.text().to_owned(),
                                    cursor: self.editor.cursor(),
                                    anchor: self.editor.anchor(),
                                    border: true,
                                    line_numbers: true,
                                    min_line_number_width: 2,
                                    scrollbar: true,
                                    height: Length::Flex(1),
                                    on_change: ctx.link().callback(Msg::EditorChanged),
                                    on_scroll_to: ctx.link().callback(Msg::ScrollTo),
                                },
                            },
                        },
                    },
                    VStack {
                        gap: 1,
                        width: Length::Flex(1),
                        Frame {
                            title: "Font Info",
                            border: true,
                            height: Length::Auto,
                            VStack {
                                gap: 1,
                                Text { content: format!("Name: {}", self.font_name) },
                                Text { content: format!("Characters: {}", self.characters.len()) },
                                Text { content: format!("Height: {}", self.font_height) },
                            },
                        },
                        Frame {
                            title: "Live Preview",
                            border: true,
                            height: Length::Flex(1),
                            VStack {
                                gap: 1,
                                Text {
                                    content: "Test text:",
                                    style: Style::new().fg(accent).dim(),
                                },
                                Input {
                                    value: self.preview_text.clone(),
                                    cursor: self.preview_text_cursor,
                                    anchor: self.preview_text_anchor,
                                    on_change: ctx.link().callback(Msg::PreviewTextChanged),
                                },
                                self.render_bigtext_preview(),
                            },
                        },
                    },
                }
            }
        } else {
            rsx! {
                Frame {
                    title: "All Characters",
                    border: true,
                    height: Length::Flex(1),
                    TextArea {
                        value: self.all_chars_editor.text().to_owned(),
                        cursor: self.all_chars_editor.cursor(),
                        anchor: self.all_chars_editor.anchor(),
                        border: false,
                        read_only: true,
                        line_numbers: false,
                        scrollbar: true,
                        on_change: ctx.link().callback(Msg::AllCharsChanged),
                        on_scroll_to: ctx.link().callback(Msg::ScrollTo),
                    },
                }
            }
        };

        let _export_modal = if self.show_export_modal {
            Some(rsx! {
                Modal::new()
                    .title("Export Font to File")
                    .on_close(ctx.link().callback(|_| Msg::CloseExportModal))
                    .child(
                        VStack::new()
                            .gap(2)
                            .child(Text::new("Enter filename for the FIGlet font (.flf):"))
                            .child(
                                Input::new(self.export_filename.clone())
                                    .cursor(self.export_filename_cursor)
                                    .anchor(self.export_filename_anchor)
                                    .placeholder("myfont.flf")
                                    .on_change(ctx.link().callback(Msg::ExportFilenameChanged)),
                            )
                            .child(
                                HStack::new()
                                    .gap(1)
                                    .child(
                                        Button::new("Export")
                                            .style(Style::new().fg(success))
                                            .on_click(ctx.link().callback(|_| Msg::ConfirmExport)),
                                    )
                                    .child(
                                        Button::new("Cancel")
                                            .on_click(ctx.link().callback(|_| Msg::CloseExportModal)),
                                    ),
                            ),
                    )
            })
        } else {
            None
        };

        let _import_modal = if self.show_import_modal {
            Some(rsx! {
                Modal::new()
                    .title("Import Font from File")
                    .on_close(ctx.link().callback(|_| Msg::CloseImportModal))
                    .child(
                        VStack::new()
                            .gap(2)
                            .child(Text::new("Enter path to FIGlet font file (.flf):"))
                            .child(
                                Input::new(self.import_filename.clone())
                                    .cursor(self.import_filename_cursor)
                                    .anchor(self.import_filename_anchor)
                                    .placeholder("font.flf")
                                    .on_change(ctx.link().callback(Msg::ImportFilenameChanged)),
                            )
                            .child(
                                HStack::new()
                                    .gap(1)
                                    .child(
                                        Button::new("Import")
                                            .style(Style::new().fg(accent))
                                            .on_click(ctx.link().callback(|_| Msg::ConfirmImport)),
                                    )
                                    .child(
                                        Button::new("Cancel")
                                            .on_click(ctx.link().callback(|_| Msg::CloseImportModal)),
                                    ),
                            ),
                    )
            })
        } else {
            None
        };

        let _help_modal = if self.show_help_modal {
            Some(rsx! {
                Modal::new()
                    .title("Keyboard Shortcuts Help")
                    .on_close(ctx.link().callback(|_| Msg::CloseHelp))
                    .child(
                        VStack::new()
                            .gap(1)
                            .child(Text::new("Navigation:").style(Style::new().bold()))
                            .child(Text::new("  Tab        - Next character"))
                            .child(Text::new("  Shift+Tab  - Previous character"))
                            .child(Text::new("  Ctrl+1     - Switch to Editor tab"))
                            .child(Text::new("  Ctrl+2     - Switch to Preview tab"))
                            .child(Text::new(""))
                            .child(Text::new("Actions:").style(Style::new().bold()))
                            .child(Text::new("  Ctrl+S     - Save current character"))
                            .child(Text::new("  Ctrl+E     - Export font to file"))
                            .child(Text::new("  Ctrl+I     - Import font from file"))
                            .child(Text::new("  Ctrl+N     - Create new font"))
                            .child(Text::new(""))
                            .child(Text::new("General:").style(Style::new().bold()))
                            .child(Text::new("  ?          - Show this help"))
                            .child(Text::new("  q          - Quit application"))
                            .child(Text::new(""))
                            .child(
                                HStack::new()
                                    .child(
                                        Button::new("Close")
                                            .on_click(ctx.link().callback(|_| Msg::CloseHelp)),
                                    ),
                            ),
                    )
            })
        } else {
            None
        };

        rsx! {
            Frame {
                title: "FIGlet Font Editor",
                padding: 1,
                tab_titles: vec!["Editor".to_string(), "Preview".to_string()],
                active_tab: self.active_tab,
                VStack {
                    gap: 1,
                    HStack {
                        gap: 1,
                        height: Length::Auto,
                        Button {
                            label: "New".to_string(),
                            on_click: ctx.link().callback(|_| Msg::NewFont),
                        },
                        Button {
                            label: "Import".to_string(),
                            on_click: ctx.link().callback(|_| Msg::ShowImportModal),
                        },
                        Button {
                            label: "Export".to_string(),
                            style: Style::new().fg(success),
                            on_click: ctx.link().callback(|_| Msg::ShowExportModal),
                        },
                        Button {
                            label: format!("Save '{}'", self.current_char),
                            style: Style::new().fg(accent),
                            on_click: ctx.link().callback(|_| Msg::SaveCurrentChar),
                        },
                        Button {
                            label: "Clear".to_string(),
                            style: Style::new().fg(warning),
                            on_click: ctx.link().callback(|_| Msg::ClearCurrentChar),
                        },
                        if self.active_tab == 0 {
                            HStack {
                                gap: 1,
                                Button {
                                    label: "< Prev".to_string(),
                                    on_click: ctx.link().callback(|_| Msg::PrevChar),
                                },
                                Button {
                                    label: "Next >".to_string(),
                                    on_click: ctx.link().callback(|_| Msg::NextChar),
                                },
                            },
                        },
                        Spacer {},
                        Button {
                            label: "? Help".to_string(),
                            on_click: ctx.link().callback(|_| Msg::ShowHelp),
                        },
                    },
                    main_content,
                    StatusBar {
                        style: Style::new().bg(Color::DarkGray),
                        left: Text::new(self.status_message.clone()),
                        right: Text::new(
                            format!(
                                "{} | {} chars | Ctrl+1/2=Tabs | ?=Help | q=Quit", if self.active_tab == 0 {
                                "Editor" } else { "Preview" }, self.characters.len()
                            ),
                        ),
                    },
                },
            }
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::EditorChanged(ev) => {
                self.editor.set_text(ev.value.to_string());
                self.editor.set_cursor(ev.cursor);
                self.editor.set_anchor(ev.anchor);
                Update::full()
            }
            Msg::AllCharsChanged(ev) => {
                self.all_chars_editor.set_text(ev.value.to_string());
                self.all_chars_editor.set_cursor(ev.cursor);
                self.all_chars_editor.set_anchor(ev.anchor);
                Update::full()
            }
            Msg::PreviewTextChanged(ev) => {
                self.preview_text = ev.value.to_string();
                self.preview_text_cursor = ev.cursor;
                self.preview_text_anchor = ev.anchor;
                Update::full()
            }
            Msg::ExportFilenameChanged(ev) => {
                self.export_filename = ev.value.to_string();
                self.export_filename_cursor = ev.cursor;
                self.export_filename_anchor = ev.anchor;
                Update::full()
            }
            Msg::ImportFilenameChanged(ev) => {
                self.import_filename = ev.value.to_string();
                self.import_filename_cursor = ev.cursor;
                self.import_filename_anchor = ev.anchor;
                Update::full()
            }
            Msg::SelectChar(idx) => {
                self.selected_index = Some(idx);
                self.select_expanded = false;
                if let Some(ch_str) = self.available_chars.get(idx)
                    && let Some(ch) = ch_str.chars().next()
                {
                    self.save_current_char(ctx);
                    self.load_char(ch);
                }
                Update::full()
            }
            Msg::SelectChange(idx) => {
                self.selected_index = Some(idx);
                if let Some(ch_str) = self.available_chars.get(idx)
                    && let Some(ch) = ch_str.chars().next()
                {
                    self.save_current_char(ctx);
                    self.load_char(ch);
                }
                Update::full()
            }
            Msg::SelectToggle(expanded) => {
                self.select_expanded = expanded;
                Update::full()
            }
            Msg::PrevChar => {
                self.prev_char(ctx);
                Update::full()
            }
            Msg::NextChar => {
                self.next_char(ctx);
                Update::full()
            }
            Msg::SaveCurrentChar => {
                self.save_current_char(ctx);
                Update::full()
            }
            Msg::ClearCurrentChar => {
                self.clear_current_char(ctx);
                Update::full()
            }
            Msg::ShowExportModal => {
                self.show_export_modal = true;
                Update::full()
            }
            Msg::CloseExportModal => {
                self.show_export_modal = false;
                Update::full()
            }
            Msg::ConfirmExport => {
                self.export_to_file(ctx);
                Update::full()
            }
            Msg::ShowImportModal => {
                self.show_import_modal = true;
                Update::full()
            }
            Msg::CloseImportModal => {
                self.show_import_modal = false;
                Update::full()
            }
            Msg::ConfirmImport => {
                self.import_from_file(ctx);
                Update::full()
            }
            Msg::NewFont => {
                self.new_font(ctx);
                Update::full()
            }
            Msg::ShowHelp => {
                self.show_help_modal = true;
                Update::full()
            }
            Msg::CloseHelp => {
                self.show_help_modal = false;
                Update::full()
            }
            Msg::ScrollTo(_offset) => Update::none(),
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') if !key.mods.ctrl && !key.mods.alt && !key.mods.shift => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('s') if key.mods.ctrl => {
                self.save_current_char(ctx);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('e') if key.mods.ctrl => {
                self.show_export_modal = true;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('i') if key.mods.ctrl => {
                self.show_import_modal = true;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('n') if key.mods.ctrl => {
                self.new_font(ctx);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('1') if key.mods.ctrl => {
                self.active_tab = 0;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') if key.mods.ctrl => {
                self.active_tab = 1;
                self.update_all_chars_preview();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Tab if self.active_tab == 0 => {
                self.next_char(ctx);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::BackTab if self.active_tab == 0 => {
                self.prev_char(ctx);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('?') => {
                self.show_help_modal = true;
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}

fn main() -> Result<()> {
    let mut editor = FigletEditor::default();
    editor.update_all_chars_preview();

    App::new()
        .title("tui-lipan - FIGlet Font Editor")
        .mount(editor)
        .run()
}
