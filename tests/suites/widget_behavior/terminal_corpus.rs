use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{Value, json};
use tui_lipan::prelude::TerminalScreen;
use tui_lipan::{CapturedFrame, CellModifiers, Color, TerminalRenderSnapshot};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    rows: u16,
    cols: u16,
    scrollback: usize,
    description: String,
    source: String,
    #[serde(default)]
    ops: Vec<Operation>,
    #[serde(default)]
    known_defects: Vec<KnownDefect>,
    /// Reviewed differences of a second engine from the Alacritty golden.
    #[serde(default)]
    differences: Vec<Difference>,
}

/// A field (exact JSON pointer) a cross-engine comparison ignores. Never applies to the golden
/// or chunk-split checks.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Difference {
    path: String,
    reason: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Write { bytes: [usize; 2] },
    Resize { rows: u16, cols: u16 },
    Scroll { delta: i64 },
    Dump,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KnownDefect {
    engine: String,
    check: CheckKind,
    reason: String,
    seed: u64,
    path: String,
    expected_contains: String,
    actual_contains: String,
    #[serde(default)]
    differences: Vec<KnownDifference>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KnownDifference {
    path: String,
    expected: Value,
    actual: Value,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum CheckKind {
    /// The engine's output changes when the same bytes arrive in different chunks.
    ChunkSplit,
    /// A second engine's output differs from the Alacritty golden (`seed` is unused, write `0`).
    #[cfg_attr(not(feature = "terminal-ghostty"), allow(dead_code))]
    Engine,
}

fn check_split(
    expected: &Value,
    actual: &Value,
    defect: Option<&KnownDefect>,
) -> Result<(), String> {
    let difference = first_difference(expected, actual, "");
    match (difference, defect) {
        (None, None) => Ok(()),
        (None, Some(defect)) => Err(format!(
            "known defect no longer reproduces: {}",
            defect.reason
        )),
        (Some(diff), None) => Err(diff),
        (Some(_), Some(defect)) => check_known_defect(expected, actual, defect),
    }
}

fn check_known_defect(
    expected: &Value,
    actual: &Value,
    defect: &KnownDefect,
) -> Result<(), String> {
    let mut normalized = expected.clone();
    let field = normalized
        .pointer_mut(&defect.path)
        .ok_or_else(|| format!("missing defect path {}", defect.path))?;
    let text = field
        .as_str()
        .filter(|text| text.contains(&defect.expected_contains))
        .ok_or_else(|| format!("expected text does not reproduce {}", defect.reason))?;
    *field = Value::String(text.replacen(&defect.expected_contains, &defect.actual_contains, 1));
    for difference in &defect.differences {
        if normalized.pointer(&difference.path) != Some(&difference.expected)
            || actual.pointer(&difference.path) != Some(&difference.actual)
        {
            return Err(format!("unexpected defect field {}", difference.path));
        }
        *normalized.pointer_mut(&difference.path).unwrap() = difference.actual.clone();
    }
    first_difference(&normalized, actual, "").map_or(Ok(()), Err)
}

fn attributes(modifiers: &CellModifiers) -> Vec<String> {
    let mut attrs: Vec<_> = [
        (modifiers.bold, "bold"),
        (modifiers.dim, "dim"),
        (modifiers.italic, "italic"),
        (modifiers.reverse, "reverse"),
        (modifiers.strikethrough, "strikethrough"),
    ]
    .into_iter()
    .filter(|(set, _)| *set)
    .map(|(_, name)| name.to_owned())
    .collect();
    if let Some(underline) = modifiers.underline {
        attrs.push(format!("underline:{underline:?}"));
    }
    attrs
}

fn row(frame: &CapturedFrame, snapshot: &TerminalRenderSnapshot, index: usize) -> Value {
    let cells = frame.row(index as u16);
    let text = cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect::<String>();
    let mut value = json!({"text": text.trim_end_matches(' ')});
    let extra_cells: Vec<_> = cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.symbol.chars().count() != 1)
        .map(|(col, cell)| json!([col, cell.symbol]))
        .collect();
    let styles: Vec<_> = frame
        .row_runs(index as u16)
        .iter()
        .filter_map(|run| {
            let mut style = serde_json::Map::new();
            for (key, color) in [
                ("fg", run.fg),
                ("bg", run.bg),
                ("underline_color", run.underline_color),
            ] {
                if color != Color::Reset {
                    style.insert(key.into(), json!(format!("{color:?}")));
                }
            }
            let attrs = attributes(&run.modifiers);
            if !attrs.is_empty() {
                style.insert("attrs".into(), json!(attrs));
            }
            if style.is_empty() {
                return None;
            }
            style.insert("col".into(), json!(run.x));
            style.insert("width".into(), json!(run.width));
            Some(Value::Object(style))
        })
        .collect();
    let links: Vec<_> = snapshot
        .hyperlinks
        .iter()
        .filter(|link| link.row == index)
        .map(|link| json!({"uri": link.uri.as_ref(), "start": link.start_col, "end": link.end_col}))
        .collect();
    for (key, entries) in [("cells", extra_cells), ("styles", styles), ("links", links)] {
        if !entries.is_empty() {
            value[key] = json!(entries);
        }
    }
    if snapshot.wrapped_rows.get(index).copied().unwrap_or(false) {
        value["wrapped"] = json!(true);
    }
    value
}

fn dump(
    screen: &mut TerminalScreen,
    responses: &mut Vec<Vec<u8>>,
    clipboard: &mut Vec<String>,
) -> Value {
    responses.extend(screen.drain_responses());
    clipboard.extend(
        screen
            .drain_clipboard_events()
            .iter()
            .map(|event| format!("{event:?}")),
    );
    let offset = screen.scrollback_offset();
    screen.set_scrollback(0);
    let live = screen.render_snapshot();
    let history = screen.total_scrollback_rows();
    let mut rows = Vec::new();
    for off in (1..=history).rev() {
        screen.set_scrollback(off);
        let snapshot = screen.render_snapshot();
        rows.push(row(&screen.capture_frame(), &snapshot, 0));
    }
    screen.set_scrollback(0);
    let frame = screen.capture_frame();
    for index in 0..usize::from(frame.height) {
        rows.push(row(&frame, &live, index));
    }
    while rows.last() == Some(&json!({"text": ""})) {
        rows.pop();
    }
    screen.set_scrollback(offset);
    json!({"size": [frame.width, frame.height], "rows": rows, "history_rows": history, "scroll_offset": offset,
        "cursor": {"row": live.cursor_row, "col": live.cursor_col, "visible": live.cursor_visible,
                   "shape": format!("{:?}", live.cursor_shape), "blinking": live.cursor_blinking},
        "title": screen.title(), "alternate": screen.is_alternate_screen(),
        "key_modes": format!("{:?}", screen.key_modes()), "mouse_mode": format!("{:?}", screen.mouse_mode()),
        "semantic": format!("{:?}", screen.semantic_state()), "marks": format!("{:?}", screen.semantic_marks()),
        "status": format!("{:?}", screen.program_status_snapshot()), "responses": responses,
        "clipboard": clipboard, "bells": screen.bell_count()})
}

fn run(
    fixture: &Fixture,
    bytes: &[u8],
    factory: impl Fn(u16, u16, usize) -> TerminalScreen,
    seed: Option<u64>,
) -> Value {
    let mut screen = factory(fixture.rows, fixture.cols, fixture.scrollback);
    screen.set_program_status_enabled(true);
    screen.set_xtversion_enabled(true);
    screen.set_terminal_identity("terminal-corpus");
    let mut rng = seed.unwrap_or(0);
    let mut responses = Vec::new();
    let mut clipboard = Vec::new();
    let mut dumps = Vec::new();
    let ops = if fixture.ops.is_empty() {
        vec![
            Operation::Write {
                bytes: [0, bytes.len()],
            },
            Operation::Dump,
        ]
    } else {
        // Borrow explicit operations without cloning them.
        Vec::new()
    };
    let ops = if fixture.ops.is_empty() {
        &ops
    } else {
        &fixture.ops
    };
    for op in ops {
        match *op {
            Operation::Write {
                bytes: [start, end],
            } => {
                assert!(
                    start <= end && end <= bytes.len(),
                    "invalid write range {start}..{end}"
                );
                feed_pieces(&mut screen, &bytes[start..end], seed, &mut rng);
            }
            Operation::Resize { rows, cols } => screen.resize(rows, cols),
            Operation::Scroll { delta } => screen.set_scrollback(
                screen
                    .scrollback_offset()
                    .saturating_add_signed(delta as isize),
            ),
            Operation::Dump => dumps.push(dump(&mut screen, &mut responses, &mut clipboard)),
        }
    }
    assert!(!dumps.is_empty(), "fixture must contain a dump operation");
    json!({"dumps": dumps})
}

fn feed_pieces(screen: &mut TerminalScreen, bytes: &[u8], seed: Option<u64>, rng: &mut u64) {
    let mut pos = 0;
    while pos < bytes.len() {
        let n = if seed.is_some() {
            *rng ^= *rng << 13;
            *rng ^= *rng >> 7;
            *rng ^= *rng << 17;
            1 + (*rng as usize % 17)
        } else {
            bytes.len() - pos
        };
        let next = bytes.len().min(pos.saturating_add(n));
        screen.process_bytes(&bytes[pos..next]);
        pos = next;
    }
}

fn fixtures() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/terminal-corpus");
    let mut paths: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty());
    paths
}

