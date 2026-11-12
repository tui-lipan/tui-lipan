//! [`RenderState`]: a per-frame copy of the viewport.
//!
//! Rows and cells are lending iterators. Each borrows the `RenderState` mutably, so none can
//! outlive the next [`RenderState::update`], which is exactly how long the library keeps row
//! and cell data valid. The terminal is only needed during `update`.

use std::marker::PhantomData;
use std::ptr::{self, NonNull};

use super::terminal::Terminal;
use super::{GhosttyError, Rgb, Style, check, no_value, sized, sys};

/// How much of the frame changed since the render state was last cleaned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Dirty {
    /// Nothing changed.
    Clean,
    /// Some rows changed; their dirty flags say which.
    Partial,
    /// Global state changed; redraw everything.
    Full,
}

impl Dirty {
    fn from_raw(raw: sys::GhosttyRenderStateDirty) -> Self {
        match raw {
            sys::GHOSTTY_RENDER_STATE_DIRTY_FALSE => Self::Clean,
            sys::GHOSTTY_RENDER_STATE_DIRTY_PARTIAL => Self::Partial,
            _ => Self::Full,
        }
    }

    fn to_raw(self) -> sys::GhosttyRenderStateDirty {
        match self {
            Self::Clean => sys::GHOSTTY_RENDER_STATE_DIRTY_FALSE,
            Self::Partial => sys::GHOSTTY_RENDER_STATE_DIRTY_PARTIAL,
            Self::Full => sys::GHOSTTY_RENDER_STATE_DIRTY_FULL,
        }
    }
}

/// The cursor as it should be drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cursor {
    /// Viewport column and row, `None` when the cursor is outside the viewport.
    pub position: Option<(u16, u16)>,
    /// Whether the cursor sits on the right half of a wide character.
    pub wide_tail: bool,
    /// DECTCEM.
    pub visible: bool,
    pub blinking: bool,
    /// Whether the cursor is at a password prompt.
    pub password_input: bool,
    pub shape: CursorShape,
}

/// The cursor's visual shape (DECSCUSR).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CursorShape {
    Bar,
    Block,
    Underline,
    BlockHollow,
}

/// The frame's colors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Colors {
    pub background: Rgb,
    pub foreground: Rgb,
    /// Set only when the program or configuration chose a cursor color.
    pub cursor: Option<Rgb>,
    pub palette: Box<[Rgb; 256]>,
}

/// A row's identity across updates; compare with `==`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct RowId([u64; 2]);

/// How a cell is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CellWide {
    /// One column.
    Narrow,
    /// The left half of a two-column character.
    Wide,
    /// The right half of a wide character; draw nothing.
    SpacerTail,
    /// Padding at the end of a soft-wrapped row whose wide character moved to the next row.
    SpacerHead,
}

/// What OSC 133 marked a cell as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticContent {
    Output,
    Input,
    Prompt,
}

/// A copy of the viewport for drawing one frame.
pub(crate) struct RenderState {
    raw: NonNull<sys::GhosttyRenderStateImpl>,
    rows: NonNull<sys::GhosttyRenderStateRowIteratorImpl>,
    cells: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
}

impl Drop for RenderState {
    fn drop(&mut self) {
        // SAFETY: owned handles; no borrow of self outlives this.
        unsafe {
            sys::ghostty_render_state_row_cells_free(self.cells.as_ptr());
            sys::ghostty_render_state_row_iterator_free(self.rows.as_ptr());
            sys::ghostty_render_state_free(self.raw.as_ptr());
        }
    }
}

impl std::fmt::Debug for RenderState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RenderState")
            .field("dirty", &self.dirty())
            .finish_non_exhaustive()
    }
}

