//! Fixed-size sweep history.
//!
//! QSpectrumAnalyzer's `HistoryBuffer` called `np.roll()` on every append, which
//! copies the whole `rows x bins` array each time -- the dominant cost of a
//! large sweep. This is a real ring buffer: appending touches one row, and the
//! waterfall uploads that one row to the GPU and tells the shader where the
//! write head is.

/// A `capacity x bins` ring of sweep rows.
#[derive(Clone, Debug, Default)]
pub struct HistoryBuffer {
    data: Vec<f32>,
    bins: usize,
    capacity: usize,
    /// Rows actually written, saturating at `capacity`.
    len: usize,
    /// Row index the next append will write to.
    head: usize,
    /// Rows appended over the buffer's whole life.
    counter: u64,
}

impl HistoryBuffer {
    pub fn new(bins: usize, capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            data: vec![0.0; bins * capacity],
            bins,
            capacity,
            len: 0,
            head: 0,
            counter: 0,
        }
    }

    pub fn bins(&self) -> usize {
        self.bins
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Number of rows currently held.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Total rows ever appended, including evicted ones.
    pub fn counter(&self) -> u64 {
        self.counter
    }

    /// Row index the *next* append will occupy. The shader needs this to know
    /// where the ring wraps.
    pub fn head(&self) -> usize {
        self.head
    }

    /// The whole backing store, laid out as `capacity` consecutive rows.
    ///
    /// Row `i` of this slice is *not* the i-th newest sweep -- use
    /// [`row_index_from_newest`](Self::row_index_from_newest) to map. The GPU
    /// waterfall uploads into this layout directly.
    pub fn raw(&self) -> &[f32] {
        &self.data
    }

    /// Drop every row but keep the allocation and geometry.
    pub fn clear(&mut self) {
        self.len = 0;
        self.head = 0;
        self.counter = 0;
        self.data.fill(0.0);
    }

    /// Re-shape, reusing the allocation when the total size happens to match.
    pub fn reshape(&mut self, bins: usize, capacity: usize) {
        let capacity = capacity.max(1);
        self.bins = bins;
        self.capacity = capacity;
        self.data.clear();
        self.data.resize(bins * capacity, 0.0);
        self.len = 0;
        self.head = 0;
        self.counter = 0;
    }

    /// Append one sweep, evicting the oldest when full.
    ///
    /// Returns the row index written, which is what the waterfall uploads.
    /// Rows shorter than `bins` are zero-filled; longer ones are truncated, so
    /// a backend that changes its bin count mid-run cannot corrupt the buffer.
    pub fn append(&mut self, row: &[f32]) -> usize {
        let index = self.head;
        let start = index * self.bins;
        let end = start + self.bins;
        let dst = &mut self.data[start..end];

        let n = row.len().min(self.bins);
        dst[..n].copy_from_slice(&row[..n]);
        dst[n..].fill(0.0);

        self.head = (self.head + 1) % self.capacity;
        self.len = (self.len + 1).min(self.capacity);
        self.counter += 1;
        index
    }

    /// Backing-store index of the `age`-th newest row (`0` is newest).
    pub fn row_index_from_newest(&self, age: usize) -> Option<usize> {
        if age >= self.len {
            return None;
        }
        Some((self.head + self.capacity - 1 - age) % self.capacity)
    }

    /// The `age`-th newest row (`0` is newest).
    pub fn row_from_newest(&self, age: usize) -> Option<&[f32]> {
        let i = self.row_index_from_newest(age)?;
        Some(&self.data[i * self.bins..(i + 1) * self.bins])
    }

    /// The newest row.
    pub fn newest(&self) -> Option<&[f32]> {
        self.row_from_newest(0)
    }

    /// Rows from oldest to newest -- the order
    /// `HistoryBuffer.get_buffer()` returned.
    pub fn iter_rows(&self) -> impl Iterator<Item = &[f32]> + '_ {
        (0..self.len)
            .rev()
            .filter_map(move |age| self.row_from_newest(age))
    }

    /// Apply `f` to every live row. Used when the baseline changes and the
    /// stored history has to be re-levelled in place.
    pub fn for_each_row_mut(&mut self, mut f: impl FnMut(&mut [f32])) {
        if self.len == 0 {
            return;
        }
        // Touch only the live rows: when the ring is not yet full the unwritten
        // tail must stay at zero so it never shows up in the waterfall.
        let (bins, capacity, head, len) = (self.bins, self.capacity, self.head, self.len);
        for age in 0..len {
            let i = (head + capacity - 1 - age) % capacity;
            f(&mut self.data[i * bins..(i + 1) * bins]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(h: &HistoryBuffer) -> Vec<Vec<f32>> {
        h.iter_rows().map(|r| r.to_vec()).collect()
    }

    #[test]
    fn fills_then_evicts_oldest() {
        let mut h = HistoryBuffer::new(2, 3);
        assert!(h.is_empty());

        h.append(&[1.0, 1.0]);
        h.append(&[2.0, 2.0]);
        assert_eq!(h.len(), 2);
        assert_eq!(rows(&h), vec![vec![1.0, 1.0], vec![2.0, 2.0]]);

        h.append(&[3.0, 3.0]);
        assert_eq!(h.len(), 3);
        assert_eq!(
            rows(&h),
            vec![vec![1.0, 1.0], vec![2.0, 2.0], vec![3.0, 3.0]]
        );

        // Full: the oldest row goes.
        h.append(&[4.0, 4.0]);
        assert_eq!(h.len(), 3);
        assert_eq!(
            rows(&h),
            vec![vec![2.0, 2.0], vec![3.0, 3.0], vec![4.0, 4.0]]
        );
        assert_eq!(h.counter(), 4);
    }

    #[test]
    fn newest_and_age_indexing() {
        let mut h = HistoryBuffer::new(1, 4);
        for i in 1..=6 {
            h.append(&[i as f32]);
        }
        assert_eq!(h.newest(), Some(&[6.0f32][..]));
        assert_eq!(h.row_from_newest(0), Some(&[6.0f32][..]));
        assert_eq!(h.row_from_newest(1), Some(&[5.0f32][..]));
        assert_eq!(h.row_from_newest(3), Some(&[3.0f32][..]));
        assert_eq!(h.row_from_newest(4), None);
    }

    #[test]
    fn append_returns_the_written_row_and_wraps() {
        let mut h = HistoryBuffer::new(1, 3);
        assert_eq!(h.append(&[0.0]), 0);
        assert_eq!(h.append(&[0.0]), 1);
        assert_eq!(h.append(&[0.0]), 2);
        assert_eq!(h.append(&[0.0]), 0);
        assert_eq!(h.head(), 1);
    }

    #[test]
    fn mismatched_row_lengths_are_padded_or_truncated() {
        let mut h = HistoryBuffer::new(3, 2);
        h.append(&[1.0]);
        assert_eq!(h.newest(), Some(&[1.0f32, 0.0, 0.0][..]));
        h.append(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(h.newest(), Some(&[1.0f32, 2.0, 3.0][..]));
    }

    #[test]
    fn for_each_row_mut_skips_unwritten_rows() {
        let mut h = HistoryBuffer::new(1, 4);
        h.append(&[10.0]);
        h.append(&[20.0]);
        h.for_each_row_mut(|r| r[0] -= 5.0);
        assert_eq!(rows(&h), vec![vec![5.0], vec![15.0]]);
        // The two never-written rows must still read as zero.
        assert_eq!(h.raw().iter().filter(|v| **v == 0.0).count(), 2);
    }

    #[test]
    fn clear_and_reshape_reset_state() {
        let mut h = HistoryBuffer::new(2, 2);
        h.append(&[1.0, 2.0]);
        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.head(), 0);
        assert_eq!(h.counter(), 0);

        h.reshape(5, 3);
        assert_eq!(h.bins(), 5);
        assert_eq!(h.capacity(), 3);
        assert_eq!(h.raw().len(), 15);
        assert!(h.is_empty());
    }

    #[test]
    fn zero_capacity_is_clamped_to_one() {
        let mut h = HistoryBuffer::new(1, 0);
        assert_eq!(h.capacity(), 1);
        h.append(&[1.0]);
        h.append(&[2.0]);
        assert_eq!(rows(&h), vec![vec![2.0]]);
    }
}