fn first_difference<'a>(left: &'a Value, right: &'a Value, path: &str) -> Option<String> {
    match (left, right) {
        (Value::Object(a), Value::Object(b)) if a.keys().eq(b.keys()) => a
            .iter()
            .find_map(|(key, value)| first_difference(value, &b[key], &format!("{path}/{key}"))),
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(index, (a, b))| first_difference(a, b, &format!("{path}/{index}"))),
        _ if left == right => None,
        _ => Some(format!("{path}: {left} != {right}")),
    }
}

fn load_fixture(path: &Path) -> Fixture {
    let fixture: Fixture = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
    assert!(fixture.rows > 0 && fixture.cols > 0, "{}", path.display());
    assert!(!fixture.description.trim().is_empty(), "{}", path.display());
    assert!(!fixture.source.trim().is_empty(), "{}", path.display());
    fixture
}

fn split_seeds(fixture: &Fixture, engine: &str) -> Vec<u64> {
    let mut seeds = vec![1, 0x1234_5678, 0xdead_beef];
    for defect in &fixture.known_defects {
        for field in [
            &defect.engine,
            &defect.reason,
            &defect.path,
            &defect.expected_contains,
            &defect.actual_contains,
        ] {
            assert!(
                !field.trim().is_empty(),
                "known defect field must not be empty"
            );
        }
        assert_ne!(defect.expected_contains, defect.actual_contains);
        if defect.engine == engine
            && defect.check == CheckKind::ChunkSplit
            && !seeds.contains(&defect.seed)
        {
            seeds.push(defect.seed);
        }
    }
    seeds
}

