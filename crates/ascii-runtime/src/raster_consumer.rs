//! Bounded process-local raster-to-ASCII conversion over [`BoundedHandoff`] publications.
//!
//! The consumer allocates one fixed scratch buffer at construction and charges it to the shared
//! [`ResourceBudget`]. Pixel bytes are copied while [`FrameLease::with_pixels`] holds the pool
//! lock; raster validation and `ascii-convert` work run only after that callback returns. A
//! successfully converted frame retains its acknowledged lease and output permit until the wrapper
//! is dropped. If conversion unwinds, the completion token is dropped unacknowledged, leaving the
//! source slot quarantined rather than falsely declaring completion. This remains an in-process
//! consumer and does not implement native mapping, transport ownership, disconnect recovery, or
//! device completion.

use std::vec::Vec;

use ascii_convert::{
    ConversionSettings, GlyphTable, GridSizing, LinearRgb, NormalizedRasterResources,
    convert_density,
};
use ascii_core::{
    AsciiFrame, Budget, CancellationToken, Error as CoreError, FrameMetadata, GlyphTableId,
    RasterFrameView, validate_raster_frame,
};

use crate::{
    BoundedHandoff, FrameDescriptor, FrameLease, MemoryPermit, MemoryUsage, ResourceBudget,
    RuntimeError,
};

/// Failures while receiving and converting one handoff frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RasterConsumerError {
    /// The runtime could not receive, lease, or account for the source frame.
    Runtime(RuntimeError),
    /// The published byte extent exceeds the consumer's fixed scratch bound.
    InputTooLarge {
        /// Published descriptor byte length.
        byte_length: u64,
        /// Configured scratch-buffer bound.
        max_bytes: usize,
    },
    /// Core raster validation or density conversion rejected the frame.
    Conversion(CoreError),
}

impl core::fmt::Display for RasterConsumerError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Runtime(error) => write!(formatter, "raster consumer runtime failure: {error}"),
            Self::InputTooLarge {
                byte_length,
                max_bytes,
            } => write!(
                formatter,
                "published raster has {byte_length} bytes; consumer scratch bound is {max_bytes}"
            ),
            Self::Conversion(error) => write!(formatter, "raster conversion failed: {error}"),
        }
    }
}

impl std::error::Error for RasterConsumerError {}

impl From<RuntimeError> for RasterConsumerError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

impl From<CoreError> for RasterConsumerError {
    fn from(error: CoreError) -> Self {
        Self::Conversion(error)
    }
}

/// A synchronous consumer with a fixed reusable raster-copy buffer.
///
/// Construct it with the same [`ResourceBudget`] used by the handoff so its scratch bytes share
/// the application's aggregate CPU-memory ceiling. The configured sizing and colors are fixed for
/// the consumer lifetime; create a new consumer when those conversion settings change.
#[derive(Debug)]
pub struct RasterToAsciiConsumer {
    resource_budget: ResourceBudget,
    scratch: Vec<u8>,
    scratch_bound: usize,
    _scratch_permit: MemoryPermit,
    sizing: GridSizing,
    foreground: LinearRgb,
    background: LinearRgb,
    normalized_resources: NormalizedRasterResources,
}

impl RasterToAsciiConsumer {
    /// Allocates and charges the single reusable scratch buffer.
    pub fn new(
        resource_budget: &ResourceBudget,
        max_input_bytes: usize,
        sizing: GridSizing,
        foreground: LinearRgb,
        background: LinearRgb,
        normalized_resources: NormalizedRasterResources,
    ) -> Result<Self, RasterConsumerError> {
        if max_input_bytes == 0 {
            return Err(RuntimeError::InvalidCapacity.into());
        }
        let charged_bytes =
            u64::try_from(max_input_bytes).map_err(|_| RuntimeError::SizeOverflow)?;
        if max_input_bytes > isize::MAX as usize {
            return Err(RuntimeError::SizeOverflow.into());
        }
        let scratch_permit =
            resource_budget.try_reserve_memory(MemoryUsage::new(charged_bytes, 0))?;
        let mut scratch = Vec::new();
        scratch
            .try_reserve_exact(max_input_bytes)
            .map_err(|_| RuntimeError::AllocationFailed)?;
        scratch.resize(max_input_bytes, 0);

        Ok(Self {
            resource_budget: resource_budget.clone(),
            scratch,
            scratch_bound: max_input_bytes,
            _scratch_permit: scratch_permit,
            sizing,
            foreground,
            background,
            normalized_resources,
        })
    }

