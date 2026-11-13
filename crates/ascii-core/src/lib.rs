//! Dependency-free, portable base crate for AsciiWall.
//!
//! API-level frame, raster, budget, cancellation, and error contracts are owned by COR-001.
#![no_std]
#![forbid(unsafe_code)]
#[cfg(test)]
extern crate std;
/// The dimensions of an ASCII cell grid.
///
/// The dimensions remain independent of any allocation; callers must apply their
/// own resource limits before materializing a grid.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct GridSize {
    columns: u32,
    rows: u32,
}

impl GridSize {
    /// Creates a grid size from its column and row counts.
    #[must_use]
    pub const fn new(columns: u32, rows: u32) -> Self {
        Self { columns, rows }
    }

    /// Returns the number of columns.
    #[must_use]
    pub const fn columns(self) -> u32 {
        self.columns
    }

    /// Returns the number of rows.
    #[must_use]
    pub const fn rows(self) -> u32 {
        self.rows
    }
    /// Returns whether either dimension is zero.
    ///
    /// Empty sizes are representable as geometry; frame validation must reject them before allocation.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.columns == 0 || self.rows == 0
    }

    /// Returns the exact number of cells as a `u64`.
    ///
    /// The product of two `u32` values always fits in `u64`.
    #[must_use]
    pub const fn cell_count(self) -> u64 {
        self.columns as u64 * self.rows as u64
    }
}

#[cfg(test)]
mod tests {
    use super::GridSize;

    #[test]
    fn grid_size_reports_its_dimensions_and_cell_count() {
        let size = GridSize::new(4, 4);

        std::assert_eq!(size.columns(), 4);
        std::assert_eq!(size.rows(), 4);
        std::assert_eq!(size.cell_count(), 16);
    }
    #[test]
    fn grid_size_detects_empty_dimensions() {
        std::assert!(GridSize::new(0, 10).is_empty());
        std::assert!(GridSize::new(10, 0).is_empty());
        std::assert!(!GridSize::new(10, 10).is_empty());
    }

    #[test]
    fn grid_size_cell_count_handles_maximum_dimensions() {
        let size = GridSize::new(u32::MAX, u32::MAX);

        std::assert_eq!(size.cell_count(), u32::MAX as u64 * u32::MAX as u64);
    }
}