fn defect_for_seed<'a>(fixture: &'a Fixture, engine: &str, seed: u64) -> Option<&'a KnownDefect> {
    let mut defects = fixture.known_defects.iter().filter(|defect| {
        defect.engine == engine && defect.check == CheckKind::ChunkSplit && defect.seed == seed
    });
    let defect = defects.next();
    assert!(
        defects.next().is_none(),
        "duplicate known defect for {engine} seed {seed}"
    );
    defect
}

fn check_golden(path: &Path, expected: &Value) -> Result<(), String> {
    let golden = path.with_extension("alacritty.json");
    let formatted = format!("{}\n", serde_json::to_string_pretty(expected).unwrap());
    if std::env::var_os("TUI_LIPAN_BLESS_TERMINAL_CORPUS").is_some() {
        fs::write(&golden, formatted).unwrap();
    } else if fs::read_to_string(&golden).unwrap() != formatted {
        return Err(format!("golden mismatch: {}", golden.display()));
    }
    Ok(())
}

fn check_fixture(path: &Path, failures: &mut Vec<String>) {
    let fixture = load_fixture(path);
    let bytes = fs::read(path.with_extension("vt")).unwrap();
    let expected = run(&fixture, &bytes, TerminalScreen::new, None);
    if let Err(failure) = check_golden(path, &expected) {
        failures.push(failure);
    }
    for seed in split_seeds(&fixture, "alacritty") {
        let defect = defect_for_seed(&fixture, "alacritty", seed);
        let actual = run(&fixture, &bytes, TerminalScreen::new, Some(seed));
        if let Err(diff) = check_split(&expected, &actual, defect) {
            failures.push(format!("{} seed {seed}: {diff}", path.display()));
        }
    }
}