    /// Returns the configured maximum number of source bytes copied into scratch.
    #[must_use]
    pub const fn max_input_bytes(&self) -> usize {
        self.scratch_bound
    }

    /// Receives and converts the next frame, or returns `None` when the bounded handoff is empty.
    ///
    /// The exact completion token is held across the source copy, checked-view construction, and
    /// conversion. Ordinary errors acknowledge and release the lease because this method owns no
    /// external work. A panic before conversion returns does not acknowledge: token and lease drop
    /// unacknowledged, preserving the runtime's quarantine behavior. On success the token is
    /// acknowledged after conversion and the returned wrapper keeps the lease/output permit live
    /// until its drop.
    ///
    /// Core metadata binds the glyph-table fingerprint, publication id as frame id, generation,
    /// and a nanosecond-to-microsecond timestamp (sub-microsecond remainder truncated). The
    /// returned descriptor retains the exact source timestamp and pool identity; duration is zero
    /// because the raster descriptor carries no duration.
    pub fn convert_next(
        &mut self,
        handoff: &BoundedHandoff,
        glyphs: &GlyphTable,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<Option<ConvertedFrame>, RasterConsumerError> {
        let Some(mut lease) = handoff.receive_frame(budget, cancellation)? else {
            return Ok(None);
        };
        let descriptor = *lease.descriptor();
        lease.pin_output()?;
        let token = lease.completion_token()?;
        let result = self.convert_lease(&lease, descriptor, glyphs, budget, cancellation);

        match result {
            Ok(frame) => {
                token.acknowledge();
                Ok(Some(ConvertedFrame {
                    frame,
                    descriptor,
                    _lease: lease,
                }))
            }
            Err(error) => {
                token.acknowledge();
                drop(lease);
                Err(error)
            }
        }
    }

    fn convert_lease(
        &mut self,
        lease: &FrameLease,
        descriptor: FrameDescriptor,
        glyphs: &GlyphTable,
        budget: &Budget,
        cancellation: &CancellationToken,
    ) -> Result<AsciiFrame, RasterConsumerError> {
        let byte_length =
            usize::try_from(descriptor.byte_length).map_err(|_| RuntimeError::MappingBounds)?;
        if byte_length > self.scratch_bound {
            return Err(RasterConsumerError::InputTooLarge {
                byte_length: descriptor.byte_length,
                max_bytes: self.scratch_bound,
            });
        }

        lease
            .with_pixels(|pixels| {
                if pixels.len() != byte_length {
                    return Err(RasterConsumerError::Runtime(
                        RuntimeError::InvalidDescriptor,
                    ));
                }
                self.scratch[..byte_length].copy_from_slice(pixels);
                Ok(())
            })
            .map_err(RasterConsumerError::Runtime)??;

        let row_stride =
            usize::try_from(descriptor.row_stride).map_err(|_| RuntimeError::MappingBounds)?;
        let input = RasterFrameView::new(
            descriptor.width,
            descriptor.height,
            row_stride,
            descriptor.format,
            descriptor.color,
            descriptor.orientation,
            &self.scratch[..byte_length],
        );
        let timestamp_us = i64::try_from(descriptor.media_timestamp_ns / 1_000)
            .map_err(|_| RuntimeError::SizeOverflow)?;
        let metadata = FrameMetadata::new(
            GlyphTableId::new(*glyphs.fingerprint()),
            descriptor.publication_id,
            descriptor.generation,
            timestamp_us,
            0,
        );

        // Keep the aggregate CPU permit over both checked raster validation and the real converter.
        let _cpu_permit = self.resource_budget.try_acquire_cpu()?;
        let validated = validate_raster_frame(input, budget, cancellation)?;
        let settings = ConversionSettings::new(
            self.sizing,
            metadata,
            cancellation,
            self.foreground,
            self.background,
        )?;
        convert_density(
            validated.view(),
            glyphs,
            &settings,
            budget,
            &self.normalized_resources,
        )
        .map_err(RasterConsumerError::Conversion)
    }
}

/// An owned ASCII result that retains its acknowledged source lease and output permit.
///
/// Only borrowed access to the [`AsciiFrame`] is exposed: consuming or moving its owned cell buffer
/// out of this wrapper would detach it from the runtime's output accounting.
#[derive(Debug)]
pub struct ConvertedFrame {
    frame: AsciiFrame,
    descriptor: FrameDescriptor,
    _lease: FrameLease,
}

impl ConvertedFrame {
    /// Borrows the converted ASCII frame while its runtime output permit remains held.
    #[must_use]
    pub const fn frame(&self) -> &AsciiFrame {
        &self.frame
    }