impl RenderState {
    pub fn new() -> Result<Self, GhosttyError> {
        // Allocate all three before wrapping, freeing what exists on failure.
        let mut raw: sys::GhosttyRenderState = ptr::null_mut();
        let mut rows: sys::GhosttyRenderStateRowIterator = ptr::null_mut();
        let mut cells: sys::GhosttyRenderStateRowCells = ptr::null_mut();
        // SAFETY: default allocator, out pointers valid. Freeing NULL is allowed.
        unsafe {
            let result = check(sys::ghostty_render_state_new(ptr::null(), &raw mut raw))
                .and_then(|()| {
                    check(sys::ghostty_render_state_row_iterator_new(
                        ptr::null(),
                        &raw mut rows,
                    ))
                })
                .and_then(|()| {
                    check(sys::ghostty_render_state_row_cells_new(
                        ptr::null(),
                        &raw mut cells,
                    ))
                });
            match (
                result,
                NonNull::new(raw),
                NonNull::new(rows),
                NonNull::new(cells),
            ) {
                (Ok(()), Some(raw), Some(rows), Some(cells)) => Ok(Self { raw, rows, cells }),
                (result, ..) => {
                    sys::ghostty_render_state_row_cells_free(cells);
                    sys::ghostty_render_state_row_iterator_free(rows);
                    sys::ghostty_render_state_free(raw);
                    Err(result.err().unwrap_or(GhosttyError::OutOfMemory))
                }
            }
        }
    }

    fn raw(&self) -> sys::GhosttyRenderState {
        self.raw.as_ptr()
    }

    /// Copies the terminal's viewport and consumes its dirty state.
    pub fn update(&mut self, terminal: &mut Terminal) -> Result<Dirty, GhosttyError> {
        // SAFETY: both handles live; `&mut self` ends every row/cell borrow of the old data.
        check(unsafe { sys::ghostty_render_state_update(self.raw(), terminal.as_raw()) })?;
        Ok(self.dirty())
    }

    /// Reads a value.
    ///
    /// # Safety
    ///
    /// `T` must be the documented output type for `key`, valid when zeroed.
    unsafe fn get<T>(&self, key: sys::GhosttyRenderStateData) -> Option<T> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: `out` has the documented type.
        let result =
            unsafe { sys::ghostty_render_state_get(self.raw(), key, (&raw mut out).cast()) };
        no_value(result, out).ok().flatten()
    }

    pub fn dirty(&self) -> Dirty {
        // SAFETY: GhosttyRenderStateDirty (int).
        let raw = unsafe { self.get(sys::GHOSTTY_RENDER_STATE_DATA_DIRTY) };
        raw.map_or(Dirty::Full, Dirty::from_raw)
    }

    /// Overrides the global dirty state, for partial consumers.
    pub fn set_dirty(&mut self, dirty: Dirty) {
        let raw = dirty.to_raw();
        // SAFETY: OPT_DIRTY takes GhosttyRenderStateDirty*.
        unsafe {
            sys::ghostty_render_state_set(
                self.raw(),
                sys::GHOSTTY_RENDER_STATE_OPT_DIRTY,
                (&raw const raw).cast(),
            );
        }
    }

    /// Marks the frame drawn: global and per-row dirty flags become clean.
    pub fn clean(&mut self) {
        // SAFETY: live handle.
        unsafe { sys::ghostty_render_state_clean(self.raw()) };
    }

    /// Viewport size in cells.
    pub fn size(&self) -> (u16, u16) {
        // SAFETY: both uint16_t.
        unsafe {
            (
                self.get(sys::GHOSTTY_RENDER_STATE_DATA_COLS).unwrap_or(0),
                self.get(sys::GHOSTTY_RENDER_STATE_DATA_ROWS).unwrap_or(0),
            )
        }
    }

    pub fn cursor(&self) -> Cursor {
        // SAFETY: a sized struct, plain data.
        let mut raw: sys::GhosttyRenderStateCursor = unsafe { sized() };
        // SAFETY: DATA_CURSOR writes the sized struct.
        let ok = check(unsafe {
            sys::ghostty_render_state_get(
                self.raw(),
                sys::GHOSTTY_RENDER_STATE_DATA_CURSOR,
                (&raw mut raw).cast(),
            )
        })
        .is_ok();
        let has_position = ok && raw.viewport_has_value;
        Cursor {
            // viewport_x/y and wide_tail are undefined without viewport_has_value.
            position: has_position.then_some((raw.viewport_x, raw.viewport_y)),
            wide_tail: has_position && raw.wide_tail,
            visible: ok && raw.visible,
            blinking: ok && raw.blinking,
            password_input: ok && raw.password_input,
            shape: match raw.visual_style {
                sys::GHOSTTY_CURSOR_STYLE_BAR => CursorShape::Bar,
                sys::GHOSTTY_CURSOR_STYLE_UNDERLINE => CursorShape::Underline,
                sys::GHOSTTY_CURSOR_STYLE_BLOCK_HOLLOW => CursorShape::BlockHollow,
                _ => CursorShape::Block,
            },
        }
    }

    pub fn colors(&self) -> Colors {
        // SAFETY: a sized struct, plain data.
        let mut raw: sys::GhosttyRenderStateColors = unsafe { sized() };
        // SAFETY: DATA_COLORS writes the sized struct; on failure it stays zeroed.
        unsafe {
            sys::ghostty_render_state_get(
                self.raw(),
                sys::GHOSTTY_RENDER_STATE_DATA_COLORS,
                (&raw mut raw).cast(),
            );
        }
        Colors {
            background: raw.background.into(),
            foreground: raw.foreground.into(),
            // `cursor` is undefined without cursor_has_value.
            cursor: raw.cursor_has_value.then(|| raw.cursor.into()),
            palette: Box::new(raw.palette.map(Rgb::from)),
        }
    }

    /// The captured rows, top to bottom.
    pub fn rows(&mut self) -> Rows<'_> {
        let mut iter = self.rows.as_ptr();
        // SAFETY: populates our pre-allocated iterator with this state's rows. DATA_ROW_ITERATOR
        // takes a pointer to the iterator handle.
        let result = unsafe {
            sys::ghostty_render_state_get(
                self.raw(),
                sys::GHOSTTY_RENDER_STATE_DATA_ROW_ITERATOR,
                (&raw mut iter).cast(),
            )
        };
        Rows {
            iter: self.rows,
            cells: self.cells,
            valid: result == sys::GHOSTTY_SUCCESS,
            _state: PhantomData,
        }
    }
}

