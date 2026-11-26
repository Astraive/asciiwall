//! Checked, generation-scoped coordinate transforms and portable monotonic-time policy.
//!
//! This module contains only deterministic math and state machines. Monitor discovery, native
//! routing, and clock sampling remain responsibilities of platform adapters.
//! Rectangles assign points with half-open ownership; geometric point transforms accept
//! closed outer edges exactly and never clamp points outside their content.

use crate::events::{
    DesktopPoint, DesktopRect, EventEnvelope, EventPayload, MonotonicEpoch, MonotonicInstant,
    Observation, UserCommand,
};
use crate::{Generation, MonitorId};

/// Error from construction or application of a coordinate/time transform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CoordinateError {
    /// A coordinate, rectangle, scale, or fit region is non-finite or has invalid dimensions.
    InvalidGeometry,
    /// A normalized scene rectangle must remain inside the unit square.
    SceneOutsideUnitSquare,
    /// A replacement topology generation must advance monotonically.
    NonMonotonicGeneration,
    /// A fit mode and its explicit visible scene region disagree.
    InvalidFitMode,
    /// Character dimensions must be non-zero.
    EmptyCharacterGrid,
    /// A point is outside its declared output or fitted content region.
    OutsideOutputOrContent,
    /// The requested local output identity is not present in this topology.
    UnknownOutput,
    /// The queried point is not covered by an output, including a spanned desktop gap.
    DisplayGap,
    /// The transform was captured under a different topology generation.
    StaleGeneration,
    /// A topology must contain at least one output and fit the compile-time capacity.
    InvalidOutputCount,
    /// Output identities must be unique within a topology.
    DuplicateOutput,
    /// Per-display and spanned outputs cannot overlap.
    OverlappingOutputs,
    /// Mirror outputs must share one identical logical desktop rectangle.
    IncompatibleMirror,
    /// Collapsed horizontal/vertical span requires aligned, non-overlapping output rectangles.
    InvalidCollapsedSpan,
    /// A computed transform overflows its finite numeric domain.
    ArithmeticOverflow,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Rect2 {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
}

impl Rect2 {
    fn new(left: f64, top: f64, width: f64, height: f64) -> Result<Self, CoordinateError> {
        let right = left + width;
        let bottom = top + height;
        if !left.is_finite()
            || !top.is_finite()
            || !width.is_finite()
            || !height.is_finite()
            || width <= 0.0
            || height <= 0.0
            || !right.is_finite()
            || !bottom.is_finite()
        {
            return Err(CoordinateError::InvalidGeometry);
        }
        Ok(Self {
            left,
            top,
            width,
            height,
        })
    }

    const fn right(self) -> f64 {
        self.left + self.width
    }

    const fn bottom(self) -> f64 {
        self.top + self.height
    }

    fn contains(self, point: Point2) -> bool {
        point.x >= self.left
            && point.y >= self.top
            && point.x < self.right()
            && point.y < self.bottom()
    }
    // Ownership uses `contains`; transforms use this to preserve exact outer-edge inverses.
    fn contains_closed(self, point: Point2) -> bool {
        point.x >= self.left
            && point.y >= self.top
            && point.x <= self.right()
            && point.y <= self.bottom()
    }

    fn contains_rect(self, other: Self) -> bool {
        other.left >= self.left
            && other.top >= self.top
            && other.right() <= self.right()
            && other.bottom() <= self.bottom()
    }

    fn intersects(self, other: Self) -> bool {
        self.left < other.right()
            && other.left < self.right()
            && self.top < other.bottom()
            && other.top < self.bottom()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Point2 {
    x: f64,
    y: f64,
}

impl Point2 {
    fn new(x: f64, y: f64) -> Result<Self, CoordinateError> {
        if x.is_finite() && y.is_finite() {
            Ok(Self { x, y })
        } else {
            Err(CoordinateError::InvalidGeometry)
        }
    }
}

/// Signed desktop logical position; the origin may be negative.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopLogicalPoint(Point2);

impl DesktopLogicalPoint {
    /// Creates a finite desktop logical position.
    pub fn new(x: f64, y: f64) -> Result<Self, CoordinateError> {
        Ok(Self(Point2::new(x, y)?))
    }

    /// Returns the signed desktop x-coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.0.x
    }

    /// Returns the signed desktop y-coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.0.y
    }
}
impl From<DesktopPoint> for DesktopLogicalPoint {
    fn from(point: DesktopPoint) -> Self {
        Self(Point2 {
            x: point.x(),
            y: point.y(),
        })
    }
}

/// Position relative to an output's unrotated logical top-left corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputLogicalPoint(Point2);

impl OutputLogicalPoint {
    /// Returns output-local x in logical units.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.0.x
    }

    /// Returns output-local y in logical units.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.0.y
    }
}

/// Position relative to the top-left of a rendered output surface, in surface pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfacePoint(Point2);

impl SurfacePoint {
    /// Returns the surface-pixel x-coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.0.x
    }

    /// Returns the surface-pixel y-coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.0.y
    }
}

/// Normalized scene position; outer edges are representable geometric boundaries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScenePoint(Point2);

impl ScenePoint {
    /// Creates a normalized scene point in the closed unit square; outside points are rejected.
    pub fn new(x: f64, y: f64) -> Result<Self, CoordinateError> {
        if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) {
            Ok(Self(Point2 { x, y }))
        } else {
            Err(CoordinateError::OutsideOutputOrContent)
        }
    }

    /// Returns the normalized x-coordinate.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.0.x
    }

    /// Returns the normalized y-coordinate.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.0.y
    }
}

/// Signed, positive-area desktop logical rectangle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DesktopLogicalRect(Rect2);

impl DesktopLogicalRect {
    /// Creates a finite non-empty desktop rectangle; negative origins are valid.
    pub fn new(left: f64, top: f64, width: f64, height: f64) -> Result<Self, CoordinateError> {
        Ok(Self(Rect2::new(left, top, width, height)?))
    }