#[test]
fn terminal_corpus_alacritty() {
    let mut failures = Vec::new();
    for path in fixtures() {
        check_fixture(&path, &mut failures);
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn known_defect_must_match_and_still_fail() {
    let defect = KnownDefect {
        engine: "alacritty".into(),
        check: CheckKind::ChunkSplit,
        reason: "lost space".into(),
        seed: 38,
        path: "/text".into(),
        expected_contains: "é é".into(),
        actual_contains: "éé".into(),
        differences: Vec::new(),
    };
    let expected = json!({"text": "é é"});
    let broken = json!({"text": "éé"});
    assert!(check_split(&expected, &broken, Some(&defect)).is_ok());
    assert!(check_split(&expected, &expected, Some(&defect)).is_err());
    assert!(check_split(&expected, &broken, None).is_err());
    assert!(check_split(&expected, &json!({"text": "other"}), Some(&defect)).is_err());
    assert!(check_split(&expected, &json!({"text": "éé", "other": 1}), Some(&defect)).is_err());
}

#[test]
fn compact_dump_keeps_styled_blank_rows_and_complex_cells() {
    let mut screen = TerminalScreen::new(3, 8, 0);
    screen.process_bytes("e\u{301}字".as_bytes());
    let capture = dump(&mut screen, &mut Vec::new(), &mut Vec::new());
    assert_eq!(capture["rows"].as_array().unwrap().len(), 1);
    assert_eq!(
        capture["rows"][0]["cells"],
        json!([[0, "e\u{301}"], [2, ""]])
    );
    screen.process_bytes(b"\x1b[3;1H\x1b[41m ");
    let styled = dump(&mut screen, &mut Vec::new(), &mut Vec::new());
    assert_eq!(styled["rows"].as_array().unwrap().len(), 3);
    assert_eq!(styled["rows"][2]["text"], "");
    assert_eq!(styled["rows"][2]["styles"][0]["width"], 1);
}

/// Null the allowlisted fields in both values. Every path must exist in the golden.
#[cfg(feature = "terminal-ghostty")]
fn without_differences(
    golden: &mut Value,
    actual: &mut Value,
    differences: &[Difference],
) -> Result<(), String> {
    for difference in differences {
        if difference.reason.trim().is_empty() {
            return Err(format!("difference {} needs a reason", difference.path));
        }
        let field = golden
            .pointer_mut(&difference.path)
            .filter(|_| !difference.path.is_empty())
            .ok_or_else(|| format!("difference path {} is not in the golden", difference.path))?;
        *field = Value::Null;
        if let Some(field) = actual.pointer_mut(&difference.path) {
            *field = Value::Null;
        }
    }
    Ok(())
}

/// Compares every fixture through the Ghostty engine with Alacritty's golden, minus the reviewed
/// `[[differences]]`, and checks that Ghostty's own output does not depend on chunking.
#[cfg(feature = "terminal-ghostty")]
#[test]
fn terminal_corpus_ghostty() {
    use tui_lipan::TerminalEngineKind;

    let engine = "ghostty";
    let ghostty = |rows, cols, scrollback| {
        TerminalScreen::with_engine(TerminalEngineKind::Ghostty, rows, cols, scrollback)
    };
    let mut failures = Vec::new();
    for path in fixtures() {
        let fixture = load_fixture(&path);
        let bytes = fs::read(path.with_extension("vt")).unwrap();
        let mut golden: Value = serde_json::from_str(
            &fs::read_to_string(path.with_extension("alacritty.json")).unwrap(),
        )
        .unwrap();
        let actual = run(&fixture, &bytes, ghostty, None);
        let mut compared = actual.clone();
        if let Err(failure) = without_differences(&mut golden, &mut compared, &fixture.differences)
        {
            failures.push(format!("{}: {failure}", path.display()));
            continue;
        }
        let defect = fixture
            .known_defects
            .iter()
            .find(|defect| defect.engine == engine && defect.check == CheckKind::Engine);
        if let Err(diff) = check_split(&golden, &compared, defect) {
            failures.push(format!("{} vs alacritty: {diff}", path.display()));
        }
        for seed in split_seeds(&fixture, engine) {
            let defect = defect_for_seed(&fixture, engine, seed);
            let split = run(&fixture, &bytes, ghostty, Some(seed));
            if let Err(diff) = check_split(&actual, &split, defect) {
                failures.push(format!("{} seed {seed}: {diff}", path.display()));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