/// The rows of one update. Borrows the render state.
pub(crate) struct Rows<'s> {
    iter: NonNull<sys::GhosttyRenderStateRowIteratorImpl>,
    cells: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
    valid: bool,
    _state: PhantomData<&'s mut RenderState>,
}

impl Rows<'_> {
    /// The next row.
    #[allow(clippy::should_implement_trait)] // a lending iterator; Iterator cannot express it
    pub fn next(&mut self) -> Option<Row<'_>> {
        // SAFETY: the iterator was populated from the borrowed render state.
        let moved = self.valid
            && unsafe { sys::ghostty_render_state_row_iterator_next(self.iter.as_ptr()) };
        moved.then(|| self.row())
    }

    /// The next row that needs drawing: every row on a full redraw, dirty rows on a partial
    /// one, none when clean.
    pub fn next_dirty(&mut self) -> Option<Row<'_>> {
        let mut y = 0u16;
        // SAFETY: as `next`.
        let moved = self.valid
            && unsafe {
                sys::ghostty_render_state_row_iterator_next_dirty(self.iter.as_ptr(), &raw mut y)
            };
        moved.then(|| self.row())
    }

    fn row(&mut self) -> Row<'_> {
        Row {
            iter: self.iter,
            cells: self.cells,
            _rows: PhantomData,
        }
    }
}

/// One row. Borrows its [`Rows`].
pub(crate) struct Row<'r> {
    iter: NonNull<sys::GhosttyRenderStateRowIteratorImpl>,
    cells: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
    _rows: PhantomData<&'r mut ()>,
}

