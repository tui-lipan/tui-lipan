use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use tui_lipan::{
    App, DraggableTabBarOverflow, InlineStartupPolicy, ScrollTarget, ScrollWheelBehavior,
    ScrollWheelConfig, SurfaceMode, TextAreaLineNumberMode, TextAreaSentinelClickEvent,
    TextAreaSentinelClickKind,
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
fn named_inline_modes_are_constructible() {
    let _ = SurfaceMode::Fullscreen;
    let _ = SurfaceMode::InlineEphemeral { height: 8 };
    let _ = SurfaceMode::InlineTranscript {
        height: 12,
        startup: InlineStartupPolicy::PreserveHost,
    };

    let _ = App::new().surface(SurfaceMode::InlineEphemeral { height: 4 });
    let _ = App::new().inline_ephemeral(4);
    let _ = App::new().inline_transcript(4);
    let _ = App::new().inline_transcript_with_startup(4, InlineStartupPolicy::ClearHost);
}

#[test]
fn legacy_wrap_policy_api_is_not_public() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!("tui_lipan_inline_api_contract_{unique}"));
    let src_dir = temp_root.join("src");
    fs::create_dir_all(&src_dir).expect("create temp src");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    fs::write(
        temp_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"inline-api-contract-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ntui-lipan = {{ path = \"{manifest_dir}\" }}\n"
        ),
    )
    .expect("write temp Cargo.toml");

    fs::write(
        src_dir.join("main.rs"),
        "use tui_lipan::{App, InlineWrapPolicy};\n\nfn main() {\n    let _ = App::new().inline(8).inline_wrap_policy(InlineWrapPolicy::AutoWrap);\n}\n",
    )
    .expect("write temp main.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(temp_root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", temp_root.join("target"))
        .output()
        .expect("run cargo check for legacy API probe");

    assert!(
        !output.status.success(),
        "legacy InlineWrapPolicy API unexpectedly compiled"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("InlineWrapPolicy") || stderr.contains("inline_wrap_policy"),
        "expected compiler error mentioning removed legacy API, got:\n{stderr}"
    );
}

#[test]
fn inline_ephemeral_has_no_history_append_api() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock before epoch")
        .as_nanos();
    let temp_root = std::env::temp_dir().join(format!(
        "tui_lipan_inline_api_contract_ephemeral_append_{unique}"
    ));
    let src_dir = temp_root.join("src");
    fs::create_dir_all(&src_dir).expect("create temp src");

    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    fs::write(
        temp_root.join("Cargo.toml"),
        format!(
            "[package]\nname = \"inline-api-contract-ephemeral-append-check\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\ntui-lipan = {{ path = \"{manifest_dir}\" }}\n"
        ),
    )
    .expect("write temp Cargo.toml");

    fs::write(
        src_dir.join("main.rs"),
        "use tui_lipan::prelude::*;\n\nstruct Demo;\n\nimpl Component for Demo {\n    type Message = ();\n    type Properties = ();\n    type State = ();\n\n    fn create_state(&self, _props: &Self::Properties) -> Self::State {}\n\n    fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {\n        ctx.insert_before([RichText::from(\"line\")]);\n        Update::full()\n    }\n\n    fn view(&self, _ctx: &Context<Self>) -> Element {\n        Text::new(\"demo\").into()\n    }\n}\n\nfn main() {\n    let _ = App::new().inline_ephemeral(4).mount(Demo);\n}\n",
    )
    .expect("write temp main.rs");

    let output = Command::new("cargo")
        .arg("check")
        .arg("--quiet")
        .arg("--manifest-path")
        .arg(temp_root.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", temp_root.join("target"))
        .output()
        .expect("run cargo check for ephemeral append API probe");

    assert!(
        !output.status.success(),
        "ephemeral mode unexpectedly compiled historical append API usage"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("insert_before") || stderr.contains("no method named"),
        "expected compiler error mentioning removed history append API surface, got:\n{stderr}"
    );
}