    /// Returns the rectangle's left edge.
    #[must_use]
    pub const fn left(self) -> f64 {
        self.0.left
    }

    /// Returns the rectangle's top edge.
    #[must_use]
    pub const fn top(self) -> f64 {
        self.0.top
    }

    /// Returns its logical width.
    #[must_use]
    pub const fn width(self) -> f64 {
        self.0.width
    }

    /// Returns its logical height.
    #[must_use]
    pub const fn height(self) -> f64 {
        self.0.height
    }

    /// Returns whether a desktop point lies inside this half-open rectangle.
    #[must_use]
    pub fn contains(self, point: DesktopLogicalPoint) -> bool {
        self.0.contains(point.0)
    }
}

/// Positive-area surface rectangle; point transforms accept its closed outer edges exactly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SurfaceRect(Rect2);

impl SurfaceRect {
    /// Creates a finite non-empty rectangle in surface pixels.
    pub fn new(left: f64, top: f64, width: f64, height: f64) -> Result<Self, CoordinateError> {
        Ok(Self(Rect2::new(left, top, width, height)?))
    }
}

/// Positive-area normalized scene rectangle; point transforms accept its closed outer edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneRect(Rect2);

impl SceneRect {
    /// Creates a finite non-empty rectangle inside normalized scene space.
    pub fn new(left: f64, top: f64, width: f64, height: f64) -> Result<Self, CoordinateError> {
        let rect = Rect2::new(left, top, width, height)?;
        if rect.left < 0.0 || rect.top < 0.0 || rect.right() > 1.0 || rect.bottom() > 1.0 {
            return Err(CoordinateError::SceneOutsideUnitSquare);
        }
        Ok(Self(rect))
    }
}

/// Non-uniform output scale in surface pixels per output-logical unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputScale {
    x: f64,
    y: f64,
}

impl OutputScale {
    /// Creates positive finite independent horizontal and vertical scales.
    pub fn new(x: f64, y: f64) -> Result<Self, CoordinateError> {
        if !x.is_finite() || !y.is_finite() || x <= 0.0 || y <= 0.0 {
            return Err(CoordinateError::InvalidGeometry);
        }
        Ok(Self { x, y })
    }

    /// Returns horizontal surface pixels per logical unit.
    #[must_use]
    pub const fn x(self) -> f64 {
        self.x
    }

    /// Returns vertical surface pixels per logical unit.
    #[must_use]
    pub const fn y(self) -> f64 {
        self.y
    }
}

/// Output panel orientation relative to its logical coordinate basis.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OutputRotation {
    /// No rotation.
    None,
    /// 90 degrees clockwise.
    Clockwise90,
    /// 180 degrees clockwise.
    Clockwise180,
    /// 270 degrees clockwise.
    Clockwise270,
}

/// Scene fitting policy. `Crop` uses an explicit visible scene sub-rectangle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FitMode {
    /// Preserve the whole scene and letterbox the surface rectangle around it.
    Letterbox,
    /// Fill the surface rectangle and expose only the declared scene sub-rectangle.
    Crop,
    /// Stretch the whole scene to the explicit surface rectangle.
    Stretch,
}

/// Explicit mapping between a surface-pixel content rectangle and normalized scene coverage.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedContent {
    surface: SurfaceRect,
    scene: SceneRect,
    mode: FitMode,
}

impl FittedContent {
    /// Creates a checked fit mapping; crop coverage is explicit and letterbox/stretch cover all scene.
    pub fn new(
        surface: SurfaceRect,
        scene: SceneRect,
        mode: FitMode,
    ) -> Result<Self, CoordinateError> {
        let full_scene = scene.0.left == 0.0
            && scene.0.top == 0.0
            && scene.0.width == 1.0
            && scene.0.height == 1.0;
        if mode != FitMode::Crop && !full_scene {
            return Err(CoordinateError::InvalidFitMode);
        }
        Ok(Self {
            surface,
            scene,
            mode,
        })
    }

    /// Returns the selected fitting policy.
    #[must_use]
    pub const fn mode(self) -> FitMode {
        self.mode
    }
}

/// Integer character-grid dimensions, used only after a scene point is validated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterGrid {
    columns: u32,
    rows: u32,
}

impl CharacterGrid {
    /// Creates a non-empty grid.
    pub const fn new(columns: u32, rows: u32) -> Result<Self, CoordinateError> {
        if columns == 0 || rows == 0 {
            Err(CoordinateError::EmptyCharacterGrid)
        } else {
            Ok(Self { columns, rows })
        }
    }

    /// Returns the grid's number of columns.
    #[must_use]
    pub const fn columns(self) -> u32 {
        self.columns
    }

    /// Returns the grid's number of rows.
    #[must_use]
    pub const fn rows(self) -> u32 {
        self.rows
    }
}

/// Character cell and normalized sub-cell position.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CharacterPosition {
    /// Zero-based character column.
    pub column: u32,
    /// Zero-based character row.
    pub row: u32,
    /// Position within the character cell, in `[0, 1)`.
    pub subcell_x: f64,
    /// Position within the character cell, in `[0, 1)`.
    pub subcell_y: f64,
}

/// Immutable validated transform for one output and topology generation.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputTransform {
    generation: Generation,
    output: OutputGeometry,
}

impl OutputTransform {
    /// Returns the generation captured by this transform.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns the stable local monitor ID represented by this transform.
    #[must_use]
    pub const fn monitor_id(&self) -> MonitorId {
        self.output.monitor
    }

    /// Converts a point on the selected output's closed desktop bounds to local logical units.
    pub fn desktop_to_output_logical(
        &self,
        current_generation: Generation,
        desktop: DesktopLogicalPoint,
    ) -> Result<OutputLogicalPoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        if !self.output.desktop.0.contains_closed(desktop.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        Ok(OutputLogicalPoint(Point2::new(
            desktop.x() - self.output.desktop.0.left,
            desktop.y() - self.output.desktop.0.top,
        )?))
    }

