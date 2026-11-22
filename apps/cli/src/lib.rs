//! Standalone CLI request validation and shared-engine integration points.

mod arguments;
pub mod convert;
mod diagnostic;
pub mod font;
pub mod output;

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};

use arguments::{Cli, Color, Command, Format, Matcher};
use diagnostic::{CliError, print_error, print_help};

use ascii_convert::{
    ColorNormalizationError, DensityConversionError, GlyphConfig, NormalizedRasterResources,
};
use ascii_core::{Budget, CancellationToken, Error, GlyphFailureReason};
use ascii_media::{DecodeLimits, DecodeResources, decode_image};
use ascii_render::{AnsiColorMode, ExportError, ExportTarget, export};
use font::{FontLoadError, load_glyph_table_with_repertoire, open_regular_input_file};
/// Parses and handles one invocation without starting a controller or wallpaper host.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let cancellation = CancellationToken::new();
    run_with_cancellation(args, &cancellation)
}

/// Runs one invocation with the caller's signal-aware cancellation token.
pub fn run_with_cancellation(
    args: impl IntoIterator<Item = OsString>,
    cancellation: &CancellationToken,
) -> ExitCode {
    let stdout = io::stdout();
    run_with_cancellation_to(args, cancellation, &mut stdout.lock())
}

fn run_with_cancellation_to(
    args: impl IntoIterator<Item = OsString>,
    cancellation: &CancellationToken,
    stdout: &mut impl Write,
) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    let json_requested = args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_os_str() != OsStr::new("--"))
        .any(|arg| arg.as_os_str() == OsStr::new("--json"));
    let command = parse_error_command(&args);
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            return if print_help(&error.to_string(), stdout).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(8)
            };
        }
        Err(_) => {
            let error =
                CliError::usage("invalid command-line arguments; consult --help for usage details");
            return report_error(&error, json_requested, command, stdout);
        }
    };

    let json = cli.json;
    let result = execute(cli, cancellation, stdout);
    finish_execution(result, |error| report_error(error, json, "convert", stdout))
}

fn parse_error_command(args: &[OsString]) -> &'static str {
    // Only recognize the first positional token after root-level globals. In particular, consume
    // --config's value so a value that happens to be "convert" is never mistaken for a command.
    let mut arguments = args.iter().skip(1);
    while let Some(argument) = arguments.next() {
        let argument = argument.as_os_str();
        if argument == OsStr::new("--config") {
            let Some(value) = arguments.next() else {
                return "asciiwall";
            };
            let value = value.to_string_lossy();
            if value.starts_with('-') && value != "-" {
                return "asciiwall";
            }
            continue;
        }
        if argument.to_string_lossy().starts_with("--config=") {
            continue;
        }
        if argument == OsStr::new("--json")
            || argument == OsStr::new("--quiet")
            || argument == OsStr::new("--verbose")
            || argument == OsStr::new("--no-color")
            || argument == OsStr::new("--offline")
        {
            continue;
        }
        if argument == OsStr::new("convert") {
            return "convert";
        }
        // The root parser has no positional arguments besides its subcommand. A terminator,
        // unknown option, or non-command token therefore leaves the command unparsed.
        return "asciiwall";
    }
    "asciiwall"
}

fn report_error(
    error: &CliError,
    json: bool,
    command: &'static str,
    stdout: &mut impl Write,
) -> ExitCode {
    if print_error(error, json, command, stdout).is_ok() {
        ExitCode::from(error.exit)
    } else {
        ExitCode::from(8)
    }
}

fn finish_execution(
    result: Result<(), ExecutionError>,
    report_cli_error: impl FnOnce(&CliError) -> ExitCode,
) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(ExecutionError::Cli(error)) => report_cli_error(&error),
        Err(ExecutionError::SuccessStatus(error)) => ExitCode::from(error.exit),
    }
}

enum ExecutionError {
    Cli(CliError),
    SuccessStatus(CliError),
}

impl From<CliError> for ExecutionError {
    fn from(error: CliError) -> Self {
        Self::Cli(error)
    }
}

