use std::fs;
use std::path::PathBuf;

#[path = "../src/format_common.rs"]
mod format_common;
#[path = "../src/rsx_ast.rs"]
mod rsx_ast;
#[path = "../src/rsx_format.rs"]
mod rsx_format;
#[path = "../src/ui_ast.rs"]
mod ui_ast;
#[path = "../src/ui_format.rs"]
mod ui_format;

#[test]
fn formats_all_ui_fixtures() {
    for case in fixture_cases() {
        let input = fs::read_to_string(case.input_path).unwrap();
        let expected = fs::read_to_string(case.expected_path).unwrap();

        let formatted = ui_format::format_file_contents(&input)
            .unwrap()
            .unwrap_or_else(|| input.clone());

        assert_eq!(formatted, expected, "fixture `{}` mismatch", case.name);
    }
}

#[test]
fn second_pass_is_noop_for_all_expected_fixtures() {
    for case in fixture_cases() {
        let expected = fs::read_to_string(case.expected_path).unwrap();
        let second_pass = ui_format::format_file_contents(&expected).unwrap();

        assert!(
            second_pass.is_none(),
            "fixture `{}` should be idempotent on second pass",
            case.name
        );
    }
}

#[test]
fn fixtures_do_not_reference_removed_api_names() {
    for case in fixture_cases() {
        for path in [&case.input_path, &case.expected_path] {
            let source = fs::read_to_string(path).unwrap();
            if let Some(removed) = find_removed_api(&source) {
                panic!(
                    "fixture `{}` still references removed API `{removed}`",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn removed_api_matcher_uses_whole_token_boundaries() {
    let valid_fixture = r#"
fn view() {
    let el = rsx! {
        Frame {
            title: "Root",
            title_alignment: Align::Center,
            column_spacing: 2,
            legend_spacing: 1,
        }
    };
    let styled = ui! {
        Frame::new().title("Root").focus_style(Style::default()) => {
            Text::new("ok"),
        }
    };
    let _ = (my_header_left, alignment::Center, Update::layout_hash());
}
"#;
    assert_eq!(find_removed_api(valid_fixture), None);
    assert_eq!(
        find_removed_api("rsx! { Frame { title_alignment: Align::Center } }"),
        None
    );
    assert_eq!(
        find_removed_api("rsx! { Grid { column_spacing: 2, legend_spacing: 1 } }"),
        None
    );
    assert_eq!(
        find_removed_api("Frame::new().focus_style(Style::default())"),
        None
    );

    assert_eq!(
        find_removed_api("rsx! { VStack { alignment: Align::Center } }"),
        Some("alignment:")
    );
    assert_eq!(
        find_removed_api("rsx! { VStack { alignment : Align::Center } }"),
        Some("alignment:")
    );
    assert_eq!(
        find_removed_api(
            "rsx! { VStack { title_alignment: Align::Start, alignment: Align::End } }"
        ),
        Some("alignment:")
    );
    assert_eq!(
        find_removed_api("rsx! { VStack { spacing: 2 } }"),
        Some("spacing:")
    );
    assert_eq!(
        find_removed_api("ui! { Frame::new().header_left(\"Root\") => { Text::new(\"a\") } }"),
        Some("header_left")
    );
    assert_eq!(
        find_removed_api("rsx! { Frame { header_left: \"Root\" } }"),
        Some("header_left")
    );
    assert_eq!(
        find_removed_api("FrameLabel::new(\"a\").focused_style(Style::default())"),
        Some("focused_style")
    );
    assert_eq!(
        find_removed_api("Update::render().with_command(cmd)"),
        Some(".with_command(")
    );
    assert_eq!(find_removed_api("Update::layout()"), Some("Update::layout"));
}

enum RemovedApi {
    PropOrMethod(&'static str),
    Method(&'static str),
    RsxField(&'static str),
    Path(&'static str),
}

impl RemovedApi {
    fn label(&self) -> &'static str {
        match *self {
            Self::PropOrMethod(name) | Self::Path(name) => name,
            Self::Method("with_command") => ".with_command(",
            Self::Method(name) => name,
            Self::RsxField("alignment") => "alignment:",
            Self::RsxField("spacing") => "spacing:",
            Self::RsxField(name) => name,
        }
    }

    fn matches(&self, source: &str) -> bool {
        match *self {
            Self::PropOrMethod(name) => {
                matches_method(source, name) || matches_rsx_field(source, name)
            }
            Self::Method(name) => matches_method(source, name),
            Self::RsxField(name) => matches_rsx_field(source, name),
            Self::Path(path) => matches_path(source, path),
        }
    }
}

fn find_removed_api(source: &str) -> Option<&'static str> {
    const REMOVED_APIS: &[RemovedApi] = &[
        RemovedApi::PropOrMethod("header_left"),
        RemovedApi::PropOrMethod("header_center"),
        RemovedApi::PropOrMethod("header_right"),
        RemovedApi::PropOrMethod("header_style"),
        RemovedApi::PropOrMethod("header_padding"),
        RemovedApi::PropOrMethod("header_content"),
        RemovedApi::PropOrMethod("focused_style"),
        RemovedApi::PropOrMethod("focused_header_style"),
        RemovedApi::PropOrMethod("focused_footer_style"),
        RemovedApi::PropOrMethod("focused_description_style"),
        RemovedApi::PropOrMethod("focused_path"),
        RemovedApi::Path("Update::layout"),
        RemovedApi::Method("with_command"),
        RemovedApi::RsxField("alignment"),
        RemovedApi::RsxField("spacing"),
    ];

    REMOVED_APIS
        .iter()
        .find(|removed| removed.matches(source))
        .map(RemovedApi::label)
}

fn is_ident_char(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn has_leading_boundary(bytes: &[u8], start: usize) -> bool {
    start == 0 || !is_ident_char(bytes[start - 1])
}

fn has_trailing_boundary(bytes: &[u8], end: usize) -> bool {
    end == bytes.len() || !is_ident_char(bytes[end])
}

fn next_non_ws(bytes: &[u8], mut idx: usize) -> Option<usize> {
    while idx < bytes.len() && bytes[idx].is_ascii_whitespace() {
        idx += 1;
    }
    (idx < bytes.len()).then_some(idx)
}

fn matches_method(source: &str, name: &str) -> bool {
    let bytes = source.as_bytes();
    let mut offset = 0;
    while let Some(rel) = source[offset..].find(name) {
        let start = offset + rel;
        let end = start + name.len();
        let preceded_by_dot =
            start > 0 && bytes[start - 1] == b'.' && (start < 2 || bytes[start - 2] != b'.');
        let followed_by_paren = next_non_ws(bytes, end).is_some_and(|i| bytes[i] == b'(');
        if preceded_by_dot && followed_by_paren {
            return true;
        }
        offset = end;
    }
    false
}

fn matches_rsx_field(source: &str, name: &str) -> bool {
    let bytes = source.as_bytes();
    let mut offset = 0;
    while let Some(rel) = source[offset..].find(name) {
        let start = offset + rel;
        let end = start + name.len();
        let followed_by_colon = next_non_ws(bytes, end)
            .is_some_and(|i| bytes[i] == b':' && bytes.get(i + 1) != Some(&b':'));
        if has_leading_boundary(bytes, start) && followed_by_colon {
            return true;
        }
        offset = end;
    }
    false
}

fn matches_path(source: &str, path: &str) -> bool {
    let bytes = source.as_bytes();
    let mut offset = 0;
    while let Some(rel) = source[offset..].find(path) {
        let start = offset + rel;
        let end = start + path.len();
        if has_leading_boundary(bytes, start) && has_trailing_boundary(bytes, end) {
            return true;
        }
        offset = end;
    }
    false
}

struct FixtureCase {
    name: String,
    input_path: PathBuf,
    expected_path: PathBuf,
}

fn fixture_cases() -> Vec<FixtureCase> {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ui-fmt");
    let mut cases = Vec::new();

    for entry in fs::read_dir(&fixture_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path.is_file() {
            continue;
        }

        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };

        if !file_name.ends_with(".input.rs") {
            continue;
        }

        let name = file_name.trim_end_matches(".input.rs").to_string();
        let expected_path = fixture_dir.join(format!("{name}.expected.rs"));
        assert!(
            expected_path.exists(),
            "missing expected fixture for `{name}`"
        );

        cases.push(FixtureCase {
            name,
            input_path: path,
            expected_path,
        });
    }

    cases.sort_by(|a, b| a.name.cmp(&b.name));
    cases
}
