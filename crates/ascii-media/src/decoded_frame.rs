use crate::resources::MemoryPermit;
use ascii_core::{
    AlphaMode, Budget, CancellationToken, ColorDescription, Error, PixelChannels, PixelFormat,
    RasterFrameView, RasterOrientation, validate_raster_frame,
};

/// An owned, normalized image and its already-validated raster view.
///
/// `view()` borrows from this value; the pixel backing cannot outlive the frame.
///
/// A view cannot escape the lifetime of its owner:
///
/// ```compile_fail
/// use ascii_core::RasterFrameView;
/// use ascii_media::DecodedFrame;
/// fn escape(frame: &DecodedFrame) -> RasterFrameView<'static> {
///     frame.view()
/// }
/// ```
#[derive(Debug)]
pub struct DecodedFrame {
    width: u32,
    height: u32,
    row_stride: usize,
    format: PixelFormat,
    color: ColorDescription,
    pixels: Vec<u8>,
    // Declared after pixels so the allocation is freed before its charge is released.
    _memory_permit: MemoryPermit,
}
pub(crate) struct DecodedFrameData {
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) row_stride: usize,
    pub(crate) channels: PixelChannels,
    pub(crate) alpha: AlphaMode,
    pub(crate) pixels: Vec<u8>,
    pub(crate) memory_permit: MemoryPermit,
}

impl DecodedFrame {
    pub(crate) fn new(
        data: DecodedFrameData,
        budget: &Budget,
        cancel: &CancellationToken,
    ) -> Result<Self, Error> {
        let DecodedFrameData {
            width,
            height,
            row_stride,
            channels,
            alpha,
            pixels,
            memory_permit,
        } = data;
        let capacity = u64::try_from(pixels.capacity()).map_err(|_| Error::SizeOverflow)?;
        if memory_permit.bytes() != capacity {
            return Err(Error::Malformed(
                ascii_core::MalformedReason::InvalidConfiguration,
            ));
        }
        let frame = Self {
            width,
            height,
            row_stride,
            format: PixelFormat::new(channels, 8),
            color: ColorDescription::srgb(alpha),
            pixels,
            _memory_permit: memory_permit,
        };
        validate_raster_frame(
            RasterFrameView::new(
                frame.width,
                frame.height,
                frame.row_stride,
                frame.format,
                frame.color,
                RasterOrientation::TopDown,
                &frame.pixels,
            ),
            budget,
            cancel,
        )?;
        Ok(frame)
    }

    #[must_use]
    pub const fn width(&self) -> u32 {
        self.width
    }

    #[must_use]
    pub const fn height(&self) -> u32 {
        self.height
    }

    #[must_use]
    pub const fn row_stride(&self) -> usize {
        self.row_stride
    }

    #[must_use]
    pub const fn format(&self) -> PixelFormat {
        self.format
    }

    #[must_use]
    pub const fn color(&self) -> ColorDescription {
        self.color
    }

    #[must_use]
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Returns a borrowed raster view of the already-validated frame.
    ///
    /// The pixel slice in the returned view cannot outlive this borrow.
    #[must_use]
    pub fn view(&self) -> RasterFrameView<'_> {
        RasterFrameView::new(
            self.width,
            self.height,
            self.row_stride,
            self.format,
            self.color,
            RasterOrientation::TopDown,
            &self.pixels,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::DecodeResources;

    #[test]
    fn padded_backing_is_charged_including_final_row_padding() {
        let resources = DecodeResources::new(1, 64);
        let pixels = vec![1u8; 16];
        let capacity = u64::try_from(pixels.capacity()).unwrap();
        let permit = resources.try_reserve_memory(capacity).unwrap();
        let frame = DecodedFrame::new(
            DecodedFrameData {
                width: 2,
                height: 2,
                row_stride: 8,
                channels: PixelChannels::Rgb,
                alpha: AlphaMode::Opaque,
                pixels,
                memory_permit: permit,
            },
            &Budget::new(64, 64, 64),
            &CancellationToken::new(),
        )
        .unwrap();

        assert_eq!(frame.view().row_stride(), 8);
        assert_eq!(frame.view().pixels().len(), 16);
        assert_eq!(resources.cpu_bytes_in_use(), capacity);
        drop(frame);
        assert_eq!(resources.cpu_bytes_in_use(), 0);
    }
}