fn reject_offline_network_paths(
    offline: bool,
    config: Option<&Path>,
    request: &arguments::ConvertArgs,
) -> Result<(), CliError> {
    if !offline {
        return Ok(());
    }

    #[cfg(windows)]
    {
        let mut current_directory = None;
        let image = resolve_offline_path(&request.image, &mut current_directory)
            .map_err(|()| offline_path_error())?;
        let font = request
            .font
            .as_deref()
            .map(|path| resolve_offline_path(path, &mut current_directory))
            .transpose()
            .map_err(|()| offline_path_error())?;
        let output = if request.output.as_os_str() == OsStr::new("-") {
            None
        } else {
            Some(
                resolve_offline_path(&request.output, &mut current_directory)
                    .map_err(|()| offline_path_error())?,
            )
        };
        let config = config
            .map(|path| resolve_offline_path(path, &mut current_directory))
            .transpose()
            .map_err(|()| offline_path_error())?;
        let paths = [Some(&image), font.as_ref(), output.as_ref(), config.as_ref()];

        // Inspect all effective roots before probing any path component, so a network path in
        // one argument vetoes the invocation before local destination metadata can take priority.
        for path in paths.into_iter().flatten() {
            let path = path.as_ref();
            if is_unc_path(path) || is_remote_drive_path(path) {
                return Err(offline_path_error());
            }
        }

        // Check each component without following its final reparse point. Stop at the first one
        // before any descendant can be resolved through a junction or symlink target.
        for path in [Some(&image), font.as_ref(), output.as_ref(), config.as_ref()]
            .into_iter()
            .flatten()
        {
            if path_has_reparse_component(path.as_ref()).unwrap_or(true) {
                return Err(offline_path_error());
            }
        }
    }

    #[cfg(not(windows))]
    let _ = (config, request);

    Ok(())
}

#[cfg(windows)]
fn offline_path_error() -> CliError {
    CliError::usage(
        "offline mode does not allow network-backed UNC paths, mapped drives, non-volume device namespaces, or filesystem reparse points; paths must be safely inspectable",
    )
}

#[cfg(windows)]
fn resolve_offline_path<'a>(
    path: &'a Path,
    current_directory: &mut Option<std::path::PathBuf>,
) -> Result<std::borrow::Cow<'a, Path>, ()> {
    if path.is_absolute() {
        return Ok(std::borrow::Cow::Borrowed(path));
    }

    if current_directory.is_none() {
        // On Windows this obtains the process path with GetCurrentDirectoryW; it does not inspect
        // file contents or follow a reparse point.
        *current_directory = Some(std::env::current_dir().map_err(|_| ())?);
    }
    let mut absolute = current_directory.as_ref().ok_or(())?.clone();
    absolute.push(path);
    if !absolute.is_absolute() {
        return Err(());
    }
    Ok(std::borrow::Cow::Owned(absolute))
}

#[cfg(windows)]
fn is_unc_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};

    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
    )
}

#[cfg(windows)]
fn is_remote_drive_path(path: &Path) -> bool {
    use std::path::{Component, Prefix};

    let drive = match path.components().next() {
        Some(Component::Prefix(prefix)) => match prefix.kind() {
            Prefix::Disk(drive) | Prefix::VerbatimDisk(drive) => drive,
            Prefix::DeviceNS(_) => return true,
            Prefix::Verbatim(prefix) => return !is_local_volume_prefix(prefix),
            _ => return false,
        },
        _ => return false,
    };

    const DRIVE_REMOTE: u32 = 4;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetDriveTypeW(root_path_name: *const u16) -> u32;
    }

    let root_path = [u16::from(drive), b':' as u16, b'\\' as u16, 0];
    // The root-only drive-type query classifies a mapped drive without opening a share path.
    // SAFETY: `root_path` is a fixed, NUL-terminated UTF-16 buffer for this call.
    unsafe { GetDriveTypeW(root_path.as_ptr()) == DRIVE_REMOTE }
}

#[cfg(windows)]
fn is_local_volume_prefix(prefix: &OsStr) -> bool {
    let Some(prefix) = prefix.to_str() else {
        return false;
    };
    prefix
        .get(..7)
        .is_some_and(|start| start.eq_ignore_ascii_case("Volume{"))
        && prefix.ends_with('}')
}