impl<'r> Row<'r> {
    /// # Safety
    ///
    /// `T` must be the documented output type for `key`, valid when zeroed.
    unsafe fn get<T>(&self, key: sys::GhosttyRenderStateRowData) -> Option<T> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: the iterator is positioned on this row; `out` has the documented type.
        let result = unsafe {
            sys::ghostty_render_state_row_get(self.iter.as_ptr(), key, (&raw mut out).cast())
        };
        no_value(result, out).ok().flatten()
    }

    fn raw_row(&self) -> sys::GhosttyRow {
        // SAFETY: GhosttyRow (uint64_t).
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_RAW) }.unwrap_or(0)
    }

    fn row_flag(&self, key: sys::GhosttyRowData) -> bool {
        let mut out = false;
        // SAFETY: the requested row keys write a bool.
        let result = unsafe { sys::ghostty_row_get(self.raw_row(), key, (&raw mut out).cast()) };
        result == sys::GHOSTTY_SUCCESS && out
    }

    pub fn is_dirty(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_DIRTY) }.unwrap_or(true)
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        // SAFETY: ROW_OPT_DIRTY takes bool*.
        unsafe {
            sys::ghostty_render_state_row_set(
                self.iter.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_OPT_DIRTY,
                (&raw const dirty).cast(),
            );
        }
    }

    /// Position relative to the top of the viewport.
    pub fn viewport_y(&self) -> i32 {
        // SAFETY: int32_t.
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_VIEWPORT_Y) }.unwrap_or(0)
    }

    pub fn id(&self) -> Option<RowId> {
        // SAFETY: GhosttyRenderStateRowId, plain data. A zeroed id is never valid.
        let raw: sys::GhosttyRenderStateRowId =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_DATA_ID) }?;
        (raw.bits != [0, 0]).then_some(RowId(raw.bits))
    }

    /// The row's packed cell values, one per column, in one call. Decode them with
    /// [`ghostty_cell_get`](sys::ghostty_cell_get) or a layout checked against the library.
    /// Valid as long as the render state is borrowed.
    pub fn raw_cells(&self) -> &'r [sys::GhosttyCell] {
        // SAFETY: GhosttyCellsView is plain data; zeroed is an empty view.
        let mut view: sys::GhosttyCellsView = unsafe { std::mem::zeroed() };
        // SAFETY: ROW_DATA_CELLS_RAW writes a view borrowed from the render state, valid until
        // its next update, which the `'r` borrow of the render state rules out.
        let result = unsafe {
            sys::ghostty_render_state_row_get(
                self.iter.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS_RAW,
                (&raw mut view).cast(),
            )
        };
        if result != sys::GHOSTTY_SUCCESS || view.ptr.is_null() || view.len == 0 {
            return &[];
        }
        // SAFETY: `len` contiguous cells, valid for 'r as above.
        unsafe { std::slice::from_raw_parts(view.ptr, view.len) }
    }

    /// Whether this row soft-wraps into the next.
    pub fn wraps(&self) -> bool {
        self.row_flag(sys::GHOSTTY_ROW_DATA_WRAP)
    }

    /// Whether this row continues a soft-wrapped one.
    pub fn is_wrap_continuation(&self) -> bool {
        self.row_flag(sys::GHOSTTY_ROW_DATA_WRAP_CONTINUATION)
    }

    /// Whether any cell may have a hyperlink (false positives possible).
    pub fn may_have_hyperlink(&self) -> bool {
        self.row_flag(sys::GHOSTTY_ROW_DATA_HYPERLINK)
    }

    /// The selected columns of this row, inclusive.
    pub fn selection(&self) -> Option<(u16, u16)> {
        // SAFETY: a sized struct, plain data.
        let mut raw: sys::GhosttyRenderStateRowSelection = unsafe { sized() };
        // SAFETY: ROW_DATA_SELECTION writes the sized struct; NO_VALUE when unselected.
        let result = unsafe {
            sys::ghostty_render_state_row_get(
                self.iter.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_DATA_SELECTION,
                (&raw mut raw).cast(),
            )
        };
        no_value(result, (raw.start_x, raw.end_x)).ok().flatten()
    }

    /// The row's cells, left to right.
    pub fn cells(&mut self) -> Cells<'_> {
        let mut cells = self.cells.as_ptr();
        // SAFETY: populates our pre-allocated cells handle for this row. ROW_DATA_CELLS takes a
        // pointer to the handle.
        let result = unsafe {
            sys::ghostty_render_state_row_get(
                self.iter.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_DATA_CELLS,
                (&raw mut cells).cast(),
            )
        };
        Cells {
            cells: self.cells,
            valid: result == sys::GHOSTTY_SUCCESS,
            _row: PhantomData,
        }
    }
}

