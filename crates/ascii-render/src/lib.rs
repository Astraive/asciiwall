//! Bounded GPU presentation of immutable ASCII character frames.
#![forbid(unsafe_code)]

mod renderer;
mod surface;

pub use renderer::{DeviceStatus, Error, PresenterStats, Renderer, SHADER_REVISION};
pub use surface::{PresentOutcome, RecoverableReason, RenderTarget, SkippedReason};
