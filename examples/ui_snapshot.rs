use std::fs;
use std::path::PathBuf;

use tui_lipan::prelude::*;
use tui_lipan::{TestBackend, UiSnapshotOptions};

struct AgentDashboard;

impl Component for AgentDashboard {
    type Message = ();
    type Properties = ();
    type State = u8;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        0
    }

    fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = ctx.state.wrapping_add(1);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        HStack::new()
            .child(
                Frame::new()
                    .title("Nav")
                    .width(Length::Px(22))
                    .child(
                        List::new()
                            .items(["Overview", "Logs"].map(ListItem::new))
                            .selected(ctx.state as usize % 2)
                            .key("routes"),
                    )
                    .key("sidebar"),
            )
            .child(
                Frame::new()
                    .title("Panel")
                    .child(Text::new(format!("tick {}", ctx.state)))
                    .key("panel"),
            )
            .into()
    }
}

fn export_dir() -> PathBuf {
    std::env::temp_dir().join("tui-lipan-ui-snapshot-example")
}

fn main() -> tui_lipan::Result<()> {
    let mut backend = TestBackend::new(AgentDashboard);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 16,
    });
    backend.render();
    backend.dispatch(()).ok();

    let snapshot = backend.capture_ui_snapshot_with_margin(20, 8, &UiSnapshotOptions::default());
    let dir = export_dir();
    fs::create_dir_all(&dir)?;

    let md_path = dir.join("ui-snapshot.md");
    fs::write(&md_path, snapshot.to_markdown())?;
    println!("Wrote {}", md_path.display());

    #[cfg(feature = "ui-snapshot-json")]
    {
        let json_path = dir.join("ui-snapshot.json");
        fs::write(&json_path, snapshot.to_json_pretty())?;
        println!("Wrote {}", json_path.display());
    }

    #[cfg(feature = "ui-snapshot-png")]
    {
        // PNG uses real-font text when available, with bitmap-cell rendering as
        // the deterministic fallback.
        let png_path = dir.join("ui-snapshot.png");
        fs::write(&png_path, snapshot.to_png_default())?;
        println!("Wrote {}", png_path.display());
    }

    println!("\n--- markdown preview ---\n{}", snapshot.to_markdown());

    // Live app pattern (queued until after the next paint):
    // let slot = UiSnapshotSlot::new();
    // ctx.request_ui_snapshot_to("ui-snapshot.md");
    // ctx.request_ui_snapshot_to_slot(&slot);

    Ok(())
}
