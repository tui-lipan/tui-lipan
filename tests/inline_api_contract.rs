use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tui_lipan::{
    App, DraggableTabBarOverflow, FocusChanged, FocusEntry, InlineHeight, InlineStartupPolicy, Key,
    ScrollTarget, ScrollWheelBehavior, ScrollWheelConfig, SurfaceMode, Tag, TextAreaLineNumberMode,
    TextAreaSentinelClickEvent, TextAreaSentinelClickKind,
};

#[test]
fn root_and_prelude_export_focus_event_types() {
    let entry = FocusEntry::new(Some(Key::from("field")), Tag::Input);
    assert!(entry.is_within_key("field"));
    assert_eq!(entry.keys().count(), 1);
    let changed = FocusChanged {
        old: None,
        new: Some(entry),
    };
    let _ = App::new().on_focus_changed(|_: &FocusChanged| {});
    let _: tui_lipan::prelude::FocusChanged = changed;
    let _: tui_lipan::prelude::Tag = Tag::Input;
}

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

/// Locates the on-disk scratch crate for one API probe.
///
/// Every probe crate lives under `target/inline-api-probe/` and they all share a single
/// `CARGO_TARGET_DIR`, so the nested `cargo check` builds `tui-lipan` and its dependency
/// graph exactly once and reuses it on every later run. An isolated per-run temp dir would
/// be tidier, but it rebuilds the whole crate from scratch each time: that costs well over
/// a minute of CPU per probe and fans out a second 16-wide rustc job alongside the test
/// harness. `target/` is already gitignored and already swept by `cargo clean`, so the
/// cached dirs need no cleanup of their own.
struct ProbeDir(PathBuf);

impl ProbeDir {
    fn new(unique_tag: &str) -> Self {
        Self(Self::root().join(unique_tag))
    }

    /// Shared parent of every probe crate, and of the one `CARGO_TARGET_DIR` they share.
    fn root() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .join("inline-api-probe")
    }

    fn src_dir(&self) -> PathBuf {
        self.0.join("src")
    }

    fn manifest_path(&self) -> PathBuf {
        self.0.join("Cargo.toml")
    }

    /// Shared across probes on purpose, so the second probe reuses the first one's build.
    /// Concurrent probes serialize on Cargo's lock for this directory, which also keeps two
    /// nested builds from fighting over every core at once.
    fn target_dir(&self) -> PathBuf {
        Self::root().join("target")
    }
}

/// Writes `contents` to `path` only when it differs from what is already there.
///
/// Cargo decides freshness by mtime, so unconditionally rewriting an unchanged file would
/// force the probe crate to be re-checked on every run.
fn write_if_changed(path: &Path, contents: &str, what: &str) {
    if fs::read_to_string(path).is_ok_and(|existing| existing == contents) {
        return;
    }
    fs::write(path, contents).unwrap_or_else(|err| panic!("write {what}: {err}"));
}

/// Writes a throwaway crate that depends on this checkout of `tui-lipan`, runs `cargo check`
/// on it, and asserts the check fails with an error mentioning one of `expected_error_substrings`
/// — used to pin that a removed/never-added API surface stays uncompilable. The probe crate and
/// its `CARGO_TARGET_DIR` are cached under `target/inline-api-probe/` between runs; see
/// [`ProbeDir`] for why they are not thrown away.
fn assert_probe_crate_fails_to_compile(
    unique_tag: &str,
    package_name: &str,
    main_rs: &str,
    unexpected_success_message: &str,
    expected_error_substrings: &[&str],
) {
    let probe = ProbeDir::new(unique_tag);
    fs::create_dir_all(probe.src_dir()).expect("create probe src");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    // The trailing empty `[workspace]` makes the probe its own workspace root. Without it
    // Cargo walks up, finds this crate's workspace, and refuses to build a nested package
    // that is not a member.
    write_if_changed(
        &probe.manifest_path(),
        &format!(
            "[package]\nname = \"{package_name}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ntui-lipan = {{ path = \"{manifest_dir}\" }}\n\n[workspace]\n"
        ),
        "probe Cargo.toml",
    );

    write_if_changed(&probe.src_dir().join("main.rs"), main_rs, "probe main.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(probe.manifest_path())
        .env("CARGO_TARGET_DIR", probe.target_dir())
        // Probes can run concurrently with each other and with the rest of the suite; an
        // uncapped nested build would claim every core on top of the test harness.
        .env("CARGO_BUILD_JOBS", "4")
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
