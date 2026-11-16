//! Typed failures shared by the portable core contracts.

/// The malformed-input condition reported by a validator.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MalformedReason {
    /// A frame or raster has a zero dimension.
    EmptyDimensions,
    /// A frame's cell count does not match its dimensions.
    CellCountMismatch,
    /// A frame is bound to a different glyph table than the caller expects.
    GlyphTableMismatch,
    /// A frame references a glyph outside the supplied table's bounds.
    GlyphIndexOutOfRange,
    /// A raster row stride is shorter than its pixel row.
    RowStrideTooSmall,
    /// A raster backing slice is shorter than the last required pixel byte.
    RasterBackingTooShort,
    /// Caller-supplied input configuration violates its own bounds.
    InvalidConfiguration,
    /// Encoded input data is empty, unreadable, or structurally invalid.
    InvalidEncodedData,
}

/// A supported-core capability that the supplied input requires but this API does not support.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UnsupportedReason {
    /// The raster's channel layout is not supported.
    PixelFormat,
    /// The raster's sample depth is not supported.
    SampleDepth,
    /// The raster orientation is not supported without transforming the pixels.
    Orientation,
    /// The raster color primaries or transfer function are not supported.
    ColorDescription,
    /// The alpha mode does not match the raster's channel layout.
    AlphaMode,
    /// The input requires a format or feature this API does not support.
    UnsupportedFormat,
}
/// A glyph-specific single-cell validation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GlyphFailureReason {
    /// The scalar is outside the strict supported repertoire.
    UnsupportedScalar,
    /// The repertoire contains the same scalar more than once.
    DuplicateScalar,
    /// The selected font has no glyph for the requested scalar.
    GlyphUnavailable,
    /// The glyph's measured advance exceeds one configured cell.
    MultiCellMetrics,
}

/// The bounded resource that was exhausted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BudgetKind {
    /// Owned ASCII cell-vector capacity, measured in bytes.
    FrameBytes,
    /// The size of an encoded input asset, measured in bytes.
    EncodedBytes,
    /// The minimum readable raster extent, excluding trailing bytes and final-row padding.
    RasterBytes,
    /// The bounded per-cell or per-pixel validation work estimate.
    WorkUnits,
    /// The shared number of active decoding jobs has reached its limit.
    DecodeConcurrency,
}

/// A typed validation or operation failure.
///
/// Permission, missing-asset, and temporary-device variants are available to portable-core
/// consumers so callers can preserve those categories across engine boundaries. This crate
/// itself performs no I/O and therefore does not produce those three variants.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Input violates a core data invariant.
    Malformed(MalformedReason),
    /// Input requires metadata or a capability not supported by this core API.
    Unsupported(UnsupportedReason),
    /// A configured byte or work limit would be exceeded.
    BudgetExceeded(BudgetKind),
    /// A bounded allocation failed despite passing preflight checks.
    AllocationFailed,
    /// Checked size arithmetic, platform-size conversion, or allocation-layout validation failed.
    SizeOverflow,
    /// The operation was cancelled before it completed.
    Cancelled,
    /// The requested operation was denied by a permission boundary.
    PermissionDenied,
    /// A required external asset was not available.
    MissingAsset,
    /// A required device or service is temporarily unavailable.
    TemporarilyUnavailable,
    /// A repertoire or font glyph failure, including the offending Unicode scalar.
    Glyph {
        /// The scalar rejected by the single-cell contract.
        character: char,
        /// Why the scalar or its selected-font metrics failed validation.
        reason: GlyphFailureReason,
    },
}
impl core::fmt::Display for Error {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Malformed(_) => formatter.write_str("malformed input"),
            Self::Unsupported(_) => formatter.write_str("unsupported input"),
            Self::BudgetExceeded(_) => formatter.write_str("resource budget exceeded"),
            Self::Glyph { character, reason } => {
                let reason = match reason {
                    GlyphFailureReason::UnsupportedScalar => "is outside the supported repertoire",
                    GlyphFailureReason::DuplicateScalar => {
                        "appears more than once in the repertoire"
                    }
                    GlyphFailureReason::GlyphUnavailable => "is missing from the selected font",
                    GlyphFailureReason::MultiCellMetrics => "has an advance wider than one cell",
                };
                write!(formatter, "glyph U+{:04X} {reason}", *character as u32)
            }
            Self::AllocationFailed => formatter.write_str("allocation failed"),
            Self::SizeOverflow => formatter.write_str("size overflow"),
            Self::Cancelled => formatter.write_str("operation cancelled"),
            Self::PermissionDenied => formatter.write_str("permission denied"),
            Self::MissingAsset => formatter.write_str("required asset is missing"),
            Self::TemporarilyUnavailable => {
                formatter.write_str("required device or service is temporarily unavailable")
            }
        }
    }
}

impl core::error::Error for Error {}
