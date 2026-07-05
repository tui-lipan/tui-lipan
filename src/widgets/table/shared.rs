use crate::style::BorderStyle;

#[derive(Clone, Copy)]
pub(crate) struct TableBorderGlyphs {
    pub top_left: &'static str,
    pub top: &'static str,
    pub top_right: &'static str,
    pub left: &'static str,
    pub center: &'static str,
    pub right: &'static str,
    pub mid_left: &'static str,
    pub mid: &'static str,
    pub mid_right: &'static str,
    pub bottom_left: &'static str,
    pub bottom: &'static str,
    pub bottom_right: &'static str,
    pub top_mid: &'static str,
    pub mid_mid: &'static str,
    pub bottom_mid: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TableBorderLineKind {
    Top,
    Mid,
    Bottom,
}

pub(crate) fn table_border_glyphs(style: BorderStyle) -> TableBorderGlyphs {
    match style {
        BorderStyle::Plain => TableBorderGlyphs {
            top_left: "┌",
            top: "─",
            top_right: "┐",
            left: "│",
            center: "│",
            right: "│",
            mid_left: "├",
            mid: "─",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "─",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::Rounded => TableBorderGlyphs {
            top_left: "╭",
            top: "─",
            top_right: "╮",
            left: "│",
            center: "│",
            right: "│",
            mid_left: "├",
            mid: "─",
            mid_right: "┤",
            bottom_left: "╰",
            bottom: "─",
            bottom_right: "╯",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::Double => TableBorderGlyphs {
            top_left: "╔",
            top: "═",
            top_right: "╗",
            left: "║",
            center: "║",
            right: "║",
            mid_left: "╠",
            mid: "═",
            mid_right: "╣",
            bottom_left: "╚",
            bottom: "═",
            bottom_right: "╝",
            top_mid: "╦",
            mid_mid: "╬",
            bottom_mid: "╩",
        },
        BorderStyle::Thick => TableBorderGlyphs {
            top_left: "┏",
            top: "━",
            top_right: "┓",
            left: "┃",
            center: "┃",
            right: "┃",
            mid_left: "┣",
            mid: "━",
            mid_right: "┫",
            bottom_left: "┗",
            bottom: "━",
            bottom_right: "┛",
            top_mid: "┳",
            mid_mid: "╋",
            bottom_mid: "┻",
        },
        BorderStyle::LightDoubleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "╌",
            top_right: "┐",
            left: "╎",
            center: "╎",
            right: "╎",
            mid_left: "├",
            mid: "╌",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "╌",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::HeavyDoubleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "╍",
            top_right: "┐",
            left: "╏",
            center: "╏",
            right: "╏",
            mid_left: "├",
            mid: "╍",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "╍",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::LightTripleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "┄",
            top_right: "┐",
            left: "┆",
            center: "┆",
            right: "┆",
            mid_left: "├",
            mid: "┄",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "┄",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::HeavyTripleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "┅",
            top_right: "┐",
            left: "┇",
            center: "┇",
            right: "┇",
            mid_left: "├",
            mid: "┅",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "┅",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::LightQuadrupleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "┈",
            top_right: "┐",
            left: "┊",
            center: "┊",
            right: "┊",
            mid_left: "├",
            mid: "┈",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "┈",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::HeavyQuadrupleDashed => TableBorderGlyphs {
            top_left: "┌",
            top: "┉",
            top_right: "┐",
            left: "┋",
            center: "┋",
            right: "┋",
            mid_left: "├",
            mid: "┉",
            mid_right: "┤",
            bottom_left: "└",
            bottom: "┉",
            bottom_right: "┘",
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
        BorderStyle::Custom { glyphs } => TableBorderGlyphs {
            top_left: glyphs.top_left,
            top: glyphs.top,
            top_right: glyphs.top_right,
            left: glyphs.left,
            center: glyphs.left,
            right: glyphs.right,
            mid_left: glyphs.left,
            mid: glyphs.top,
            mid_right: glyphs.right,
            bottom_left: glyphs.bottom_left,
            bottom: glyphs.bottom,
            bottom_right: glyphs.bottom_right,
            top_mid: "┬",
            mid_mid: "┼",
            bottom_mid: "┴",
        },
    }
}

pub(crate) fn table_border_line(
    kind: TableBorderLineKind,
    widths: &[u16],
    glyphs: TableBorderGlyphs,
    outer_frame: bool,
    column_separators: bool,
) -> String {
    let mut out = String::new();
    match kind {
        TableBorderLineKind::Top => {
            if outer_frame {
                out.push_str(glyphs.top_left);
                if column_separators {
                    for (i, &w) in widths.iter().enumerate() {
                        out.push_str(&glyphs.top.repeat(w as usize));
                        if i + 1 < widths.len() {
                            out.push_str(glyphs.top_mid);
                        }
                    }
                } else {
                    let total: usize = widths.iter().map(|w| *w as usize).sum();
                    out.push_str(&glyphs.top.repeat(total));
                }
                out.push_str(glyphs.top_right);
            }
        }
        TableBorderLineKind::Mid => {
            if outer_frame {
                out.push_str(glyphs.mid_left);
            }
            for (i, &w) in widths.iter().enumerate() {
                out.push_str(&glyphs.mid.repeat(w as usize));
                if i + 1 < widths.len() && column_separators {
                    out.push_str(glyphs.mid_mid);
                }
            }
            if outer_frame {
                out.push_str(glyphs.mid_right);
            }
        }
        TableBorderLineKind::Bottom => {
            if outer_frame {
                out.push_str(glyphs.bottom_left);
                if column_separators {
                    for (i, &w) in widths.iter().enumerate() {
                        out.push_str(&glyphs.bottom.repeat(w as usize));
                        if i + 1 < widths.len() {
                            out.push_str(glyphs.bottom_mid);
                        }
                    }
                } else {
                    let total: usize = widths.iter().map(|w| *w as usize).sum();
                    out.push_str(&glyphs.bottom.repeat(total));
                }
                out.push_str(glyphs.bottom_right);
            }
        }
    }
    out
}

pub(crate) fn table_fixed_chars(ncols: usize, outer_frame: bool, column_separators: bool) -> u16 {
    let outer: u16 = if outer_frame { 2 } else { 0 };
    let inner: u16 = if column_separators {
        ncols.saturating_sub(1) as u16
    } else {
        0
    };
    outer.saturating_add(inner)
}

pub(crate) fn table_render_width(
    widths: &[u16],
    outer_frame: bool,
    column_separators: bool,
) -> u16 {
    widths
        .iter()
        .copied()
        .sum::<u16>()
        .saturating_add(table_fixed_chars(
            widths.len(),
            outer_frame,
            column_separators,
        ))
}

pub(crate) fn distribute_extra_width(widths: &mut [u16], mut extra: u16) {
    if widths.is_empty() || extra == 0 {
        return;
    }
    let mut i = 0usize;
    while extra > 0 {
        let idx = i % widths.len();
        widths[idx] = widths[idx].saturating_add(1);
        extra -= 1;
        i = i.saturating_add(1);
    }
}

pub(crate) fn shrink_widths_to_fit(widths: &mut [u16], target_total: u16, min_col_total: u16) {
    if widths.is_empty() {
        return;
    }
    let mut current_total: u16 = widths.iter().copied().sum();
    if current_total <= target_total {
        return;
    }

    while current_total > target_total {
        let mut max_idx = None;
        let mut max_w = min_col_total;
        for (i, &w) in widths.iter().enumerate() {
            if w > max_w {
                max_w = w;
                max_idx = Some(i);
            }
        }
        let Some(i) = max_idx else {
            break;
        };
        widths[i] = widths[i].saturating_sub(1).max(min_col_total);
        let next_total: u16 = widths.iter().copied().sum();
        if next_total == current_total {
            break;
        }
        current_total = next_total;
    }
}
