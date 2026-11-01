//! Per-item colours, disabled commands, and multiline palette labels.

use std::sync::Arc;

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
        .list_config(ListConfig::default().gutter_gap(1))
        .item_status(Arc::new(|item, _| {
            item.disabled
                .then(|| ListItemStatus::from_spans([Span::new("●").fg(Color::Red)]))
        }))
        .item_gutter(Arc::new(|item, _| {
            item.disabled.then(|| {
                ListItemGutter::spinner(
                    Spinner::new()
                        .spinner_style(SpinnerStyle::Line)
                        .frame(0)
                        .label("offline")
                        .style(Style::new().fg(Color::Red))
                        .label_style(Style::new().fg(Color::Red)),
                )
            })
        }))
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