    /// Returns the exact pool, generation, slot, and publication identity of the consumed raster.
    #[must_use]
    pub const fn descriptor(&self) -> FrameDescriptor {
        self.descriptor
    }
}

#[cfg(test)]
#[path = "../../ascii-convert/tests/support/synthetic_font.rs"]
#[allow(dead_code)]
mod synthetic_font;

#[cfg(test)]
mod tests {
    use super::synthetic_font;
    use super::{ConvertedFrame, RasterConsumerError, RasterToAsciiConsumer};
    use crate::{
        BoundedHandoff, RasterDescriptorMetadata, ResourceBudget, ResourceLimits, RuntimeError,
        SlotState,
    };
    use ascii_convert::{
        GlyphConfig, GlyphRepertoire, GridSizing, LinearRgb, NormalizedRasterResources,
        build_glyph_table,
    };
    use ascii_core::{
        AlphaMode, Budget, CancellationToken, ColorDescription, GlyphTableId, PixelFormat,
        RasterOrientation,
    };

    fn core_budget() -> Budget {
        Budget::new(1 << 20, 1 << 20, 1 << 20)
    }

    fn resource_budget() -> ResourceBudget {
        ResourceBudget::new(ResourceLimits::new(1, 0, 1, 1 << 20, 1 << 20))
    }

    fn metadata(timestamp_ns: u64) -> RasterDescriptorMetadata {
        RasterDescriptorMetadata {
            width: 1,
            height: 1,
            row_stride: 3,
            format: PixelFormat::rgb8(),
            color: ColorDescription::srgb(AlphaMode::Opaque),
            orientation: RasterOrientation::TopDown,
            media_timestamp_ns: timestamp_ns,
        }
    }

    fn glyph_table() -> ascii_convert::GlyphTable {
        let font = synthetic_font::synthetic_font(false);
        let repertoire = GlyphRepertoire::new("BA ").unwrap();
        let config = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
        build_glyph_table(
            &font,
            &repertoire,
            &config,
            &core_budget(),
            &CancellationToken::new(),
        )
        .unwrap()
    }

    fn consumer(resources: &ResourceBudget, max_input_bytes: usize) -> RasterToAsciiConsumer {
        RasterToAsciiConsumer::new(
            resources,
            max_input_bytes,
            GridSizing::columns(1),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
            NormalizedRasterResources::new(1 << 20),
        )
        .unwrap()
    }

    fn submit_one(
        handoff: &BoundedHandoff,
        timestamp_ns: u64,
        pixels: &[u8],
    ) -> crate::FrameDescriptor {
        handoff
            .submit_frame(
                metadata(timestamp_ns),
                pixels,
                &core_budget(),
                &CancellationToken::new(),
            )
            .unwrap()
    }

    fn assert_output_is_bound(
        output: &ConvertedFrame,
        descriptor: crate::FrameDescriptor,
        table: &ascii_convert::GlyphTable,
    ) {
        let frame = output.frame();
        assert_eq!(output.descriptor(), descriptor);
        assert_eq!(
            frame.glyph_table_id(),
            GlyphTableId::new(*table.fingerprint())
        );
        assert_eq!(frame.frame_id(), descriptor.publication_id);
        assert_eq!(frame.generation(), descriptor.generation);
        assert_eq!(
            frame.pts_us(),
            i64::try_from(descriptor.media_timestamp_ns / 1_000).unwrap()
        );
        assert_eq!(frame.duration_us(), 0);
        assert!(!frame.cells().is_empty());
    }