    /// Converts output-local logical units to signed desktop coordinates.
    pub fn output_logical_to_desktop(
        &self,
        current_generation: Generation,
        local: OutputLogicalPoint,
    ) -> Result<DesktopLogicalPoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        if !self.output.local_bounds().contains_closed(local.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        Ok(DesktopLogicalPoint(Point2::new(
            self.output.desktop.0.left + local.x(),
            self.output.desktop.0.top + local.y(),
        )?))
    }

    /// Applies independent scale and rotation from output logical to surface pixels.
    pub fn output_logical_to_surface(
        &self,
        current_generation: Generation,
        local: OutputLogicalPoint,
    ) -> Result<SurfacePoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        if !self.output.local_bounds().contains_closed(local.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        let width = self.output.desktop.width();
        let height = self.output.desktop.height();
        let sx = self.output.scale.x;
        let sy = self.output.scale.y;
        let x = local.x();
        let y = local.y();
        let (surface_x, surface_y) = match self.output.rotation {
            OutputRotation::None => (x * sx, y * sy),
            OutputRotation::Clockwise90 => ((height - y) * sy, x * sx),
            OutputRotation::Clockwise180 => ((width - x) * sx, (height - y) * sy),
            OutputRotation::Clockwise270 => (y * sy, (width - x) * sx),
        };
        self.output.check_surface(Point2::new(surface_x, surface_y)?)?;
        Ok(SurfacePoint(Point2::new(surface_x, surface_y)?))
    }

    /// Applies the inverse surface-pixel rotation and scale.
    pub fn surface_to_output_logical(
        &self,
        current_generation: Generation,
        surface: SurfacePoint,
    ) -> Result<OutputLogicalPoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        self.output.check_surface(surface.0)?;
        let width = self.output.desktop.width();
        let height = self.output.desktop.height();
        let sx = self.output.scale.x;
        let sy = self.output.scale.y;
        let (x, y) = match self.output.rotation {
            OutputRotation::None => (surface.x() / sx, surface.y() / sy),
            OutputRotation::Clockwise90 => {
                (surface.y() / sx, height - surface.x() / sy)
            }
            OutputRotation::Clockwise180 => (
                width - surface.x() / sx,
                height - surface.y() / sy,
            ),
            OutputRotation::Clockwise270 => {
                (width - surface.y() / sx, surface.x() / sy)
            }
        };
        let point = Point2::new(x, y)?;
        if !self.output.local_bounds().contains_closed(point) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        Ok(OutputLogicalPoint(point))
    }

    /// Maps only points inside fitted content to normalized scene coordinates; never clamps.
    pub fn surface_to_scene(
        &self,
        current_generation: Generation,
        surface: SurfacePoint,
    ) -> Result<ScenePoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let content = self.output.fitted.surface.0;
        if !content.contains_closed(surface.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        let fitted_scene = self.output.fitted.scene.0;
        let u = (surface.x() - content.left) / content.width;
        let v = (surface.y() - content.top) / content.height;
        ScenePoint::new(
            fitted_scene.left + u * fitted_scene.width,
            fitted_scene.top + v * fitted_scene.height,
        )
    }

    /// Maps normalized scene coordinates to fitted surface content without edge attraction.
    pub fn scene_to_surface(
        &self,
        current_generation: Generation,
        scene: ScenePoint,
    ) -> Result<SurfacePoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let fitted_scene = self.output.fitted.scene.0;
        if !fitted_scene.contains_closed(scene.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        let content = self.output.fitted.surface.0;
        let u = (scene.x() - fitted_scene.left) / fitted_scene.width;
        let v = (scene.y() - fitted_scene.top) / fitted_scene.height;
        let point = Point2::new(content.left + u * content.width, content.top + v * content.height)?;
        self.output.check_surface(point)?;
        Ok(SurfacePoint(point))
    }

    /// Converts desktop logical coordinates through rotation, scale, fit, and scene normalization.
    pub fn desktop_to_scene(
        &self,
        current_generation: Generation,
        desktop: DesktopLogicalPoint,
    ) -> Result<ScenePoint, CoordinateError> {
        let local = self.desktop_to_output_logical(current_generation, desktop)?;
        let surface = self.output_logical_to_surface(current_generation, local)?;
        self.surface_to_scene(current_generation, surface)
    }

    /// Converts a scene position back to signed desktop logical coordinates.
    pub fn scene_to_desktop(
        &self,
        current_generation: Generation,
        scene: ScenePoint,
    ) -> Result<DesktopLogicalPoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let surface = self.scene_to_surface(current_generation, scene)?;
        let local = self.surface_to_output_logical(current_generation, surface)?;
        self.output_logical_to_desktop(current_generation, local)
    }

    /// Maps a normalized point to a cell and sub-cell location.
    pub fn scene_to_character(
        &self,
        current_generation: Generation,
        scene: ScenePoint,
    ) -> Result<CharacterPosition, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let scaled_x = scene.x() * f64::from(self.output.grid.columns);
        let scaled_y = scene.y() * f64::from(self.output.grid.rows);
        let column = scaled_x as u32;
        let row = scaled_y as u32;
        if column >= self.output.grid.columns || row >= self.output.grid.rows {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        Ok(CharacterPosition {
            column,
            row,
            subcell_x: scaled_x - f64::from(column),
            subcell_y: scaled_y - f64::from(row),
        })
    }

    /// Maps a cell/sub-cell position to normalized scene coordinates.
    pub fn character_to_scene(
        &self,
        current_generation: Generation,
        character: CharacterPosition,
    ) -> Result<ScenePoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        if character.column >= self.output.grid.columns
            || character.row >= self.output.grid.rows
            || !character.subcell_x.is_finite()
            || !character.subcell_y.is_finite()
            || !(0.0..1.0).contains(&character.subcell_x)
            || !(0.0..1.0).contains(&character.subcell_y)
        {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        ScenePoint::new(
            (f64::from(character.column) + character.subcell_x)
                / f64::from(self.output.grid.columns),
            (f64::from(character.row) + character.subcell_y)
                / f64::from(self.output.grid.rows),
        )
    }

    fn ensure_generation(&self, current_generation: Generation) -> Result<(), CoordinateError> {
        if self.generation.is_current(current_generation) {
            Ok(())
        } else {
            Err(CoordinateError::StaleGeneration)
        }
    }
}

