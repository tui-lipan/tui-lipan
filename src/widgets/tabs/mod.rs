//! Tabs widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_tabs;
pub use node::TabsNode;
pub use reconcile::reconcile_tabs;

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::style::{BorderStyle, Length, Padding, Style, StyleSlot};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Tab overflow policy.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TabsOverflow {
    /// Current behavior: greedily pack tabs from the start.
    #[default]
    Clip,
    /// Keep all tabs visible by allocating per-tab budgets and ellipsizing labels.
    Ellipsis,
}

/// A tab change event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TabsEvent {
    /// Active tab index.
    pub index: usize,
}

/// A tab title.
#[derive(Clone, Debug)]
pub struct Tab {
    pub(crate) label: Arc<str>,
    pub(crate) style: Style,
}

impl Tab {
    /// Create a new tab.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            style: Style::default(),
        }
    }

    /// Set style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl From<&'static str> for Tab {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Tab {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for Tab {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

/// A horizontal tab bar.
#[derive(Clone)]
pub struct Tabs {
    pub(crate) tabs: Arc<[Tab]>,
    pub(crate) active: usize,
    pub(crate) style: Style,
    pub(crate) focus_style: StyleSlot,
    pub(crate) hover_style: StyleSlot,
    pub(crate) tab_hover_style: StyleSlot,
    pub(crate) active_style: StyleSlot,
    pub(crate) divider: char,
    pub(crate) caps: Option<(char, char)>,
    pub(crate) overflow: TabsOverflow,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) on_change: Option<Callback<TabsEvent>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
}

impl Default for Tabs {
    fn default() -> Self {
        Self {
            tabs: Arc::new([]),
            active: 0,
            style: Style::default(),
            focus_style: StyleSlot::Inherit,
            hover_style: StyleSlot::Inherit,
            tab_hover_style: StyleSlot::Inherit,
            active_style: StyleSlot::Inherit,
            divider: '│',
            caps: None,
            overflow: TabsOverflow::Clip,
            border: false,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            width: Length::Flex(1),
            height: Length::Auto,
            on_change: None,
            on_click: None,
            on_key: None,
            disabled: false,
            disabled_style: Style::default(),
            focusable: true,
        }
    }
}

