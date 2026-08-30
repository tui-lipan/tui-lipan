//! Pixel-precise pointer positions from the host (DEC private mode 1016).
//!
//! A program that wants to know where the pointer is more finely than which cell it is over - one
//! dragging a scrollbar, panning a canvas, moving a map - asks for SGR-pixels reporting and then
//! probes whether it took. Passing that on means having pixel positions to pass, which means asking
//! the *host* for the same mode and knowing how many pixels a cell is.
//!
//! Both conditions are strict, because the mode replaces cell coordinates with pixel ones inside
//! the same report: reading one as the other would misplace every click on the screen. So the host
//! has to answer the probe, and the cell size has to come from somewhere exact - the size the host
//! reports when asked, or a window size that divides evenly into the grid. A terminal that pads its
//! window reports one that does not, and is left reporting cells.
//!
//! Which is why what a report *contains* is tracked apart from what this host *could* do. Both
//! halves of [`is_active`] can turn true long after startup - the cell size is re-derived from
//! every resize - and until the host has actually been asked for the mode it keeps sending cells.
//! Dividing those by the cell size collapses the whole screen into its top-left corner, so
//! [`read_report`] answers from [`reports_pixels`]: what the host was told, not what it could be
//! told.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The host answered its startup probe saying it implements mode 1016.
static SUPPORTED: AtomicBool = AtomicBool::new(false);
/// Cell size in pixels, packed as `width << 16 | height`. Zero when not known exactly.
static CELL: AtomicU32 = AtomicU32::new(0);
/// The host has been asked for the mode and has not been told to stop, so reports carry pixels.
static REPORTS_PIXELS: AtomicBool = AtomicBool::new(false);

/// Cell sizes outside this range are a misreport, not a font.
const CELL_WIDTH: std::ops::RangeInclusive<u16> = 2..=64;
const CELL_HEIGHT: std::ops::RangeInclusive<u16> = 2..=128;

/// Where the pointer is, once a pixel report has been resolved against the cell size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PointerPosition {
    /// Zero-based cell the pointer is over, which is what the widget tree is laid out in.
    pub(crate) cell: (u16, u16),
    /// Where inside that cell it is, in pixels.
    pub(crate) sub_cell: (u16, u16),
}

/// What a pointer report off the wire turned out to hold.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PointerReport {
    /// The report already named a cell, and its coordinates stand as they are.
    Cells,
    /// The report named a pixel, resolved here against the cell size.
    Pixels(PointerPosition),
}

/// The cell size the host reported when asked, if it was asked and answered.
///
/// This is the direct answer to the question, so it beats dividing a window size.
#[cfg(feature = "image")]
fn queried_cell_size() -> Option<(u16, u16)> {
    crate::backend::ratatui_backend::image_support::queried_host_cell_size()
}

#[cfg(not(feature = "image"))]
fn queried_cell_size() -> Option<(u16, u16)> {
    None
}

/// Take the host's window size into account, whether from startup or a resize.
///
/// Called with whatever the platform reported: `None` pixel dimensions, or zeroes, mean the host
/// does not say, which leaves the queried size as the only source.
pub(crate) fn note_window_size(
    cols: u16,
    rows: u16,
    pixel_width: Option<u16>,
    pixel_height: Option<u16>,
) {
    // The division goes first because it is live: it follows a font zoom, where the size queried
    // once at startup does not. It also only answers when the grid divides the reported area
    // exactly, which is the same condition that makes a pixel report placeable at all.
    let cell = divide_window(cols, rows, pixel_width, pixel_height)
        .or_else(queried_cell_size)
        .filter(usable);
    // Nothing to learn from this size keeps whatever was already known. Reports are still arriving
    // as pixels if the mode is on, and the previous divisor places them far better than none.
    if let Some(cell) = cell {
        CELL.store(pack(cell), Ordering::Release);
    }
}

/// The cell size implied by a window size, when the grid divides it exactly.
fn divide_window(
    cols: u16,
    rows: u16,
    pixel_width: Option<u16>,
    pixel_height: Option<u16>,
) -> Option<(u16, u16)> {
    let width = pixel_width.filter(|value| *value > 0)?;
    let height = pixel_height.filter(|value| *value > 0)?;
    if cols == 0 || rows == 0 || width % cols != 0 || height % rows != 0 {
        return None;
    }
    Some((width / cols, height / rows))
}

fn usable((width, height): &(u16, u16)) -> bool {
    CELL_WIDTH.contains(width) && CELL_HEIGHT.contains(height)
}

fn pack((width, height): (u16, u16)) -> u32 {
    (u32::from(width) << 16) | u32::from(height)
}

/// The cell size in pixels that reports are resolved against.
///
/// A window size that divided exactly wins, being live; the size the host reported when asked is
/// the fallback, and is what makes this answerable at startup - before any window size has been
/// looked at - so that the decision about which input decoder to run can depend on it.
pub(crate) fn cell_size() -> Option<(u16, u16)> {
    let packed = CELL.load(Ordering::Acquire);
    if packed == 0 {
        return queried_cell_size().filter(usable);
    }
    Some(((packed >> 16) as u16, (packed & 0xffff) as u16))
}

/// Record what the host answered when asked whether it implements the mode.
pub(crate) fn note_host_support(supported: bool) {
    SUPPORTED.store(supported, Ordering::Release);
}