/// The cells of one row. Borrows its [`Row`].
pub(crate) struct Cells<'c> {
    cells: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
    valid: bool,
    _row: PhantomData<&'c mut ()>,
}

impl Cells<'_> {
    /// The next cell.
    #[allow(clippy::should_implement_trait)] // a lending iterator; Iterator cannot express it
    pub fn next(&mut self) -> Option<CellRef<'_>> {
        // SAFETY: the handle was populated from the borrowed row.
        let moved =
            self.valid && unsafe { sys::ghostty_render_state_row_cells_next(self.cells.as_ptr()) };
        moved.then(|| self.cell())
    }

    /// The cell at column `x`.
    pub fn select(&mut self, x: u16) -> Option<CellRef<'_>> {
        // SAFETY: as `next`; out-of-range columns are rejected.
        let ok = self.valid
            && unsafe { sys::ghostty_render_state_row_cells_select(self.cells.as_ptr(), x) }
                == sys::GHOSTTY_SUCCESS;
        ok.then(|| self.cell())
    }

    fn cell(&mut self) -> CellRef<'_> {
        CellRef {
            cells: self.cells,
            _cells: PhantomData,
        }
    }
}

/// One cell. Borrows its [`Cells`].
pub(crate) struct CellRef<'x> {
    cells: NonNull<sys::GhosttyRenderStateRowCellsImpl>,
    _cells: PhantomData<&'x mut ()>,
}

