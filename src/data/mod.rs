//! Measurement storage.

pub mod history;
pub mod soapy_bin;
pub mod storage;

pub use history::HistoryBuffer;
pub use storage::{Baseline, BinMismatch, DataStorage, Updated};
