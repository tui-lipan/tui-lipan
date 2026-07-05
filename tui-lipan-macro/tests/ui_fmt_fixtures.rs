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
