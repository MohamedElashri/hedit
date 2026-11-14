//! Logical row offsets shared by cursor conversions. The cache is derived data:
//! it is omitted from serialization and equality and shared by galley clones.
use super::{Galley, Row};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Default)]
pub struct CursorIndexCache(OnceLock<Arc<CursorIndex>>);
impl std::fmt::Debug for CursorIndexCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CursorIndexCache")
    }
}
impl PartialEq for CursorIndexCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}
impl Eq for CursorIndexCache {}
impl CursorIndexCache {
    /// Approximate initialized metadata, without initializing the cache.
    pub fn bytes(&self) -> usize {
        self.0.get().map_or(0, |index| {
            std::mem::size_of::<CursorIndex>()
                + index.rows.capacity() * std::mem::size_of::<CursorRow>()
        })
    }
}
#[derive(Clone, Copy, Debug)]
pub struct CursorRow {
    pub start: usize,
    pub paragraph: usize,
    pub offset: usize,
}
#[derive(Debug)]
pub struct CursorIndex {
    pub rows: Vec<CursorRow>,
    pub end: CursorRow,
    /// Binary pointer lookup is only valid for finite, non-overlapping rows.
    pub(crate) disjoint: bool,
}
impl CursorIndex {
    fn new(rows: &[Arc<Row>]) -> Self {
        let mut index = Self {
            rows: Vec::with_capacity(rows.len()),
            end: CursorRow {
                start: 0,
                paragraph: 0,
                offset: 0,
            },
            disjoint: true,
        };
        let mut bottom = f32::NEG_INFINITY;
        for row in rows {
            index.rows.push(index.end);
            index.end.start += row.char_count_including_newline();
            if row.ends_with_newline {
                index.end.paragraph += 1;
                index.end.offset = 0;
            } else {
                index.end.offset += row.char_count_including_newline();
            }
            index.disjoint &= row.min_y().is_finite()
                && row.max_y().is_finite()
                && bottom <= row.min_y()
                && row.min_y() <= row.max_y();
            bottom = row.max_y();
        }
        index
    }
}
impl Galley {
    /// Cached offsets for the current row topology. Invalidate after changing
    /// glyph counts, newline flags, row order or vertical row geometry.
    pub fn cursor_index(&self) -> &CursorIndex {
        let index = self
            .cursor_cache
            .0
            .get_or_init(|| Arc::new(CursorIndex::new(&self.rows)));
        debug_assert_eq!(
            index.rows.len(),
            self.rows.len(),
            "invalidate the cursor index after changing rows"
        );
        index
    }
    /// Share offsets without retaining the galley's glyphs or meshes.
    pub fn shared_cursor_index(&self) -> Arc<CursorIndex> {
        self.cursor_index();
        Arc::clone(self.cursor_cache.0.get().unwrap())
    }
    pub fn invalidate_cursor_index(&mut self) {
        self.cursor_cache = CursorIndexCache::default();
    }
    pub fn row_start(&self, row: usize) -> usize {
        let index = self.cursor_index();
        index.rows.get(row).unwrap_or(&index.end).start
    }
    /// First row ending strictly below `y`, or the last row. Used by Hedit's
    /// RTL hit testing, which intentionally treats shared boundaries differently
    /// from TextEdit's nearest-row selection.
    pub fn row_at_y(&self, y: f32) -> usize {
        if self.cursor_index().disjoint && y.is_finite() {
            self.rows
                .partition_point(|row| row.max_y() <= y)
                .min(self.rows.len().saturating_sub(1))
        } else {
            self.rows
                .iter()
                .position(|row| y < row.max_y())
                .unwrap_or(self.rows.len().saturating_sub(1))
        }
    }
}