/// One output's validated geometry and fit state, independent of monitor discovery.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputGeometry {
    monitor: MonitorId,
    desktop: DesktopLogicalRect,
    scale: OutputScale,
    rotation: OutputRotation,
    surface_width: f64,
    surface_height: f64,
    fitted: FittedContent,
    grid: CharacterGrid,
}

impl OutputGeometry {
    /// Creates one output transform from synthetic or adapter-supplied geometry.
    pub fn new(
        monitor: MonitorId,
        desktop: DesktopLogicalRect,
        scale: OutputScale,
        rotation: OutputRotation,
        fitted: FittedContent,
        grid: CharacterGrid,
    ) -> Result<Self, CoordinateError> {
        let scaled_width = desktop.width() * scale.x;
        let scaled_height = desktop.height() * scale.y;
        let (surface_width, surface_height) = match rotation {
            OutputRotation::None | OutputRotation::Clockwise180 => {
                (scaled_width, scaled_height)
            }
            OutputRotation::Clockwise90 | OutputRotation::Clockwise270 => {
                (scaled_height, scaled_width)
            }
        };
        if !surface_width.is_finite() || !surface_height.is_finite() {
            return Err(CoordinateError::ArithmeticOverflow);
        }
        let surface_bounds = Rect2::new(0.0, 0.0, surface_width, surface_height)?;
        if !surface_bounds.contains_rect(fitted.surface.0) {
            return Err(CoordinateError::InvalidGeometry);
        }
        Ok(Self {
            monitor,
            desktop,
            scale,
            rotation,
            surface_width,
            surface_height,
            fitted,
            grid,
        })
    }

    /// Returns the stable local monitor ID.
    #[must_use]
    pub const fn monitor_id(&self) -> MonitorId {
        self.monitor
    }

    /// Returns the desktop logical bounds.
    #[must_use]
    pub const fn desktop_bounds(&self) -> DesktopLogicalRect {
        self.desktop
    }

    /// Returns the rotated surface dimensions in pixels.
    #[must_use]
    pub const fn surface_dimensions(&self) -> (f64, f64) {
        (self.surface_width, self.surface_height)
    }

    fn local_bounds(&self) -> Rect2 {
        Rect2 {
            left: 0.0,
            top: 0.0,
            width: self.desktop.width(),
            height: self.desktop.height(),
        }
    }

    fn check_surface(&self, point: Point2) -> Result<(), CoordinateError> {
        let bounds = Rect2 {
            left: 0.0,
            top: 0.0,
            width: self.surface_width,
            height: self.surface_height,
        };
        if bounds.contains_closed(point) {
            Ok(())
        } else {
            Err(CoordinateError::OutsideOutputOrContent)
        }
    }
}

/// Policy for mapping a multi-output virtual desktop into scene space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpanGapPolicy {
    /// Keep physical gaps in the virtual desktop's normalized span; points in gaps remain unrouted.
    Preserve,
    /// Collapse horizontal gaps for outputs aligned along one common horizontal row.
    CollapseHorizontal,
    /// Collapse vertical gaps for outputs aligned along one common vertical column.
    CollapseVertical,
}

/// Caller-owned identity used to share one scene clock across mirrored outputs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct SceneClockGroupId(u64);

impl SceneClockGroupId {
    /// Creates a clock-group identity.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the caller-assigned group value.
    #[must_use]
    pub const fn value(self) -> u64 {
        self.0
    }
}

/// Display routing mode; mirrored outputs share a scene-clock group but retain local transforms.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayRoutingMode {
    /// Route to the single output whose desktop bounds contain the position.
    PerDisplay,
    /// Route to the intersected output and map through a virtual span.
    Span(SpanGapPolicy),
    /// Route to every mirrored output while assigning one shared simulation clock group.
    Mirror {
        /// Shared scene-clock group; per-output coordinate transforms remain independent.
        clock_group: SceneClockGroupId,
    },
}

/// A bounded set of output targets selected for one desktop point.
#[derive(Clone, Debug, PartialEq)]
pub struct OutputRoute<const CAPACITY: usize> {
    outputs: [Option<MonitorId>; CAPACITY],
    len: usize,
    span_scene: Option<ScenePoint>,
}

impl<const CAPACITY: usize> OutputRoute<CAPACITY> {
    /// Returns the number of routed outputs.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns whether no output intersects the desktop point.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns one shared span-scene point, when routing in span mode.
    #[must_use]
    pub const fn span_scene_point(&self) -> Option<ScenePoint> {
        self.span_scene
    }

    /// Iterates the routed output identities without allocating.
    pub fn outputs(&self) -> impl Iterator<Item = MonitorId> + '_ {
        self.outputs[..self.len].iter().filter_map(|output| *output)
    }
}

/// Complete immutable topology value; install a replacement as one value to invalidate old transforms.
#[derive(Clone, Debug, PartialEq)]
pub struct TopologySnapshot<const CAPACITY: usize> {
    generation: Generation,
    mode: DisplayRoutingMode,
    outputs: [Option<OutputGeometry>; CAPACITY],
    len: usize,
    desktop_bounds: Rect2,
}

