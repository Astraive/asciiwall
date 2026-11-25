use std::time::Duration;

use crate::DecodedFrame;

/// Minimum presentation time assigned to an APNG frame, including zero-delay frames.
pub const MIN_FRAME_DURATION: Duration = Duration::from_millis(10);
/// Maximum number of composed frames retained by one animation.
pub const MAX_ANIMATION_FRAMES: usize = 256;
/// Maximum duration of one animation loop.
pub const MAX_ANIMATION_DURATION: Duration = Duration::from_secs(24 * 60 * 60);

/// Number of plays encoded by the animation; `Infinite` repeats without a final play.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AnimationLoops {
    /// Repeat forever.
    Infinite,
    /// Play exactly this many times. Zero plays are rejected during decode.
    Finite(u32),
}

/// One fully composed canvas and its normalized presentation interval.
#[derive(Debug)]
pub struct AnimationFrame {
    image: DecodedFrame,
    presentation_time: Duration,
    duration: Duration,
}

impl AnimationFrame {
    /// Returns the full-canvas decoded raster.
    #[must_use]
    pub const fn image(&self) -> &DecodedFrame {
        &self.image
    }

    /// Returns this frame's start offset within one loop.
    #[must_use]
    pub const fn presentation_time(&self) -> Duration {
        self.presentation_time
    }

    /// Returns this frame's positive normalized duration.
    #[must_use]
    pub const fn duration(&self) -> Duration {
        self.duration
    }
}

/// Bounded full-canvas animation with normalized presentation timing and loop semantics.
#[derive(Debug)]
pub struct Animation {
    frames: Vec<AnimationFrame>,
    loops: AnimationLoops,
    loop_duration: Duration,
}

impl Animation {
    pub(crate) fn from_composed_frames(
        frames: Vec<(DecodedFrame, Duration)>,
        loops: AnimationLoops,
    ) -> Option<Self> {
        if frames.is_empty() || frames.len() > MAX_ANIMATION_FRAMES {
            return None;
        }
        if matches!(loops, AnimationLoops::Finite(0)) {
            return None;
        }

        let mut timeline = Vec::with_capacity(frames.len());
        let mut presentation_time = Duration::ZERO;
        for (image, raw_duration) in frames {
            let duration = raw_duration.max(MIN_FRAME_DURATION);
            let next_time = presentation_time.checked_add(duration)?;
            if next_time > MAX_ANIMATION_DURATION {
                return None;
            }
            timeline.push(AnimationFrame {
                image,
                presentation_time,
                duration,
            });
            presentation_time = next_time;
        }

        Some(Self {
            frames: timeline,
            loops,
            loop_duration: presentation_time,
        })
    }

    /// Returns the full-canvas composed frames in source order.
    #[must_use]
    pub fn frames(&self) -> &[AnimationFrame] {
        &self.frames
    }

    /// Returns the finite or infinite play count declared by the source.
    #[must_use]
    pub const fn loops(&self) -> AnimationLoops {
        self.loops
    }

    /// Returns the normalized duration of one complete play.
    #[must_use]
    pub const fn loop_duration(&self) -> Duration {
        self.loop_duration
    }

    /// Selects the same composed frame for preview and export at an elapsed time.
    ///
    /// Finite animations hold their final frame after the declared number of plays.
    #[must_use]
    pub fn frame_at(&self, elapsed: Duration) -> &AnimationFrame {
        let offset = match self.loops {
            AnimationLoops::Infinite => elapsed.as_nanos() % self.loop_duration.as_nanos(),
            AnimationLoops::Finite(plays) => {
                let end = self.loop_duration.as_nanos() * u128::from(plays);
                if elapsed.as_nanos() >= end {
                    return self.frames.last().expect("non-empty animation");
                }
                elapsed.as_nanos() % self.loop_duration.as_nanos()
            }
        };
        let index = self
            .frames
            .partition_point(|frame| frame.presentation_time.as_nanos() <= offset)
            .saturating_sub(1);
        &self.frames[index]
    }
}
