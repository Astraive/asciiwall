//! Borrowed host targets and actionable presentation results.

/// Why the host should retry only after an external condition changes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RecoverableReason {
    /// The surface must be reconfigured by its owner.
    SurfaceOutdated,
    /// The surface must be recreated or reconfigured by its owner.
    SurfaceLost,
    /// Acquisition timed out; pace a subsequent retry.
    SurfaceTimeout,
    /// The host reported loss of the device; construct a renderer for the replacement.
    DeviceLost,
}

/// An intentional no-work result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkippedReason {
    /// The host's surface is minimized or has no drawable pixels.
    ZeroSized,
    /// The same scene is still valid in the same target generation.
    Unchanged,
}

/// Submission result; this is not a compositor-completion or visibility notification.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentOutcome {
    /// Commands were submitted; a native surface frame was also handed to `present`.
    /// A suboptimal surface should be reconfigured by its host.
    Presented { suboptimal: bool },
    /// No upload, draw, submission, or acquisition was performed.
    Skipped(SkippedReason),
    /// Host intervention or a paced retry is needed; no new frame was presented.
    Recoverable(RecoverableReason),
}

pub(crate) enum TargetKind<'a> {
    Surface(&'a wgpu::Surface<'a>),
    Texture(&'a wgpu::Texture),
}

/// A borrowed, host-owned surface or texture and its content generation.
///
/// `identity` must distinguish every target used with one renderer, including replacement
/// objects. Increment `revision` whenever the existing pixels may no longer be valid, including
/// expose/damage, external writes, surface reconfiguration and resize. Both values must not be
/// reused while they might identify a different target/content. The host must configure surface
/// size/format for the renderer's device, and use the same device for a texture. The renderer
/// never configures, creates, or retains either target. The supplied surface status is a snapshot:
/// reconstruct the target with the latest host signal on every presentation.
pub struct RenderTarget<'a> {
    pub(crate) kind: TargetKind<'a>,
    pub(crate) identity: u64,
    pub(crate) revision: u64,
    pub(crate) width: u32,
    pub(crate) height: u32,
    pub(crate) format: wgpu::TextureFormat,
    pub(crate) surface_status: Option<RecoverableReason>,
}

impl<'a> RenderTarget<'a> {
    /// Borrows an already configured surface. Zero dimensions represent a minimized surface.
    /// Supply a host-reported surface issue even when the scene is unchanged; pass `None` only
    /// when the host has no issue to report. Acquisition errors are reported on redraws.
    #[must_use]
    pub fn surface(
        surface: &'a wgpu::Surface<'a>,
        identity: u64,
        revision: u64,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
        surface_status: Option<RecoverableReason>,
    ) -> Self {
        Self {
            kind: TargetKind::Surface(surface),
            identity,
            revision,
            width,
            height,
            format,
            surface_status,
        }
    }

    /// Borrows a host-created single-sample 2D render-attachment texture.
    #[must_use]
    pub fn texture(texture: &'a wgpu::Texture, identity: u64, revision: u64) -> Self {
        Self {
            kind: TargetKind::Texture(texture),
            identity,
            revision,
            width: texture.width(),
            height: texture.height(),
            format: texture.format(),
            surface_status: None,
        }
    }

    pub(crate) fn key(&self) -> TargetKey {
        TargetKey {
            identity: self.identity,
            revision: self.revision,
            width: self.width,
            height: self.height,
            format: self.format,
            surface: matches!(&self.kind, TargetKind::Surface(_)),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TargetKey {
    identity: u64,
    revision: u64,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    surface: bool,
}

// Host reports take precedence over a static-scene cache hit.
pub(crate) fn cached_outcome(
    unchanged: bool,
    surface_status: Option<RecoverableReason>,
    last: &mut Option<(TargetKey, ascii_core::GridSize)>,
) -> Option<PresentOutcome> {
    if let Some(reason) = surface_status {
        *last = None;
        Some(PresentOutcome::Recoverable(reason))
    } else if unchanged {
        Some(PresentOutcome::Skipped(SkippedReason::Unchanged))
    } else {
        None
    }
}

pub(crate) fn surface_error(error: wgpu::SurfaceError) -> Result<PresentOutcome, crate::Error> {
    let reason = match error {
        wgpu::SurfaceError::Timeout => RecoverableReason::SurfaceTimeout,
        wgpu::SurfaceError::Outdated => RecoverableReason::SurfaceOutdated,
        wgpu::SurfaceError::Lost => RecoverableReason::SurfaceLost,
        wgpu::SurfaceError::OutOfMemory => return Err(crate::Error::OutOfMemory),
    };
    Ok(PresentOutcome::Recoverable(reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reported_loss_wins_over_static_cache_and_recovery_requires_redraw() {
        let key = TargetKey {
            identity: 1,
            revision: 1,
            width: 80,
            height: 24,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            surface: true,
        };
        let record = (key, ascii_core::GridSize::new(4, 4));
        for reason in [
            RecoverableReason::SurfaceLost,
            RecoverableReason::SurfaceOutdated,
            RecoverableReason::SurfaceTimeout,
        ] {
            let mut last = Some(record);
            assert_eq!(
                cached_outcome(true, Some(reason), &mut last),
                Some(PresentOutcome::Recoverable(reason))
            );
            assert_eq!(
                last, None,
                "a reported problem invalidates the last presentation"
            );
            let unchanged = last == Some(record);
            assert_eq!(cached_outcome(unchanged, None, &mut last), None);
        }
        let mut last = Some(record);
        assert_eq!(
            cached_outcome(true, None, &mut last),
            Some(PresentOutcome::Skipped(SkippedReason::Unchanged))
        );
        assert_eq!(last, Some(record));
        assert_eq!(cached_outcome(false, None, &mut last), None);
    }

    #[test]
    fn acquisition_failures_remain_actionable() {
        assert_eq!(
            surface_error(wgpu::SurfaceError::Timeout),
            Ok(PresentOutcome::Recoverable(
                RecoverableReason::SurfaceTimeout
            ))
        );
        assert_eq!(
            surface_error(wgpu::SurfaceError::Outdated),
            Ok(PresentOutcome::Recoverable(
                RecoverableReason::SurfaceOutdated
            ))
        );
        assert_eq!(
            surface_error(wgpu::SurfaceError::Lost),
            Ok(PresentOutcome::Recoverable(RecoverableReason::SurfaceLost))
        );
        assert_eq!(
            surface_error(wgpu::SurfaceError::OutOfMemory),
            Err(crate::Error::OutOfMemory)
        );
    }
}