impl<const CAPACITY: usize> TopologySnapshot<CAPACITY> {
    /// Builds a complete synthetic/current topology before any caller replaces its active snapshot.
    pub fn new(
        generation: Generation,
        mode: DisplayRoutingMode,
        outputs: &[OutputGeometry],
    ) -> Result<Self, CoordinateError> {
        if outputs.is_empty() || outputs.len() > CAPACITY || CAPACITY == 0 {
            return Err(CoordinateError::InvalidOutputCount);
        }
        let mut stored = [None; CAPACITY];
        let mut bounds = outputs[0].desktop.0;
        let mut index = 0;
        while index < outputs.len() {
            let output = outputs[index];
            for earlier in &outputs[..index] {
                if earlier.monitor == output.monitor {
                    return Err(CoordinateError::DuplicateOutput);
                }
                match mode {
                    DisplayRoutingMode::Mirror { .. } => {
                        if earlier.desktop != output.desktop {
                            return Err(CoordinateError::IncompatibleMirror);
                        }
                    }
                    DisplayRoutingMode::PerDisplay | DisplayRoutingMode::Span(_) => {
                        if earlier.desktop.0.intersects(output.desktop.0) {
                            return Err(CoordinateError::OverlappingOutputs);
                        }
                    }
                }
            }
            let right = bounds.right().max(output.desktop.0.right());
            let bottom = bounds.bottom().max(output.desktop.0.bottom());
            bounds.left = bounds.left.min(output.desktop.0.left);
            bounds.top = bounds.top.min(output.desktop.0.top);
            bounds.width = right - bounds.left;
            bounds.height = bottom - bounds.top;
            stored[index] = Some(output);
            index += 1;
        }
        bounds = Rect2::new(bounds.left, bounds.top, bounds.width, bounds.height)?;

        match mode {
            DisplayRoutingMode::PerDisplay => {}
            DisplayRoutingMode::Span(SpanGapPolicy::Preserve) => {}
            DisplayRoutingMode::Span(SpanGapPolicy::CollapseHorizontal) => {
                if !aligned_horizontally(outputs) {
                    return Err(CoordinateError::InvalidCollapsedSpan);
                }
            }
            DisplayRoutingMode::Span(SpanGapPolicy::CollapseVertical) => {
                if !aligned_vertically(outputs) {
                    return Err(CoordinateError::InvalidCollapsedSpan);
                }
            }
            DisplayRoutingMode::Mirror { .. } if outputs.len() < 2 => {
                return Err(CoordinateError::IncompatibleMirror);
            }
            DisplayRoutingMode::Mirror { .. } => {}
        }
        Ok(Self {
            generation,
            mode,
            outputs: stored,
            len: outputs.len(),
            desktop_bounds: bounds,
        })
    }

    /// Returns the generation of this complete topology value.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }

    /// Returns the explicit routing policy.
    #[must_use]
    pub const fn mode(&self) -> DisplayRoutingMode {
        self.mode
    }

    /// Returns the scene-clock group for a routed output in mirror mode.
    pub fn scene_clock_group(
        &self,
        monitor: MonitorId,
        current_generation: Generation,
    ) -> Result<Option<SceneClockGroupId>, CoordinateError> {
        self.ensure_generation(current_generation)?;
        self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        match self.mode {
            DisplayRoutingMode::Mirror { clock_group } => Ok(Some(clock_group)),
            DisplayRoutingMode::PerDisplay | DisplayRoutingMode::Span(_) => Ok(None),
        }
    }