impl CellRef<'_> {
    /// # Safety
    ///
    /// `T` must be the documented output type for `key`, valid when zeroed.
    unsafe fn get<T>(&self, key: sys::GhosttyRenderStateRowCellsData) -> Option<T> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: the handle is positioned on this cell; `out` has the documented type.
        let result = unsafe {
            sys::ghostty_render_state_row_cells_get(self.cells.as_ptr(), key, (&raw mut out).cast())
        };
        no_value(result, out).ok().flatten()
    }

    fn raw(&self) -> sys::GhosttyCell {
        // SAFETY: GhosttyCell (uint64_t).
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_RAW) }.unwrap_or(0)
    }

    /// # Safety
    ///
    /// `T` must be the documented output type for `key`, valid when zeroed.
    unsafe fn cell_get<T>(&self, key: sys::GhosttyCellData) -> Option<T> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: a cell value is plain data; `out` has the documented type.
        let result = unsafe { sys::ghostty_cell_get(self.raw(), key, (&raw mut out).cast()) };
        no_value(result, out).ok().flatten()
    }

    /// Appends the cell's grapheme cluster to `out`; appends nothing for an empty cell.
    pub fn push_text(&self, out: &mut String) {
        let mut stack = [0u8; 32];
        let mut buf = sys::GhosttyBuffer {
            ptr: stack.as_mut_ptr(),
            cap: stack.len(),
            len: 0,
        };
        // SAFETY: GRAPHEMES_UTF8 writes into our buffer, or reports the size it needs.
        let result = unsafe {
            sys::ghostty_render_state_row_cells_get(
                self.cells.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                (&raw mut buf).cast(),
            )
        };
        if result == sys::GHOSTTY_SUCCESS {
            push_utf8(out, &stack[..buf.len.min(stack.len())]);
            return;
        }
        if result != sys::GHOSTTY_OUT_OF_SPACE {
            return;
        }
        let mut heap = vec![0u8; buf.len];
        buf = sys::GhosttyBuffer {
            ptr: heap.as_mut_ptr(),
            cap: heap.len(),
            len: 0,
        };
        // SAFETY: as above, with a buffer of the reported size.
        let result = unsafe {
            sys::ghostty_render_state_row_cells_get(
                self.cells.as_ptr(),
                sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_GRAPHEMES_UTF8,
                (&raw mut buf).cast(),
            )
        };
        if result == sys::GHOSTTY_SUCCESS {
            push_utf8(out, &heap[..buf.len.min(heap.len())]);
        }
    }

    /// The cell's grapheme cluster, empty for an empty cell.
    pub fn text(&self) -> String {
        let mut out = String::new();
        self.push_text(&mut out);
        out
    }

    /// The first codepoint, 0 for an empty cell.
    pub fn codepoint(&self) -> u32 {
        // SAFETY: uint32_t.
        unsafe { self.cell_get(sys::GHOSTTY_CELL_DATA_CODEPOINT) }.unwrap_or(0)
    }

    pub fn has_text(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.cell_get(sys::GHOSTTY_CELL_DATA_HAS_TEXT) }.unwrap_or(false)
    }

    pub fn wide(&self) -> CellWide {
        // SAFETY: GhosttyCellWide (int).
        let raw: sys::GhosttyCellWide =
            unsafe { self.cell_get(sys::GHOSTTY_CELL_DATA_WIDE) }.unwrap_or(0);
        match raw {
            sys::GHOSTTY_CELL_WIDE_WIDE => CellWide::Wide,
            sys::GHOSTTY_CELL_WIDE_SPACER_TAIL => CellWide::SpacerTail,
            sys::GHOSTTY_CELL_WIDE_SPACER_HEAD => CellWide::SpacerHead,
            _ => CellWide::Narrow,
        }
    }

    /// Whether the cell has an OSC 8 hyperlink. Read the URI with
    /// [`Terminal::hyperlink_uri`](super::Terminal::hyperlink_uri) at its viewport point.
    pub fn has_hyperlink(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.cell_get(sys::GHOSTTY_CELL_DATA_HAS_HYPERLINK) }.unwrap_or(false)
    }

    pub fn semantic_content(&self) -> SemanticContent {
        // SAFETY: GhosttyCellSemanticContent (int).
        let raw: sys::GhosttyCellSemanticContent =
            unsafe { self.cell_get(sys::GHOSTTY_CELL_DATA_SEMANTIC_CONTENT) }.unwrap_or(0);
        match raw {
            sys::GHOSTTY_CELL_SEMANTIC_INPUT => SemanticContent::Input,
            sys::GHOSTTY_CELL_SEMANTIC_PROMPT => SemanticContent::Prompt,
            _ => SemanticContent::Output,
        }
    }

    pub fn has_styling(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_HAS_STYLING) }.unwrap_or(false)
    }

    /// The full style. Default for unstyled cells.
    pub fn style(&self) -> Style {
        // SAFETY: GhosttyStyle, plain data.
        let raw: Option<sys::GhosttyStyle> =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_STYLE) };
        raw.map(|s| Style::from_raw(&s)).unwrap_or_default()
    }

    /// The resolved foreground (palette looked up, bold not applied); `None` for the default.
    pub fn fg(&self) -> Option<Rgb> {
        // SAFETY: GhosttyColorRgb.
        let raw: Option<sys::GhosttyColorRgb> =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_FG_COLOR) };
        raw.map(Rgb::from)
    }

    /// The resolved background, including background-only cells; `None` for the default.
    pub fn bg(&self) -> Option<Rgb> {
        // SAFETY: GhosttyColorRgb.
        let raw: Option<sys::GhosttyColorRgb> =
            unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_BG_COLOR) };
        raw.map(Rgb::from)
    }

    pub fn is_selected(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get(sys::GHOSTTY_RENDER_STATE_ROW_CELLS_DATA_SELECTED) }.unwrap_or(false)
    }
}

fn push_utf8(out: &mut String, bytes: &[u8]) {
    match std::str::from_utf8(bytes) {
        Ok(s) => out.push_str(s),
        Err(_) => out.push_str(&String::from_utf8_lossy(bytes)),
    }
}