    #[test]
    fn bounded_handoff_raster_is_copied_and_converted_with_identity_and_reused_scratch() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 9, 1, 3, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        let table = glyph_table();
        let scratch_address = consumer.scratch.as_ptr();
        let scratch_capacity = consumer.scratch.capacity();
        let memory_after_construction = resources.snapshot().memory;

        let first_descriptor = submit_one(&handoff, 123_456_789, &[255, 255, 255]);
        let first = consumer
            .convert_next(&handoff, &table, &core_budget(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_output_is_bound(&first, first_descriptor, &table);
        assert_eq!(
            handoff.slot_state(first_descriptor.slot_id).unwrap(),
            SlotState::Consumed
        );
        assert_eq!(resources.snapshot().outputs, 1);
        assert_eq!(resources.snapshot().memory, memory_after_construction);
        assert_eq!(consumer.scratch.as_ptr(), scratch_address);
        assert_eq!(consumer.scratch.capacity(), scratch_capacity);
        drop(first);
        assert_eq!(
            handoff.slot_state(first_descriptor.slot_id).unwrap(),
            SlotState::Free
        );
        assert_eq!(resources.snapshot().outputs, 0);

        let second_descriptor = submit_one(&handoff, 987_654_321, &[0, 0, 0]);
        let second = consumer
            .convert_next(&handoff, &table, &core_budget(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_output_is_bound(&second, second_descriptor, &table);
        assert_eq!(resources.snapshot().memory, memory_after_construction);
        assert_eq!(consumer.scratch.as_ptr(), scratch_address);
        assert_eq!(consumer.scratch.capacity(), scratch_capacity);
        drop(second);
        assert_eq!(resources.snapshot().outputs, 0);
    }

    #[test]
    fn converted_output_stays_charged_across_generation_invalidation() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 9, 1, 3, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        let table = glyph_table();
        let old_descriptor = submit_one(&handoff, 123_000, &[255, 255, 255]);
        let old_output = consumer
            .convert_next(&handoff, &table, &core_budget(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_output_is_bound(&old_output, old_descriptor, &table);
        assert_eq!(resources.snapshot().outputs, 1);

        handoff.invalidate_generation(10).unwrap();
        assert_eq!(resources.snapshot().outputs, 1);
        assert!(!old_output.frame().cells().is_empty());
        assert_eq!(old_output.frame().generation(), 9);

        let new_descriptor = submit_one(&handoff, 456_000, &[0, 0, 0]);
        assert!(matches!(
            consumer.convert_next(&handoff, &table, &core_budget(), &CancellationToken::new()),
            Err(RasterConsumerError::Runtime(
                RuntimeError::ConcurrencyLimit(crate::PermitKind::Output)
            ))
        ));
        assert_eq!(resources.snapshot().outputs, 1);
        drop(old_output);
        assert_eq!(resources.snapshot().outputs, 0);

        let new_output = consumer
            .convert_next(&handoff, &table, &core_budget(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_output_is_bound(&new_output, new_descriptor, &table);
        assert_eq!(resources.snapshot().outputs, 1);
        drop(new_output);
        assert_eq!(resources.snapshot().outputs, 0);
    }

    #[test]
    fn invalidation_after_conversion_before_wrapper_return_keeps_output_charged() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 9, 1, 3, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        let table = glyph_table();
        let descriptor = submit_one(&handoff, 789_000, &[127, 127, 127]);
        let mut lease = handoff
            .receive_frame(&core_budget(), &CancellationToken::new())
            .unwrap()
            .unwrap();
        lease.pin_output().unwrap();
        let token = lease.completion_token().unwrap();
        let frame = consumer
            .convert_lease(
                &lease,
                descriptor,
                &table,
                &core_budget(),
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(resources.snapshot().outputs, 1);

        handoff.invalidate_generation(10).unwrap();
        token.acknowledge();
        let output = ConvertedFrame {
            frame,
            descriptor,
            _lease: lease,
        };
        assert_eq!(resources.snapshot().outputs, 1);
        assert_output_is_bound(&output, descriptor, &table);
        drop(output);
        assert_eq!(resources.snapshot().outputs, 0);
    }

    #[test]
    fn oversized_descriptor_is_typed_and_reclaimed_without_growing_scratch() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 4, 1, 6, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        let descriptor = submit_one(&handoff, 1_000, &[1, 2, 3, 4, 5, 6]);
        let address = consumer.scratch.as_ptr();
        let capacity = consumer.scratch.capacity();

        assert!(matches!(
            consumer.convert_next(
                &handoff,
                &glyph_table(),
                &core_budget(),
                &CancellationToken::new()
            ),
            Err(RasterConsumerError::InputTooLarge {
                byte_length: 6,
                max_bytes: 3,
            })
        ));
        assert_eq!(
            handoff.slot_state(descriptor.slot_id).unwrap(),
            SlotState::Free
        );
        assert_eq!(resources.snapshot().outputs, 0);
        assert_eq!(consumer.scratch.as_ptr(), address);
        assert_eq!(consumer.scratch.capacity(), capacity);
    }

    #[test]
    fn conversion_error_acknowledges_and_releases_the_consumed_slot() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 2, 1, 3, 1, 1).unwrap();
        let descriptor = submit_one(&handoff, 2_000, &[10, 20, 30]);
        let mut consumer = RasterToAsciiConsumer::new(
            &resources,
            3,
            GridSizing::columns(1),
            LinearRgb::WHITE,
            LinearRgb::BLACK,
            NormalizedRasterResources::new(0),
        )
        .unwrap();

        assert!(matches!(
            consumer.convert_next(
                &handoff,
                &glyph_table(),
                &core_budget(),
                &CancellationToken::new()
            ),
            Err(RasterConsumerError::Conversion(_))
        ));
        assert_eq!(
            handoff.slot_state(descriptor.slot_id).unwrap(),
            SlotState::Free
        );
        assert_eq!(resources.snapshot().outputs, 0);
        assert_eq!(resources.snapshot().cpu_jobs, 0);
    }

    #[test]
    fn scratch_buffer_memory_is_charged_at_construction() {
        let resources = resource_budget();
        let before = resources.snapshot().memory.cpu_bytes;
        let consumer = consumer(&resources, 17);
        assert_eq!(consumer.max_input_bytes(), 17);
        assert_eq!(
            resources.snapshot().memory.cpu_bytes - before,
            17,
            "the reusable scratch extent is charged once at construction"
        );
        drop(consumer);
        assert_eq!(resources.snapshot().memory.cpu_bytes, before);
    }

    #[test]
    fn no_frame_is_returned_when_handoff_is_empty() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 1, 1, 3, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        assert!(
            consumer
                .convert_next(
                    &handoff,
                    &glyph_table(),
                    &core_budget(),
                    &CancellationToken::new()
                )
                .unwrap()
                .is_none()
        );
        assert_eq!(resources.snapshot().outputs, 0);
    }

    #[test]
    fn conversion_cpu_permit_is_released_after_processing() {
        let resources = resource_budget();
        let handoff = BoundedHandoff::new(&resources, 1, 1, 3, 1, 1).unwrap();
        let mut consumer = consumer(&resources, 3);
        let _ = submit_one(&handoff, 0, &[8, 8, 8]);
        let output = consumer
            .convert_next(
                &handoff,
                &glyph_table(),
                &core_budget(),
                &CancellationToken::new(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(resources.snapshot().cpu_jobs, 0);
        drop(output);
        assert_eq!(resources.snapshot().outputs, 0);
    }

    #[test]
    fn invalid_scratch_bound_is_rejected() {
        let resources = resource_budget();
        assert_eq!(
            RasterToAsciiConsumer::new(
                &resources,
                0,
                GridSizing::columns(1),
                LinearRgb::WHITE,
                LinearRgb::BLACK,
                NormalizedRasterResources::new(1),
            )
            .unwrap_err(),
            RasterConsumerError::Runtime(RuntimeError::InvalidCapacity)
        );
    }
}