    /// Returns the validated transform for one output, rejecting stale-generation callers.
    pub fn output_transform(
        &self,
        monitor: MonitorId,
        current_generation: Generation,
    ) -> Result<OutputTransform, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let output = self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        Ok(OutputTransform {
            generation: self.generation,
            output: *output,
        })
    }

    /// Routes by half-open output rectangles; points not owned by an output remain explicit gaps.
    pub fn route_desktop_point(
        &self,
        desktop: DesktopLogicalPoint,
        current_generation: Generation,
    ) -> Result<OutputRoute<CAPACITY>, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let mut route = OutputRoute {
            outputs: [None; CAPACITY],
            len: 0,
            span_scene: None,
        };
        let mut index = 0;
        while index < self.len {
            let output = self.outputs[index]
                .as_ref()
                .ok_or(CoordinateError::InvalidOutputCount)?;
            if output.desktop.contains(desktop) {
                route.outputs[route.len] = Some(output.monitor);
                route.len += 1;
            }
            index += 1;
        }
        if route.len == 0 {
            return Err(CoordinateError::DisplayGap);
        }
        if let DisplayRoutingMode::Span(policy) = self.mode {
            route.span_scene = Some(self.span_scene_point(desktop, route.outputs[0].ok_or(
                CoordinateError::InvalidOutputCount,
            )?, policy)?);
        }
        Ok(route)
    }

    /// Converts a point on the selected output's closed boundary under the display policy.
    pub fn desktop_to_scene(
        &self,
        monitor: MonitorId,
        desktop: DesktopLogicalPoint,
        current_generation: Generation,
    ) -> Result<ScenePoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let output = self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        if !output.desktop.0.contains_closed(desktop.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        match self.mode {
            DisplayRoutingMode::PerDisplay | DisplayRoutingMode::Mirror { .. } => OutputTransform {
                generation: self.generation,
                output: *output,
            }
            .desktop_to_scene(current_generation, desktop),
            DisplayRoutingMode::Span(policy) => self.span_scene_point(desktop, monitor, policy),
        }
    }

    /// Converts a scene point back to the selected output, rejecting scene gaps and stale generations.
    pub fn scene_to_desktop(
        &self,
        monitor: MonitorId,
        scene: ScenePoint,
        current_generation: Generation,
    ) -> Result<DesktopLogicalPoint, CoordinateError> {
        self.ensure_generation(current_generation)?;
        let output = self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        match self.mode {
            DisplayRoutingMode::PerDisplay | DisplayRoutingMode::Mirror { .. } => OutputTransform {
                generation: self.generation,
                output: *output,
            }
            .scene_to_desktop(current_generation, scene),
            DisplayRoutingMode::Span(policy) => self.span_desktop_point(scene, monitor, policy),
        }
    }

    fn ensure_generation(&self, current_generation: Generation) -> Result<(), CoordinateError> {
        if self.generation.is_current(current_generation) {
            Ok(())
        } else {
            Err(CoordinateError::StaleGeneration)
        }
    }

    fn find_output(&self, monitor: MonitorId) -> Option<&OutputGeometry> {
        self.outputs[..self.len]
            .iter()
            .filter_map(|output| output.as_ref())
            .find(|output| output.monitor == monitor)
    }

    fn span_scene_point(
        &self,
        desktop: DesktopLogicalPoint,
        monitor: MonitorId,
        policy: SpanGapPolicy,
    ) -> Result<ScenePoint, CoordinateError> {
        let output = self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        if !output.desktop.0.contains_closed(desktop.0) {
            return Err(CoordinateError::OutsideOutputOrContent);
        }
        match policy {
            SpanGapPolicy::Preserve => ScenePoint::new(
                (desktop.x() - self.desktop_bounds.left) / self.desktop_bounds.width,
                (desktop.y() - self.desktop_bounds.top) / self.desktop_bounds.height,
            ),
            SpanGapPolicy::CollapseHorizontal => {
                let prefix = self.horizontal_prefix(output)?;
                let total = self.horizontal_total()?;
                let first = self.outputs[0]
                    .as_ref()
                    .ok_or(CoordinateError::InvalidOutputCount)?;
                ScenePoint::new(
                    (prefix + desktop.x() - output.desktop.left()) / total,
                    (desktop.y() - first.desktop.top()) / first.desktop.height(),
                )
            }
            SpanGapPolicy::CollapseVertical => {
                let prefix = self.vertical_prefix(output)?;
                let total = self.vertical_total()?;
                let first = self.outputs[0]
                    .as_ref()
                    .ok_or(CoordinateError::InvalidOutputCount)?;
                ScenePoint::new(
                    (desktop.x() - first.desktop.left()) / first.desktop.width(),
                    (prefix + desktop.y() - output.desktop.top()) / total,
                )
            }
        }
    }

    fn span_desktop_point(
        &self,
        scene: ScenePoint,
        monitor: MonitorId,
        policy: SpanGapPolicy,
    ) -> Result<DesktopLogicalPoint, CoordinateError> {
        let output = self.find_output(monitor).ok_or(CoordinateError::UnknownOutput)?;
        let point = match policy {
            SpanGapPolicy::Preserve => Point2::new(
                self.desktop_bounds.left + scene.x() * self.desktop_bounds.width,
                self.desktop_bounds.top + scene.y() * self.desktop_bounds.height,
            )?,
            SpanGapPolicy::CollapseHorizontal => {
                let prefix = self.horizontal_prefix(output)?;
                let total = self.horizontal_total()?;
                let compressed_x = scene.x() * total;
                if compressed_x < prefix || compressed_x > prefix + output.desktop.width() {
                    return Err(CoordinateError::DisplayGap);
                }
                let first = self.outputs[0]
                    .as_ref()
                    .ok_or(CoordinateError::InvalidOutputCount)?;
                Point2::new(
                    output.desktop.left() + compressed_x - prefix,
                    first.desktop.top() + scene.y() * first.desktop.height(),
                )?
            }
            SpanGapPolicy::CollapseVertical => {
                let prefix = self.vertical_prefix(output)?;
                let total = self.vertical_total()?;
                let compressed_y = scene.y() * total;
                if compressed_y < prefix || compressed_y > prefix + output.desktop.height() {
                    return Err(CoordinateError::DisplayGap);
                }
                let first = self.outputs[0]
                    .as_ref()
                    .ok_or(CoordinateError::InvalidOutputCount)?;
                Point2::new(
                    first.desktop.left() + scene.x() * first.desktop.width(),
                    output.desktop.top() + compressed_y - prefix,
                )?
            }
        };
        if !output.desktop.0.contains_closed(point) {
            return Err(CoordinateError::DisplayGap);
        }
        Ok(DesktopLogicalPoint(point))
    }

    fn horizontal_prefix(
        &self,
        output: &OutputGeometry,
    ) -> Result<f64, CoordinateError> {
        let mut prefix = 0.0;
        for current in self.outputs[..self.len].iter().filter_map(|entry| entry.as_ref()) {
            if current.desktop.left() < output.desktop.left() {
                prefix += current.desktop.width();
            }
        }
        if prefix.is_finite() {
            Ok(prefix)
        } else {
            Err(CoordinateError::ArithmeticOverflow)
        }
    }

    fn horizontal_total(&self) -> Result<f64, CoordinateError> {
        let mut total = 0.0;
        for output in self.outputs[..self.len].iter().filter_map(|entry| entry.as_ref()) {
            total += output.desktop.width();
        }
        if total.is_finite() && total > 0.0 {
            Ok(total)
        } else {
            Err(CoordinateError::ArithmeticOverflow)
        }
    }

    fn vertical_prefix(
        &self,
        output: &OutputGeometry,
    ) -> Result<f64, CoordinateError> {
        let mut prefix = 0.0;
        for current in self.outputs[..self.len].iter().filter_map(|entry| entry.as_ref()) {
            if current.desktop.top() < output.desktop.top() {
                prefix += current.desktop.height();
            }
        }
        if prefix.is_finite() {
            Ok(prefix)
        } else {
            Err(CoordinateError::ArithmeticOverflow)
        }
    }

    fn vertical_total(&self) -> Result<f64, CoordinateError> {
        let mut total = 0.0;
        for output in self.outputs[..self.len].iter().filter_map(|entry| entry.as_ref()) {
            total += output.desktop.height();
        }
        if total.is_finite() && total > 0.0 {
            Ok(total)
        } else {
            Err(CoordinateError::ArithmeticOverflow)
        }
    }
}
/// Owner for atomic whole-snapshot replacement after a complete topology has been validated.
#[derive(Clone, Debug, PartialEq)]
pub struct TopologyState<const CAPACITY: usize> {
    current: TopologySnapshot<CAPACITY>,
}

impl<const CAPACITY: usize> TopologyState<CAPACITY> {
    /// Starts from one complete topology snapshot.
    #[must_use]
    pub const fn new(current: TopologySnapshot<CAPACITY>) -> Self {
        Self { current }
    }