#[cfg(windows)]
fn path_has_reparse_component(path: &Path) -> Result<bool, ()> {
    use std::os::windows::fs::MetadataExt;
    use std::path::{Component, PathBuf};

    const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
    let mut resolved = PathBuf::new();
    let mut checked_component_depth = 0usize;
    let mut unresolved_suffix_depth = 0usize;

    // Do not probe descendants of a missing or non-directory component. Track the unresolved
    // suffix so a later `..` can return to a previously checked prefix.
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => {
                resolved.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if unresolved_suffix_depth > 0 {
                    resolved.pop();
                    unresolved_suffix_depth -= 1;
                } else if checked_component_depth > 0 {
                    resolved.pop();
                    checked_component_depth -= 1;
                }
            }
            Component::Normal(_) => {
                resolved.push(component.as_os_str());
                if unresolved_suffix_depth > 0 {
                    unresolved_suffix_depth += 1;
                    continue;
                }
                match std::fs::symlink_metadata(&resolved) {
                    Ok(metadata)
                        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 =>
                    {
                        return Ok(true);
                    }
                    Ok(_) => checked_component_depth += 1,
                    Err(error)
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                        ) =>
                    {
                        unresolved_suffix_depth = 1;
                    }
                    Err(_) => return Err(()),
                }
            }
        }
    }

    Ok(false)
}

fn execute(
    cli: Cli,
    cancellation: &CancellationToken,
    stdout: &mut impl Write,
) -> Result<(), ExecutionError> {
    let Cli {
        json,
        quiet,
        verbose,
        offline,
        config,
        command,
        ..
    } = cli;
    let Command::Convert(request) = command;
    // This veto must precede `validate`, which inspects destination metadata.
    reject_offline_network_paths(offline, config.as_deref(), &request)?;
    let (format, repertoire) = request.validate(json)?;

    if config.is_some() {
        return Err(CliError::capability(
            "configuration loading requires the shared settings registry",
        )
        .into());
    }
    if request.matcher == Matcher::Structural {
        return Err(
            CliError::capability("structural matching is not available in this build").into(),
        );
    }
    if request.color == Color::Source {
        return Err(CliError::capability(
            "source-color density conversion is not available in this build",
        )
        .into());
    }
    if format == Format::Png {
        return Err(CliError::capability(
            "PNG still export requires the shared renderer's PNG export provider",
        )
        .into());
    }
    if cancellation.is_cancelled() {
        return Err(CliError::cancelled("conversion was cancelled").into());
    }

    let font_path = request.font.as_deref().ok_or_else(|| {
        CliError::capability("no built-in font is available; select a local font with --font")
    })?;
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let normalized_raster_resources = NormalizedRasterResources::new(budget.max_raster_bytes());
    let geometry = GlyphConfig::new(8.0, 8, 12, 9)
        .map_err(|_| CliError::validation("configured font geometry is invalid"))?;
    let verbose = verbose && !quiet;
    progress(verbose, "measuring selected font");
    let glyphs =
        load_glyph_table_with_repertoire(font_path, &repertoire, &geometry, &budget, cancellation)
            .map_err(font_error)?;

    progress(verbose, "decoding source image");
    let (mut input, _) =
        open_regular_input_file(&request.image, cancellation).map_err(image_open_error)?;
    let decode_limits = DecodeLimits::new(
        budget,
        DecodeResources::new(1, budget.max_frame_bytes()),
        16_384,
        16_384,
        budget.max_raster_bytes(),
        budget.max_frame_bytes(),
    );
    let decoded = decode_image(&mut input, &decode_limits, cancellation).map_err(core_error)?;

    progress(verbose, "matching image density");
    let columns = std::num::NonZeroU32::new(request.columns)
        .ok_or_else(|| CliError::validation("columns must be positive"))?;
    let converted = convert::monochrome_density(
        decoded.view(),
        &glyphs,
        columns,
        &budget,
        &normalized_raster_resources,
        cancellation,
    )
    .map_err(density_error)?;

    progress(verbose, "serializing terminal output");
    let target = match format {
        Format::Txt => ExportTarget::Text,
        Format::Ansi => ExportTarget::Ansi(AnsiColorMode::TrueColor),
        Format::Png => {
            return Err(CliError::capability(
                "PNG still export requires the shared renderer's PNG export provider",
            )
            .into());
        }
    };
    let exported =
        export(converted.frame(), &glyphs, target, &budget, cancellation).map_err(export_error)?;
    let bytes = exported.bytes();
    if request.output.as_os_str() == OsStr::new("-") {
        publish_stdout(bytes, cancellation, stdout)?;
    } else {
        output::publish_file(&request.output, request.overwrite, cancellation, |file| {
            file.write_all(bytes)
        })
        .map_err(publish_error)?;
    }

    report_success(
        SuccessReport {
            json,
            quiet,
            verbose,
            format,
            columns: exported.size().columns(),
            rows: exported.size().rows(),
            loss_notice: exported.loss_notice(),
        },
        |status| write_success_status(stdout, status),
    )
    .map_err(ExecutionError::SuccessStatus)?;
    Ok(())
}

