use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tui_lipan::{
    App, DraggableTabBarOverflow, InlineHeight, InlineStartupPolicy, ScrollTarget,
    ScrollWheelBehavior, ScrollWheelConfig, SurfaceMode, TextAreaLineNumberMode,
    TextAreaSentinelClickEvent, TextAreaSentinelClickKind,
};

#[test]
fn root_exports_include_text_area_sentinel_click_types() {
    let _ = std::mem::size_of::<TextAreaSentinelClickEvent>();
    let _ = std::mem::size_of::<TextAreaSentinelClickKind>();
}

#[test]
fn root_exports_include_text_area_line_number_mode() {
    let _ = TextAreaLineNumberMode::Relative;
}

#[test]
fn root_exports_include_draggable_tab_bar_overflow() {
    let _ = DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 8 };
}

#[test]
fn root_exports_include_scroll_target() {
    let _ = ScrollTarget::Bottom;
}

#[test]
fn root_exports_include_scroll_wheel_types() {
    let _ = ScrollWheelBehavior::smooth(ScrollWheelConfig::default());
}

#[test]
fn prelude_exports_layered_key_dispatch_types() {
    use tui_lipan::prelude::*;
    let _ = FrameworkAction::Quit;
    let _ = FrameworkKeymap::default().unbind(FrameworkAction::Quit);
    let _ = UserKeymapPolicy::Disabled;
    let _ = KeyDispatchPolicy::AppCommandsFirst;
    let _ = TerminalKeyPolicy::AppCommandsThenTerminal;
    let _ = CommandConflictPolicy::HighestPriority;
    let _ = ChordMismatchPolicy::ForwardPrefixAndCurrent;
}

#[test]
fn root_exports_include_key_dispatch_policy_types() {
    let _ = tui_lipan::FrameworkAction::Quit;
    let _ = tui_lipan::FrameworkKeymap::default().unbind(tui_lipan::FrameworkAction::Quit);
    let _ = tui_lipan::UserKeymapPolicy::Disabled;
    let _ = tui_lipan::KeyDispatchPolicy::AppCommandsFirst;
    let _ = tui_lipan::TerminalKeyPolicy::TerminalOnly;
}

#[test]
fn named_inline_modes_are_constructible() {
    let _ = SurfaceMode::Fullscreen;
    let _ = SurfaceMode::InlineEphemeral {
        height: InlineHeight::Fixed(8),
    };
    let _ = SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(12),
        startup: InlineStartupPolicy::PreserveHost,
    };

    let _ = App::new().surface(SurfaceMode::InlineEphemeral {
        height: InlineHeight::Fixed(4),
    });
    let _ = App::new().inline_ephemeral(4);
    let _ = App::new().inline_transcript(4);
    let _ = App::new().inline_transcript_with_startup(4, InlineStartupPolicy::ClearHost);
}

#[test]
fn inline_auto_height_modes_are_constructible() {
    let _ = SurfaceMode::InlineEphemeral {
        height: InlineHeight::auto(),
    };
    let _ = SurfaceMode::InlineTranscript {
        height: InlineHeight::auto_capped(12),
        startup: InlineStartupPolicy::PreserveHost,
    };

    // Plain row counts keep working through `Into<InlineHeight>`.
    assert_eq!(InlineHeight::from(8), InlineHeight::Fixed(8));

    let _ = App::new().inline_ephemeral(InlineHeight::auto());
    let _ = App::new().inline_transcript(InlineHeight::auto_capped(10));
    let _ = App::new()
        .inline_transcript_with_startup(InlineHeight::auto(), InlineStartupPolicy::ClearHost);
}

/// Removes a temporary probe crate directory (and its own isolated `target/`, which a
/// full `cargo check` can grow to several hundred MB) on drop, including when the
/// enclosing test panics partway through — a plain end-of-function cleanup call would be
/// skipped by `assert!` failures and leak the directory on every failed run.
struct TempProbeDir(PathBuf);

impl TempProbeDir {
    fn new(unique_tag: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock before epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tui_lipan_inline_api_contract_{unique_tag}_{unique}"
        ));
        Self(path)
    }

    fn src_dir(&self) -> PathBuf {
        self.0.join("src")
    }

    fn manifest_path(&self) -> PathBuf {
        self.0.join("Cargo.toml")
    }

    fn target_dir(&self) -> PathBuf {
        self.0.join("target")
    }
}

impl Drop for TempProbeDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Writes a throwaway crate that depends on this checkout of `tui-lipan`, runs `cargo check`
/// on it, and asserts the check fails with an error mentioning one of `expected_error_substrings`
/// — used to pin that a removed/never-added API surface stays uncompilable. The probe directory
/// (and its isolated `CARGO_TARGET_DIR`) is always cleaned up via `TempProbeDir`'s `Drop`, even if
/// an assertion below panics.
fn assert_probe_crate_fails_to_compile(
    unique_tag: &str,
    package_name: &str,
    main_rs: &str,
    unexpected_success_message: &str,
    expected_error_substrings: &[&str],
) {
    let temp = TempProbeDir::new(unique_tag);
    fs::create_dir_all(temp.src_dir()).expect("create temp src");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    fs::write(
        temp.manifest_path(),
        format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ntui-lipan = {{ path = \"{manifest_dir}\" }}\n"
        ),
    )
    .expect("write temp Cargo.toml");

    fs::write(temp.src_dir().join("main.rs"), main_rs).expect("write temp main.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(temp.manifest_path())
        .env("CARGO_TARGET_DIR", temp.target_dir())
        .output()
        .expect("run cargo check for API probe");

    assert!(!output.status.success(), "{unexpected_success_message}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        expected_error_substrings
            .iter()
            .any(|needle| stderr.contains(needle)),
        "expected compiler error mentioning one of {expected_error_substrings:?}, got:\n{stderr}"
    );
}

#[test]
fn legacy_wrap_policy_api_is_not_public() {
    assert_probe_crate_fails_to_compile(
        "legacy_wrap_policy",
        "inline-api-contract-check",
        "use tui_lipan::{App, InlineWrapPolicy};\n\nfn main() {\n    let _ = App::new().inline(8).inline_wrap_policy(InlineWrapPolicy::AutoWrap);\n}\n",
        "legacy InlineWrapPolicy API unexpectedly compiled",
        &["InlineWrapPolicy", "inline_wrap_policy"],
    );
}

#[test]
fn inline_ephemeral_has_no_history_append_api() {
    assert_probe_crate_fails_to_compile(
        "ephemeral_append",
        "inline-api-contract-ephemeral-append-check",
        "use tui_lipan::prelude::*;\n\nstruct Demo;\n\nimpl Component for Demo {\n    type Message = ();\n    type Properties = ();\n    type State = ();\n\n    fn create_state(&self, _props: &Self::Properties) -> Self::State {}\n\n    fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {\n        ctx.insert_before([RichText::from(\"line\")]);\n        Update::full()\n    }\n\n    fn view(&self, _ctx: &Context<Self>) -> Element {\n        Text::new(\"demo\").into()\n    }\n}\n\nfn main() {\n    let _ = App::new().inline_ephemeral(4).mount(Demo);\n}\n",
        "ephemeral mode unexpectedly compiled historical append API usage",
        &["insert_before", "no method named"],
    );
}