/// What the host answered about the mode, on its own.
///
/// Only [`crate::pixel_pointer_status`] wants this half in isolation: everything deciding behavior
/// reads [`is_active`], which needs the cell size too.
pub(crate) fn host_supports() -> bool {
    SUPPORTED.load(Ordering::Acquire)
}

/// Whether to ask the host for pixel reports, and so whether incoming reports carry pixels.
///
/// Both halves are required. A host that reports pixels we cannot divide into cells is worse than
/// one reporting cells, so an unknown cell size leaves the mode alone.
pub(crate) fn is_active() -> bool {
    SUPPORTED.load(Ordering::Acquire) && cell_size().is_some()
}

/// Record that the host has been asked for the mode, or told to drop it.
///
/// Called by whoever writes the escape, and only after the write succeeded: this is a note of what
/// is on the wire, not of what was wanted.
pub(crate) fn note_mode_enabled(enabled: bool) {
    REPORTS_PIXELS.store(enabled, Ordering::Release);
}

/// Whether the reports arriving right now carry pixels.
///
/// [`is_active`] answers whether the mode *can* be on, which is what decides whether to ask for it.
/// This answers whether it *is*, which is the only safe question to read a report against.
pub(crate) fn reports_pixels() -> bool {
    REPORTS_PIXELS.load(Ordering::Acquire)
}

/// Read a pointer report's coordinates as whatever the host was last asked to send.
///
/// A pixel report that cannot be divided falls back to cells rather than being dropped: one cell of
/// granularity beats no pointer at all.
pub(crate) fn read_report(column: u16, row: u16) -> PointerReport {
    if !reports_pixels() {
        return PointerReport::Cells;
    }
    resolve(column, row).map_or(PointerReport::Cells, PointerReport::Pixels)
}

/// Split a pixel report into the cell it lands in and where inside that cell it is.
pub(crate) fn resolve(x_pixels: u16, y_pixels: u16) -> Option<PointerPosition> {
    let (width, height) = cell_size()?;
    Some(PointerPosition {
        cell: (x_pixels / width, y_pixels / height),
        sub_cell: (x_pixels % width, y_pixels % height),
    })
}

/// Forget everything the host said, so one test's window size does not decide another's.
#[cfg(test)]
fn reset() {
    SUPPORTED.store(false, Ordering::Release);
    CELL.store(0, Ordering::Release);
    REPORTS_PIXELS.store(false, Ordering::Release);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The statics are process-wide, so the tests that touch them run as one.
    #[test]
    fn window_size_and_support_decide_together_whether_reports_are_pixels() {
        reset();

        // Nothing divides yet, so there is nothing to read a pixel report against and the host's
        // answer alone does not turn the mode on.
        note_window_size(80, 24, None, None);
        assert_eq!(cell_size(), None);
        note_host_support(true);
        assert!(!is_active(), "nothing to divide a report by");

        // A window size the grid divides exactly gives the cell size away, which is the other half.
        note_window_size(80, 24, Some(720), Some(408));
        assert_eq!(cell_size(), Some((9, 17)));
        assert!(is_active());

        // And a host that does not implement the mode never reports pixels, however well the window
        // divides.
        note_host_support(false);
        assert!(!is_active());
        note_host_support(true);

        assert_eq!(
            resolve(723, 20),
            Some(PointerPosition {
                cell: (80, 1),
                sub_cell: (3, 3)
            })
        );

        // A window that does not divide teaches nothing, and the mode is still on at the host, so
        // reports keep being read against the size that did divide.
        note_window_size(80, 24, Some(728), Some(408));
        assert_eq!(cell_size(), Some((9, 17)));
        assert!(is_active());

        // An implausible size is a misreport rather than a very large font, and is likewise ignored.
        note_window_size(1, 1, Some(2000), Some(2000));
        assert_eq!(cell_size(), Some((9, 17)));

        // A font zoom does divide, and the fresh size replaces the old one.
        note_window_size(80, 24, Some(880), Some(480));
        assert_eq!(cell_size(), Some((11, 20)));

        reset();

        // A resize can answer the cell-size half long after startup, which turns `is_active` true
        // for a host that was never asked for the mode and is still sending cells. Dividing those
        // would collapse the screen into its top-left corner, so only the write to the host decides
        // how a report is read.
        note_host_support(true);

        // A padded window divides into nothing, so startup leaves the mode unasked for.
        note_window_size(153, 40, Some(1074), Some(646));
        assert!(!is_active(), "no cell size means nothing to ask for");
        assert!(!reports_pixels());
        assert_eq!(read_report(20, 30), PointerReport::Cells);

        // A later resize does divide. The mode is now worth asking for - but until it has been
        // asked for, the coordinates on the wire are still cells.
        note_window_size(150, 40, Some(1050), Some(640));
        assert!(is_active(), "the cell size is known now");
        assert!(!reports_pixels(), "nothing has written the escape yet");
        assert_eq!(
            read_report(20, 30),
            PointerReport::Cells,
            "cells stay cells until the host is asked"
        );

        // Once it has, the same numbers are pixels.
        note_mode_enabled(true);
        assert_eq!(
            read_report(20, 30),
            PointerReport::Pixels(PointerPosition {
                cell: (2, 1),
                sub_cell: (6, 14)
            })
        );

        // And a suspend that drops the mode puts them back, without waiting for a resize to say so.
        note_mode_enabled(false);
        assert_eq!(read_report(20, 30), PointerReport::Cells);

        reset();
    }
}