fn publish_stdout(
    bytes: &[u8],
    cancellation: &CancellationToken,
    stdout: &mut impl Write,
) -> Result<(), CliError> {
    if cancellation.is_cancelled() {
        return Err(CliError::cancelled(
            "conversion was cancelled before output publication",
        ));
    }
    stdout
        .write_all(bytes)
        .and_then(|()| stdout.flush())
        .map_err(|_| CliError::io("cannot write conversion output"))
}

fn progress(enabled: bool, message: &'static str) {
    if enabled {
        let _ = writeln!(io::stderr().lock(), "{message}");
    }
}

struct SuccessReport<'a> {
    json: bool,
    quiet: bool,
    verbose: bool,
    format: Format,
    columns: u32,
    rows: u32,
    loss_notice: &'a str,
}

fn report_success(
    report: SuccessReport<'_>,
    status_writer: impl FnOnce(&serde_json::Value) -> Result<(), CliError>,
) -> Result<(), CliError> {
    let SuccessReport {
        json,
        quiet,
        verbose,
        format,
        columns,
        rows,
        loss_notice,
    } = report;
    if json {
        let status = serde_json::json!({
            "schema_version": 1,
            "command": "convert",
            "status": "ok",
            "format": format_name(format),
            "columns": columns,
            "rows": rows,
        });
        status_writer(&status)?;
    } else if !quiet {
        let _ = writeln!(
            io::stderr().lock(),
            "conversion complete ({columns} columns, {rows} rows)"
        );
    }
    if verbose && !quiet {
        let _ = writeln!(io::stderr().lock(), "{loss_notice}");
    }
    Ok(())
}

fn write_success_status(
    stdout: &mut impl Write,
    status: &serde_json::Value,
) -> Result<(), CliError> {
    serde_json::to_writer(&mut *stdout, status)
        .map_err(|_| CliError::io("cannot write conversion status"))?;
    stdout
        .write_all(b"\n")
        .and_then(|()| stdout.flush())
        .map_err(|_| CliError::io("cannot write conversion status"))
}

const fn format_name(format: Format) -> &'static str {
    match format {
        Format::Txt => "txt",
        Format::Ansi => "ansi",
        Format::Png => "png",
    }
}

fn image_open_error(error: FontLoadError) -> CliError {
    match error {
        FontLoadError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            CliError::permission("cannot access image input")
        }
        FontLoadError::Io(_) => CliError::io("cannot read image input"),
        FontLoadError::NotRegularFile => CliError::validation("image input must be a regular file"),
        FontLoadError::TooLarge { .. } => {
            CliError::resource("image input exceeds the configured resource limit")
        }
        FontLoadError::Engine(error) => core_error(error),
    }
}

fn density_error(error: DensityConversionError) -> CliError {
    match error {
        DensityConversionError::Core(error)
        | DensityConversionError::ColorNormalization(ColorNormalizationError::Raster(error)) => {
            core_error(error)
        }
        DensityConversionError::ColorNormalization(ColorNormalizationError::UnsupportedColor(
            _,
        )) => CliError::media("image color metadata is unsupported"),
        DensityConversionError::ColorNormalization(ColorNormalizationError::InvalidSample(_)) => {
            CliError::media("image contains invalid color samples")
        }
    }
}