    /// Returns the active generation.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.current.generation
    }

    /// Returns the active immutable topology snapshot.
    #[must_use]
    pub const fn snapshot(&self) -> &TopologySnapshot<CAPACITY> {
        &self.current
    }

    /// Replaces the entire topology only after validating a strictly newer generation.
    pub fn replace(
        &mut self,
        replacement: TopologySnapshot<CAPACITY>,
    ) -> Result<Generation, CoordinateError> {
        if replacement.generation.value() <= self.current.generation.value() {
            return Err(CoordinateError::NonMonotonicGeneration);
        }
        self.current = replacement;
        Ok(self.current.generation)
    }
}

fn aligned_horizontally(outputs: &[OutputGeometry]) -> bool {
    let first = outputs[0].desktop;
    outputs.iter().all(|output| {
        output.desktop.top() == first.top() && output.desktop.height() == first.height()
    })
}

fn aligned_vertically(outputs: &[OutputGeometry]) -> bool {
    let first = outputs[0].desktop;
    outputs.iter().all(|output| {
        output.desktop.left() == first.left() && output.desktop.width() == first.width()
    })
}

/// Opaque identity of a non-receiver clock domain.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ClockDomainId(u64);

impl ClockDomainId {
    /// Creates a clock-domain identity supplied by the adapter.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

/// Rational mapping from one source clock epoch into a receiver monotonic epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockDomainMapping {
    domain: ClockDomainId,
    source_epoch: u64,
    receiver_epoch: MonotonicEpoch,
    source_origin: u64,
    receiver_origin: u64,
    rate_numerator: u64,
    rate_denominator: u64,
}

/// Failure while translating a timestamp into the receiving monotonic clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockMappingError {
    /// Source-to-receiver tick-rate ratio must be positive.
    InvalidRate,
    /// Timestamp came from another clock domain or source epoch.
    WrongSourceEpoch,
    /// Rational scaling or origin adjustment overflowed.
    ArithmeticOverflow,
}

impl ClockDomainMapping {
    /// Builds a checked rational clock mapping from one sampled source/receiver origin pair.
    pub const fn new(
        domain: ClockDomainId,
        source_epoch: u64,
        receiver_epoch: MonotonicEpoch,
        source_origin: u64,
        receiver_origin: u64,
        rate_numerator: u64,
        rate_denominator: u64,
    ) -> Result<Self, ClockMappingError> {
        if rate_numerator == 0 || rate_denominator == 0 {
            return Err(ClockMappingError::InvalidRate);
        }
        Ok(Self {
            domain,
            source_epoch,
            receiver_epoch,
            source_origin,
            receiver_origin,
            rate_numerator,
            rate_denominator,
        })
    }

    /// Translates source ticks to the receiver's monotonic epoch with checked rational arithmetic.
    pub fn map(
        self,
        domain: ClockDomainId,
        source_epoch: u64,
        source_ticks: u64,
    ) -> Result<MonotonicInstant, ClockMappingError> {
        if domain != self.domain || source_epoch != self.source_epoch {
            return Err(ClockMappingError::WrongSourceEpoch);
        }
        let (delta, positive) = if source_ticks >= self.source_origin {
            (source_ticks - self.source_origin, true)
        } else {
            (self.source_origin - source_ticks, false)
        };
        let scaled = u128::from(delta)
            .checked_mul(u128::from(self.rate_numerator))
            .ok_or(ClockMappingError::ArithmeticOverflow)?
            / u128::from(self.rate_denominator);
        let scaled = u64::try_from(scaled).map_err(|_| ClockMappingError::ArithmeticOverflow)?;
        let receiver_ticks = if positive {
            self.receiver_origin
                .checked_add(scaled)
                .ok_or(ClockMappingError::ArithmeticOverflow)?
        } else {
            self.receiver_origin
                .checked_sub(scaled)
                .ok_or(ClockMappingError::ArithmeticOverflow)?
        };
        Ok(MonotonicInstant::new(self.receiver_epoch, receiver_ticks))
    }
}

/// Explicit receiving-clock epoch tracker; discontinuities never wrap into an old epoch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ClockEpochTracker {
    current: MonotonicEpoch,
}

impl ClockEpochTracker {
    /// Starts with the receiving process's current monotonic epoch.
    #[must_use]
    pub const fn new(current: MonotonicEpoch) -> Self {
        Self { current }
    }

    /// Returns the current epoch.
    #[must_use]
    pub const fn current(self) -> MonotonicEpoch {
        self.current
    }

    /// Starts a new epoch after a clock discontinuity or source restart.
    pub fn advance(&mut self) -> Result<MonotonicEpoch, ClockMappingError> {
        self.current = self.current.checked_next().ok_or(ClockMappingError::ArithmeticOverflow)?;
        Ok(self.current)
    }
}

/// Fixed-size event time bucket used by deterministic fixed-step consumers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FixedStepScheduler {
    step_ns: u64,
    origin: Option<MonotonicInstant>,
    last_step: Option<u64>,
    last_timestamp: Option<MonotonicInstant>,
}

/// Invalid or out-of-order fixed-step schedule request.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FixedStepError {
    /// Step duration must be positive.
    ZeroStep,
    /// Timestamp belongs to another epoch or predates the scheduler origin.
    WrongEpochOrBeforeOrigin,
    /// An event would move processing behind the last assigned simulation step.
    OutOfOrder,
}

impl FixedStepScheduler {
    /// Creates a scheduler with a positive fixed step in nanoseconds.
    pub const fn new(step_ns: u64) -> Result<Self, FixedStepError> {
        if step_ns == 0 {
            Err(FixedStepError::ZeroStep)
        } else {
            Ok(Self {
                step_ns,
                origin: None,
                last_step: None,
                last_timestamp: None,
            })
        }
    }