impl Tabs {
    /// Create an empty tab bar.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace tabs.
    pub fn tabs<I>(mut self, tabs: I) -> Self
    where
        I: IntoIterator<Item = Tab>,
    {
        self.tabs = tabs.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Add a tab.
    pub fn tab(mut self, tab: impl Into<Tab>) -> Self {
        let mut tabs = self.tabs.to_vec();
        tabs.push(tab.into());
        self.tabs = tabs.into();
        self
    }

    /// Set active tab index.
    pub fn active(mut self, active: usize) -> Self {
        self.active = active;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style when the tabs widget is focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's focus style with additional fields.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set style when tabs widget is hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set style for hovered tab.
    pub fn tab_hover_style(mut self, style: Style) -> Self {
        self.tab_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's tab hover style with additional fields.
    pub fn extend_tab_hover_style(mut self, style: Style) -> Self {
        self.tab_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit tab hover style from the active theme.
    pub fn inherit_tab_hover_style(mut self) -> Self {
        self.tab_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set active tab style.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's active-tab style with additional fields.
    pub fn extend_active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit active-tab style from the active theme.
    pub fn inherit_active_style(mut self) -> Self {
        self.active_style = StyleSlot::Inherit;
        self
    }

    /// Set divider character.
    pub fn divider(mut self, ch: char) -> Self {
        self.divider = ch;
        self
    }

    /// Set the `(left, right)` end-cap glyphs drawn around the active and hovered tabs.
    ///
    /// Each cap replaces one of the tab's two padding cells, so the tab keeps its
    /// measured width and hit region. The glyphs are painted in the tab's own
    /// background color over the strip background, so the tab reads as a rounded or
    /// pointed pill (pass powerline separators for that look). `None` (the default)
    /// keeps flat space padding on every tab. Caps are skipped for a tab that has no
    /// distinct background or that is truncated by the overflow policy.
    pub fn caps(mut self, caps: Option<(char, char)>) -> Self {
        self.caps = caps;
        self
    }

    /// Set overflow policy.
    pub fn overflow(mut self, overflow: TabsOverflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Draw a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Override requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Override requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Callback fired when the active tab changes.
    pub fn on_change(mut self, cb: Callback<TabsEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set on-click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    pub(crate) fn index_at_col(
        tabs: &[Tab],
        divider: char,
        overflow: TabsOverflow,
        inner_w: usize,
        col: usize,
    ) -> Option<usize> {
        let mut x = 0usize;
        let pad_w = 2; // " " + " "
        let div_w = UnicodeWidthChar::width(divider).unwrap_or(1);
        let budgets = tab_width_budgets(tabs, divider, inner_w, overflow);

        for (i, tab) in tabs.iter().enumerate() {
            let w = budgets.as_ref().map_or_else(
                || UnicodeWidthStr::width(tab.label.as_ref()).saturating_add(pad_w),
                |budgets| budgets.get(i).copied().unwrap_or(0) as usize,
            );
            if col < x.saturating_add(w) {
                return Some(i);
            }
            x = x.saturating_add(w);

            if i + 1 < tabs.len() {
                x = x.saturating_add(div_w);
            }
        }

        None
    }
}

impl From<Tabs> for Element {
    fn from(value: Tabs) -> Self {
        Element::new(ElementKind::Tabs(value))
    }
}

impl crate::layout::hash::LayoutHash for Tabs {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.border_style.hash(hasher);
        self.padding.hash(hasher);
        self.tabs.len().hash(hasher);
        self.active.hash(hasher);
        self.divider.hash(hasher);
        self.caps.hash(hasher);
        self.overflow.hash(hasher);
        Some(())
    }
}

/// Return rendered width budget per tab for the given available width.
///
/// For `TabsOverflow::Clip`, returns `None` so callers keep the greedy path.
pub(crate) fn tab_width_budgets(
    tabs: &[Tab],
    divider: char,
    max_w: usize,
    overflow: TabsOverflow,
) -> Option<Vec<u16>> {
    if overflow == TabsOverflow::Clip {
        return None;
    }

    if tabs.is_empty() {
        return Some(Vec::new());
    }

    let n = tabs.len();
    let div_w = UnicodeWidthChar::width(divider).unwrap_or(1);
    let divider_budget = div_w.saturating_mul(n.saturating_sub(1));
    let usable = max_w.saturating_sub(divider_budget);

    let nat: Vec<usize> = tabs
        .iter()
        .map(|tab| UnicodeWidthStr::width(tab.label.as_ref()).saturating_add(2))
        .collect();
    let nat_sum = nat.iter().copied().sum::<usize>();
    if nat_sum <= usable {
        return Some(
            nat.into_iter()
                .map(|w| w.min(u16::MAX as usize) as u16)
                .collect(),
        );
    }

    const MIN_TAB_CELLS: usize = 3;
    let min_total = MIN_TAB_CELLS.saturating_mul(n);
    if usable < min_total {
        let each = (usable / n).max(1).min(u16::MAX as usize) as u16;
        return Some(vec![each; n]);
    }

    let mut alloc = vec![MIN_TAB_CELLS; n];
    let mut caps: Vec<usize> = nat
        .iter()
        .map(|&w| w.saturating_sub(MIN_TAB_CELLS))
        .collect();
    let mut extra = usable.saturating_sub(min_total);

    let pass1 = allocate_proportional(&caps, &nat, extra);
    for i in 0..n {
        alloc[i] = alloc[i].saturating_add(pass1[i]);
        caps[i] = caps[i].saturating_sub(pass1[i]);
    }
    extra = extra.saturating_sub(pass1.iter().copied().sum::<usize>());

    if extra > 0 {
        let pass2 = allocate_proportional(&caps, &nat, extra);
        for i in 0..n {
            alloc[i] = alloc[i].saturating_add(pass2[i]);
        }
    }

    Some(
        alloc
            .into_iter()
            .map(|w| w.min(u16::MAX as usize) as u16)
            .collect(),
    )
}

fn allocate_proportional(caps: &[usize], weights: &[usize], budget: usize) -> Vec<usize> {
    let n = caps.len();
    if budget == 0 || n == 0 {
        return vec![0; n];
    }

    let active_weight_sum = caps
        .iter()
        .zip(weights.iter())
        .filter(|(cap, _)| **cap > 0)
        .map(|(_, w)| *w)
        .sum::<usize>();
    if active_weight_sum == 0 {
        return vec![0; n];
    }

    let mut out = vec![0usize; n];
    let mut fracs = Vec::with_capacity(n);

    for i in 0..n {
        if caps[i] == 0 {
            fracs.push((i, -1.0_f64));
            continue;
        }

        let exact = (budget as f64) * (weights[i] as f64) / (active_weight_sum as f64);
        let grant = (exact.floor() as usize).min(caps[i]);
        out[i] = grant;
        fracs.push((i, exact - (grant as f64)));
    }

    let mut leftover = budget
        .saturating_sub(out.iter().copied().sum::<usize>())
        .min(caps.iter().copied().sum::<usize>());

    fracs.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    while leftover > 0 {
        let mut progressed = false;
        for (idx, _) in &fracs {
            if leftover == 0 {
                break;
            }
            if out[*idx] < caps[*idx] {
                out[*idx] += 1;
                leftover -= 1;
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::{Tab, TabsOverflow, tab_width_budgets};

    fn mk_tabs(labels: &[&str]) -> Vec<Tab> {
        labels.iter().map(|l| Tab::new(*l)).collect()
    }

    #[test]
    fn budgets_equal_labels_exact_fit_under_and_over() {
        let tabs = mk_tabs(&["aa", "bb", "cc"]);

        assert_eq!(
            tab_width_budgets(&tabs, '|', 14, TabsOverflow::Ellipsis),
            Some(vec![4, 4, 4])
        );
        assert_eq!(
            tab_width_budgets(&tabs, '|', 11, TabsOverflow::Ellipsis),
            Some(vec![3, 3, 3])
        );
        assert_eq!(
            tab_width_budgets(&tabs, '|', 20, TabsOverflow::Ellipsis),
            Some(vec![4, 4, 4])
        );
    }

    #[test]
    fn budgets_long_label_eats_slack_first() {
        let tabs = mk_tabs(&["x", "super-long-label", "y"]);
        let budgets = tab_width_budgets(&tabs, '|', 20, TabsOverflow::Ellipsis).unwrap();

        assert_eq!(budgets.len(), 3);
        assert_eq!(budgets[0], 3);
        assert_eq!(budgets[2], 3);
        assert!(budgets[1] > budgets[0]);
    }

    #[test]
    fn budgets_single_tab() {
        let tabs = mk_tabs(&["hello"]);
        assert_eq!(
            tab_width_budgets(&tabs, '|', 7, TabsOverflow::Ellipsis),
            Some(vec![7])
        );
        assert_eq!(
            tab_width_budgets(&tabs, '|', 3, TabsOverflow::Ellipsis),
            Some(vec![3])
        );
    }

    #[test]
    fn budgets_zero_tabs_and_zero_width() {
        let tabs = mk_tabs(&[]);
        assert_eq!(
            tab_width_budgets(&tabs, '|', 0, TabsOverflow::Ellipsis),
            Some(vec![])
        );

        let one = mk_tabs(&["a", "b", "c"]);
        assert_eq!(
            tab_width_budgets(&one, '|', 0, TabsOverflow::Ellipsis),
            Some(vec![1, 1, 1])
        );
    }

    #[test]
    fn budgets_with_wide_divider() {
        let tabs = mk_tabs(&["aa", "bb"]);
        // Divider '好' has width 2, so max_w=8 leaves usable=6.
        assert_eq!(
            tab_width_budgets(&tabs, '好', 8, TabsOverflow::Ellipsis),
            Some(vec![3, 3])
        );
    }
}