fn core_error(error: Error) -> CliError {
    match error {
        Error::PermissionDenied => CliError::permission("cannot access image input"),
        Error::MissingAsset => CliError::io("cannot read image input"),
        Error::TemporarilyUnavailable => {
            CliError::capability("image processing is temporarily unavailable")
        }
        Error::Glyph { reason, .. } => glyph_error(reason),
        Error::Malformed(_) | Error::Unsupported(_) => {
            CliError::media("image input is invalid or unsupported")
        }
        Error::BudgetExceeded(_) | Error::SizeOverflow | Error::AllocationFailed => {
            CliError::resource("image conversion exceeds configured resource limits")
        }
        Error::Cancelled => CliError::cancelled("conversion was cancelled"),
    }
}

fn glyph_error(reason: GlyphFailureReason) -> CliError {
    match reason {
        GlyphFailureReason::UnsupportedScalar | GlyphFailureReason::DuplicateScalar => {
            CliError::validation("charset must contain unique supported single-cell glyphs")
        }
        GlyphFailureReason::GlyphUnavailable => {
            CliError::validation("selected font lacks a requested glyph")
        }
        GlyphFailureReason::MultiCellMetrics => {
            CliError::validation("selected font cannot fit a requested glyph in one cell")
        }
    }
}

fn export_error(error: ExportError) -> CliError {
    match error {
        ExportError::Core(error) => core_error(error),
        ExportError::UnsupportedGlyph { .. } => {
            CliError::validation("glyph table contains unsupported text")
        }
    }
}

fn publish_error(error: output::PublishError) -> CliError {
    match error {
        output::PublishError::Cancelled => {
            CliError::cancelled("conversion was cancelled before output publication")
        }
        output::PublishError::DestinationExists => {
            CliError::usage("output already exists; use --overwrite to replace it")
        }
        output::PublishError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            CliError::permission("cannot write conversion output")
        }
        output::PublishError::Io(_) => CliError::io("cannot write conversion output"),
    }
}