    /// Assigns the next event to a fixed step while preserving nondecreasing event-time order.
    pub fn step_for(&mut self, timestamp: MonotonicInstant) -> Result<u64, FixedStepError> {
        let origin = match self.origin {
            Some(origin) => origin,
            None => {
                self.origin = Some(timestamp);
                self.last_step = Some(0);
                return Ok(0);
            }
        };
        let elapsed = timestamp
            .duration_since(origin)
            .ok_or(FixedStepError::WrongEpochOrBeforeOrigin)?;
        if self
            .last_timestamp
            .is_some_and(|last| timestamp.duration_since(last).is_none())
        {
            return Err(FixedStepError::OutOfOrder);
        }
        let step = elapsed / self.step_ns;
        if self.last_step.is_some_and(|last| step < last) {
            return Err(FixedStepError::OutOfOrder);
        }
        self.last_step = Some(step);
        self.last_timestamp = Some(timestamp);
        Ok(step)
    }

    /// Resets step numbering at a new receiver monotonic epoch.
    pub fn reset(&mut self) {
        self.origin = None;
        self.last_step = None;
        self.last_timestamp = None;
    }
}

/// Rejection reason for an event rejected by the portable freshness gate.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventRejection {
    /// Event belongs to an invalidated assignment/source generation.
    StaleGeneration,
    /// Event belongs to a previous or future receiver-clock epoch/time.
    StaleClock,
    /// A pointer press occurred while paused or before the latest resume boundary.
    StaleClick,
    /// A timestamp arrived before the last accepted event in the same monotonic epoch.
    OutOfOrder,
    /// Stop has already been accepted for this session.
    Stopped,
}

/// Session freshness gate rejects old clicks across pause/resume and stale generations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventFreshnessGate {
    generation: Generation,
    epoch: MonotonicEpoch,
    max_click_age_ns: u64,
    paused: bool,
    stopped: bool,
    resumed_at: Option<MonotonicInstant>,
    last_event: Option<MonotonicInstant>,
}

impl EventFreshnessGate {
    /// Creates a session gate for one topology/assignment generation and clock epoch.
    #[must_use]
    pub const fn new(
        generation: Generation,
        epoch: MonotonicEpoch,
        max_click_age_ns: u64,
    ) -> Self {
        Self {
            generation,
            epoch,
            max_click_age_ns,
            paused: false,
            stopped: false,
            resumed_at: None,
            last_event: None,
        }
    }

    /// Replaces the accepted generation only after a strictly newer snapshot is committed.
    pub fn replace_generation(
        &mut self,
        generation: Generation,
    ) -> Result<(), EventRejection> {
        if generation.value() <= self.generation.value() {
            return Err(EventRejection::StaleGeneration);
        }
        self.generation = generation;
        self.resumed_at = None;
        Ok(())
    }

    /// Applies topology replacement and pause/resume/stop boundaries in queue order.
    pub fn accept(
        &mut self,
        event: EventEnvelope,
        now: MonotonicInstant,
    ) -> Result<(), EventRejection> {
        if self.stopped {
            return Err(EventRejection::Stopped);
        }
        let topology_generation = match event.payload {
            EventPayload::TopologyChanged(change) => {
                if !event.generation.is_current(self.generation)
                    || !change.previous().is_current(self.generation)
                {
                    return Err(EventRejection::StaleGeneration);
                }
                Some(change.current())
            }
            _ if !event.generation.is_current(self.generation) => {
                return Err(EventRejection::StaleGeneration);
            }
            _ => None,
        };
        if event.timestamp.epoch() != self.epoch
            || now.epoch() != self.epoch
            || now.duration_since(event.timestamp).is_none()
        {
            return Err(EventRejection::StaleClock);
        }
        if event.payload.is_button_press() {
            let age = now
                .duration_since(event.timestamp)
                .ok_or(EventRejection::StaleClock)?;
            if self.paused
                || age > self.max_click_age_ns
                || self
                    .resumed_at
                    .is_some_and(|resumed_at| event.timestamp < resumed_at)
            {
                return Err(EventRejection::StaleClick);
            }
        }
        if self
            .last_event
            .is_some_and(|last| event.timestamp.duration_since(last).is_none())
        {
            return Err(EventRejection::OutOfOrder);
        }
        if let Some(generation) = topology_generation {
            self.generation = generation;
            self.resumed_at = None;
        }
        if let EventPayload::UserCommand(command) = event.payload {
            match command {
                UserCommand::Start | UserCommand::Resume => {
                    self.paused = false;
                    self.resumed_at = Some(event.timestamp);
                }
                UserCommand::Pause => self.paused = true,
                UserCommand::Stop => self.stopped = true,
            }
        }
        self.last_event = Some(event.timestamp);
        Ok(())
    }
}

/// Linear fading weight for the last known window force.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WindowForceFreshness {
    freshness_ns: u64,
    fade_ns: u64,
    last_good: Option<MonotonicInstant>,
}

impl WindowForceFreshness {
    /// Creates a freshness budget and a positive-duration fade tail.
    pub const fn new(freshness_ns: u64, fade_ns: u64) -> Option<Self> {
        if fade_ns == 0 {
            None
        } else {
            Some(Self {
                freshness_ns,
                fade_ns,
                last_good: None,
            })
        }
    }

    /// Records a timestamp only when a real window geometry observation is present.
    pub fn observe(
        &mut self,
        timestamp: MonotonicInstant,
        observation: &Observation<DesktopRect>,
    ) {
        if observation.is_known() {
            self.last_good = Some(timestamp);
        }
    }

    /// Returns the force multiplier: full through freshness, then linearly fades to zero.
    #[must_use]
    pub fn weight_at(self, now: MonotonicInstant) -> f64 {
        let Some(last_good) = self.last_good else {
            return 0.0;
        };
        let Some(age) = now.duration_since(last_good) else {
            return 0.0;
        };
        if age <= self.freshness_ns {
            return 1.0;
        }
        let fade_age = age - self.freshness_ns;
        if fade_age >= self.fade_ns {
            0.0
        } else {
            1.0 - (fade_age as f64 / self.fade_ns as f64)
        }
    }
}

