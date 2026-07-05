use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::core::node::WidgetNode;
use crate::style::{Length, Span, Style};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::Overflow;

use super::reconcile::PreparePlan;
use super::{
    Sparkline, SparklineAggregation, SparklineLineGlyphs, SparklineVariant, SparklineZeroPolicy,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SparklineCacheKey {
    pub(crate) hash: u64,
}

impl SparklineCacheKey {
    /// Cache key that accounts for the prepared-data plan derived from the
    /// allocated rect (target sample count + slicing strategy).
    pub(crate) fn new_with_plan(spark: &Sparkline, plan: PreparePlan) -> Self {
        let mut hasher = DefaultHasher::new();
        render_hash_into(&mut hasher, spark);
        plan.hash(&mut hasher);
        Self {
            hash: hasher.finish(),
        }
    }
}

fn render_hash_into(hasher: &mut DefaultHasher, spark: &Sparkline) {
    spark.data.hash(hasher);
    spark.min.hash(hasher);
    spark.max.hash(hasher);
    spark.bars.hash(hasher);
    spark.variant.hash(hasher);
    spark.max_points.hash(hasher);
    spark.aggregation.hash(hasher);
    spark.zero_policy.hash(hasher);
    spark.line_glyphs.hash(hasher);
    spark.chart_height.hash(hasher);
    spark.mirror_x.hash(hasher);
    spark.mirror_y.hash(hasher);
    spark.style.hash(hasher);
    spark.rising_style.hash(hasher);
    spark.falling_style.hash(hasher);
    spark.flat_style.hash(hasher);
    spark.turn_style.hash(hasher);
    spark.gradient.hash(hasher);
    spark.height_gradient.hash(hasher);
    spark.gradient_range.hash(hasher);
    spark.overflow.hash(hasher);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SparklineWidgetKey {
    pub(crate) render_key: SparklineCacheKey,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl SparklineWidgetKey {
    pub(crate) fn new(spark: &Sparkline, render_key: SparklineCacheKey) -> Self {
        Self {
            render_key,
            width: spark.width,
            height: spark.height,
        }
    }
}

#[derive(Clone)]
pub struct SparklineNode {
    pub data: Vec<u64>,
    pub min: Option<u64>,
    pub max: Option<u64>,
    pub bars: Vec<char>,
    pub variant: SparklineVariant,
    pub max_points: Option<usize>,
    pub aggregation: SparklineAggregation,
    pub zero_policy: SparklineZeroPolicy,
    pub line_glyphs: SparklineLineGlyphs,
    pub chart_height: u16,
    pub mirror_x: bool,
    pub mirror_y: bool,
    pub style: Style,
    pub rising_style: Style,
    pub falling_style: Style,
    pub flat_style: Style,
    pub turn_style: Style,
    pub gradient: Option<ColorGradient>,
    pub height_gradient: Option<ColorGradient>,
    pub gradient_range: Option<GradientRange>,
    pub width: Length,
    pub height: Length,
    pub overflow: Overflow,

    /// Cached rendered output (rows of spans).
    pub(crate) output: Arc<SparklineRenderOutput>,
    pub(crate) cache_key: SparklineCacheKey,
    pub(crate) widget_key: SparklineWidgetKey,
}

#[derive(Debug)]
pub(crate) struct SparklineRenderOutput {
    pub rows: Vec<Vec<Span>>,
}

impl Default for SparklineNode {
    fn default() -> Self {
        Self {
            data: Vec::new(),
            min: None,
            max: None,
            bars: Vec::new(),
            variant: SparklineVariant::Bars,
            max_points: None,
            aggregation: SparklineAggregation::Average,
            zero_policy: SparklineZeroPolicy::Empty,
            line_glyphs: SparklineLineGlyphs::default(),
            chart_height: 1,
            mirror_x: false,
            mirror_y: false,
            style: Style::default(),
            rising_style: Style::default(),
            falling_style: Style::default(),
            flat_style: Style::default(),
            turn_style: Style::default(),
            gradient: None,
            height_gradient: None,
            gradient_range: None,
            width: Length::Auto,
            height: Length::Auto,
            overflow: Overflow::Auto,
            output: Arc::new(SparklineRenderOutput { rows: Vec::new() }),
            cache_key: SparklineCacheKey { hash: 0 },
            widget_key: SparklineWidgetKey {
                render_key: SparklineCacheKey { hash: 0 },
                width: Length::Auto,
                height: Length::Auto,
            },
        }
    }
}

impl From<Sparkline> for SparklineNode {
    fn from(value: Sparkline) -> Self {
        let mut node = Self::default();
        super::reconcile_sparkline(&value, &mut node, None);
        node
    }
}

impl WidgetNode for SparklineNode {}
