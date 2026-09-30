//! Date picker types.

/// Date selection event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct DateEvent {
    /// Selected year.
    pub year: i32,
    /// Selected month (1-12).
    pub month: u32,
    /// Selected day (1-31).
    pub day: u32,
}
