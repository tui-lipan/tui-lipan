//! Per-item colours, disabled commands, and multiline palette labels.

use tui_lipan::prelude::*;
use tui_lipan::{Result, Sketch};

fn view() -> Element {
    SearchPalette::new()
        .items([
            SearchItem::new("Current workspace\né猫 · /home/developer/project", 0)
                .active(true)
                .label_style(Style::new().fg(Color::Green))
                .right_style(Style::new().fg(Color::Cyan))
                .description(ItemDescription::new().right("ready")),
            SearchItem::new("Deploy to production", 1)
                .disabled(true)
                .description(ItemDescription::new().right("offline")),
            SearchItem::new("Open a very long project name that needs truncation", 2)
                .description(ItemDescription::new().right("Ctrl+O")),
        ])
        .description_placement(DescriptionPlacement::Right)
        .list_selection_style(Style::new().fg(Color::White).bg(Color::Blue))
        .list_active_style(Style::new().bg(Color::Red))
        .height(Length::Auto)
        .into()
}

pub fn sketch() -> Result<()> {
    Sketch::view("search-palette-items", view)
        .viewport(40, 12)
        .fit(20, 8)
        .focus_next(1)
        .write()?;
    Ok(())
}
