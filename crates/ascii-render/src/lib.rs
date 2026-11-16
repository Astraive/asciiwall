//! Safe text and restricted ANSI serialization for validated ASCII frames.
//!
//! Export never writes to a destination. It returns a complete bounded buffer so a caller can
//! publish it through its own destination transaction.

mod export;

pub use export::{AnsiColorMode, ExportError, ExportOutput, ExportTarget, export};