fn font_error(error: FontLoadError) -> CliError {
    match error {
        FontLoadError::Io(error) if error.kind() == io::ErrorKind::PermissionDenied => {
            CliError::permission("cannot access selected font input")
        }
        FontLoadError::Io(_) => CliError::io("cannot read selected font input"),
        FontLoadError::NotRegularFile => CliError::validation("font input must be a regular file"),
        FontLoadError::TooLarge { .. } => {
            CliError::resource("font input exceeds the configured resource limit")
        }
        FontLoadError::Engine(Error::PermissionDenied) => {
            CliError::permission("cannot access selected font input")
        }
        FontLoadError::Engine(Error::MissingAsset) => {
            CliError::io("cannot read selected font input")
        }
        FontLoadError::Engine(Error::TemporarilyUnavailable) => {
            CliError::capability("font measurement is temporarily unavailable")
        }
        FontLoadError::Engine(Error::Glyph { reason, .. }) => glyph_error(reason),
        FontLoadError::Engine(Error::Malformed(_) | Error::Unsupported(_)) => {
            CliError::validation("font is invalid or unsupported")
        }
        FontLoadError::Engine(
            Error::BudgetExceeded(_) | Error::SizeOverflow | Error::AllocationFailed,
        ) => CliError::resource("font exceeds configured resource limits"),
        FontLoadError::Engine(Error::Cancelled) => {
            CliError::cancelled("font loading was cancelled")
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::io::{self, Write};
    use std::process::ExitCode;

    use ascii_core::CancellationToken;

    #[cfg(windows)]
    use super::{is_unc_path, reject_offline_network_paths};
    use super::{publish_error, publish_stdout, run_with_cancellation_to, write_success_status};

    #[allow(dead_code)]
    mod synthetic_font {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../crates/ascii-convert/tests/support/synthetic_font.rs"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn offline_unc_paths_are_rejected_before_local_path_metadata() {
        use std::path::{Path, PathBuf};

        use super::arguments::{Color, ConvertArgs, Format, Matcher};

        let local_image = PathBuf::from(r"C:\local\source.png");
        let local_font = PathBuf::from(r"C:\local\font.ttf");
        let local_output = PathBuf::from(r"C:\local\output.txt");
        let mut request = ConvertArgs {
            image: local_image.clone(),
            output: local_output.clone(),
            font: Some(local_font.clone()),
            columns: 80,
            matcher: Matcher::Density,
            charset: " .".to_owned(),
            color: Color::Mono,
            format: Some(Format::Txt),
            overwrite: false,
        };

        for unc_path in [
            r"\\server\share\source.png",
            "//server/share/font.ttf",
            r"\\?\UNC\server\share\output.txt",
        ] {
            assert!(is_unc_path(Path::new(unc_path)));

            request.image = PathBuf::from(unc_path);
            assert_eq!(
                reject_offline_network_paths(true, None, &request)
                    .unwrap_err()
                    .exit,
                2
            );

            request.image = local_image.clone();
            request.font = Some(PathBuf::from(unc_path));
            assert_eq!(
                reject_offline_network_paths(true, None, &request)
                    .unwrap_err()
                    .exit,
                2
            );

            request.font = Some(local_font.clone());
            request.output = PathBuf::from(unc_path);
            assert_eq!(
                reject_offline_network_paths(true, None, &request)
                    .unwrap_err()
                    .exit,
                2
            );

            request.output = local_output.clone();
            assert_eq!(
                reject_offline_network_paths(true, Some(Path::new(unc_path)), &request)
                    .unwrap_err()
                    .exit,
                2
            );
        }

        assert!(reject_offline_network_paths(false, None, &request).is_ok());
    }

    struct FailAfterPartialSuccessStatus {
        bytes: Vec<u8>,
        pending_json: Vec<u8>,
        success_status_attempts: usize,
        error_response_attempts: usize,
        success_status: Option<serde_json::Value>,
    }

    impl Write for FailAfterPartialSuccessStatus {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let ends_with_newline = bytes.ends_with(b"\n");
            let response_bytes = if ends_with_newline {
                &bytes[..bytes.len() - 1]
            } else {
                bytes
            };
            self.pending_json.extend_from_slice(response_bytes);
            if let Ok(response) = serde_json::from_slice::<serde_json::Value>(&self.pending_json) {
                if response["schema_version"] == 1
                    && response["command"] == "convert"
                    && response["status"] == "ok"
                    && response["format"] == "txt"
                    && response["columns"] == 12
                    && response["rows"].as_u64().is_some_and(|rows| rows > 0)
                {
                    self.success_status_attempts += 1;
                    self.success_status = Some(response);
                    self.bytes.extend_from_slice(&self.pending_json[..4]);
                    self.pending_json.clear();
                    return Err(io::Error::other(
                        "simulated partial success status write failure",
                    ));
                }
                if response["schema_version"] == 1
                    && response["command"] == "convert"
                    && response["status"] == "error"
                    && response["error"].is_object()
                    && response["result"].is_null()
                {
                    self.error_response_attempts += 1;
                    self.bytes.extend_from_slice(&self.pending_json);
                    self.pending_json.clear();
                }
            }
            if ends_with_newline {
                self.bytes.push(b'\n');
            }
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailOnNewline {
        bytes: Vec<u8>,
    }

    impl Write for FailOnNewline {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes == b"\n" {
                return Err(io::Error::other("simulated newline write failure"));
            }
            self.bytes.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    struct FailOnWrite;

    impl Write for FailOnWrite {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("simulated status write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn stdout_publication_checks_cancellation_before_writing() {
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let mut stdout = Vec::new();

        let error = publish_stdout(b"artwork", &cancellation, &mut stdout).unwrap_err();

        assert_eq!(error.category, "cancelled");
        assert_eq!(error.exit, 10);
        assert!(stdout.is_empty());
    }

    #[test]
    fn publication_existing_output_race_is_usage_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("output.txt");
        let cancellation = CancellationToken::new();
        let publication_error =
            crate::output::publish_file(&destination, false, &cancellation, |file| {
                file.write_all(b"our output")?;
                std::fs::write(&destination, b"racing output")?;
                Ok(())
            })
            .unwrap_err();
        let error = publish_error(publication_error);

        assert_eq!(error.code, "cli.usage");
        assert_eq!(error.exit, 2);
        assert_eq!(
            error.message,
            "output already exists; use --overwrite to replace it"
        );
    }

    #[test]
    fn non_destination_already_exists_is_an_io_error() {
        let dir = tempfile::tempdir().unwrap();
        let destination = dir.path().join("output.txt");
        let occupied = dir.path().join("occupied.txt");
        std::fs::write(&occupied, b"occupied").unwrap();
        let cancellation = CancellationToken::new();
        let publication_error =
            crate::output::publish_file(&destination, false, &cancellation, |file| {
                file.write_all(b"staged output")?;
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&occupied)
                    .map(|_| ())
            })
            .unwrap_err();
        assert!(
            matches!(&publication_error, crate::output::PublishError::Io(error)
                if error.kind() == io::ErrorKind::AlreadyExists)
        );

        let error = publish_error(publication_error);

        assert_eq!(error.code, "cli.io");
        assert_eq!(error.category, "io");
        assert_eq!(error.exit, 8);
        assert!(!destination.exists());
        assert_eq!(std::fs::read(&occupied).unwrap(), b"occupied");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn success_json_writer_errors_are_reported() {
        let mut stdout = FailOnWrite;

        let error =
            write_success_status(&mut stdout, &serde_json::json!({"status": "ok"})).unwrap_err();

        assert_eq!(error.category, "io");
        assert_eq!(error.exit, 8);
    }

    #[test]
    fn success_json_newline_errors_are_reported() {
        let mut stdout = FailOnNewline { bytes: Vec::new() };

        let error =
            write_success_status(&mut stdout, &serde_json::json!({"status": "ok"})).unwrap_err();

        assert_eq!(error.category, "io");
        assert_eq!(error.exit, 8);
        assert!(!stdout.bytes.is_empty());
    }

    #[test]
    fn top_level_partial_success_status_write_does_not_attempt_second_json_response() {
        const RIGHTS_CLEARED_PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86,
            0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];
        let directory = tempfile::tempdir().unwrap();
        let image = directory.path().join("image.png");
        let font = directory.path().join("font.ttf");
        let output = directory.path().join("published.txt");
        std::fs::write(&image, RIGHTS_CLEARED_PNG).unwrap();
        std::fs::write(&font, synthetic_font::synthetic_font(false)).unwrap();

        let mut stdout = FailAfterPartialSuccessStatus {
            bytes: Vec::new(),
            pending_json: Vec::new(),
            success_status_attempts: 0,
            error_response_attempts: 0,
            success_status: None,
        };
        let exit = run_with_cancellation_to(
            [
                OsString::from("asciiwall"),
                OsString::from("--json"),
                OsString::from("--quiet"),
                OsString::from("convert"),
                image.as_os_str().to_owned(),
                OsString::from("--font"),
                font.as_os_str().to_owned(),
                OsString::from("--output"),
                output.as_os_str().to_owned(),
                OsString::from("--columns"),
                OsString::from("12"),
                OsString::from("--charset"),
                OsString::from(" "),
                OsString::from("--format"),
                OsString::from("txt"),
            ],
            &CancellationToken::new(),
            &mut stdout,
        );

        assert_eq!(exit, ExitCode::from(8));
        assert_eq!(
            stdout.success_status_attempts, 1,
            "exactly one success JSON response must be attempted"
        );
        assert_eq!(
            stdout.error_response_attempts, 0,
            "a failed success response must not trigger an error JSON response"
        );
        assert!(
            stdout.pending_json.is_empty(),
            "no incomplete JSON response"
        );
        let success_status = stdout
            .success_status
            .as_ref()
            .expect("the response writer must identify the success status");
        assert_eq!(success_status["schema_version"], 1);
        assert_eq!(success_status["command"], "convert");
        assert_eq!(success_status["status"], "ok");
        assert_eq!(success_status["format"], "txt");
        assert_eq!(success_status["columns"], 12);
        let status_json = serde_json::to_vec(success_status).unwrap();
        assert_eq!(stdout.bytes, status_json[..4]);

        let rows = success_status["rows"].as_u64().unwrap();
        assert!(rows > 0);
        let mut expected_artifact = Vec::new();
        for _ in 0..rows {
            expected_artifact.extend_from_slice(b"            \n");
        }
        assert_eq!(
            std::fs::read(&output).unwrap(),
            expected_artifact,
            "the fully published text artifact must survive the status failure"
        );
    }
}
