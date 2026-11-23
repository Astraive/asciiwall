//! Standalone CLI request validation and shared-engine integration points.

mod arguments;
pub mod convert;
mod diagnostic;
pub mod font;
pub mod output;
#[cfg(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))]
mod posix_output;
#[cfg(windows)]
mod windows_path;

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
#[cfg(windows)]
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
#[cfg(windows)]
use font::load_glyph_table_with_repertoire_from_file;
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
    #[cfg(windows)]
    if args.iter().skip(1).any(|argument| {
        argument.as_os_str() == OsStr::new("--offline")
            && args.iter().skip(1).any(|argument| {
                let argument = argument.as_os_str();
                let value = argument
                    .to_str()
                    .and_then(|argument| argument.split_once('='))
                    .map_or(argument, |(_, value)| OsStr::new(value));
                is_unc_path(Path::new(value))
            })
    }) {
        return report_error(&offline_path_error(), json_requested, command, stdout);
    }
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

#[cfg(windows)]
fn prepare_offline_io(
    offline: bool,
    config: Option<&Path>,
    request: &arguments::ConvertArgs,
) -> Result<Option<windows_path::PreparedConvertIo>, CliError> {
    if !offline {
        return Ok(None);
    }
    // Preserve the raw spelling check before Windows path normalization can reinterpret
    // slash-style UNC paths as rooted local paths.
    let raw_paths = [
        Some(request.image.as_path()),
        request.font.as_deref(),
        (request.output.as_os_str() != OsStr::new("-")).then_some(request.output.as_path()),
        config,
    ];
    if raw_paths.into_iter().flatten().any(is_unc_path) {
        return Err(offline_path_error());
    }
    windows_path::prepare_offline_paths(request, config)
        .map(Some)
        .map_err(|error| {
            #[cfg(test)]
            eprintln!("{error}");
            if error.is_rejected() {
                offline_path_error()
            } else if error.is_permission_denied() {
                CliError::permission("offline path access was denied")
            } else {
                CliError::io("offline path verification failed")
            }
        })
}

#[cfg(all(windows, test))]
fn reject_offline_network_paths(
    offline: bool,
    config: Option<&Path>,
    request: &arguments::ConvertArgs,
) -> Result<(), CliError> {
    prepare_offline_io(offline, config, request).map(|_| ())
}

#[cfg(windows)]
fn offline_path_error() -> CliError {
    CliError::usage(
        "offline mode requires a known local filesystem and rejects network-backed, reparse, or unsafe paths",
    )
}

#[cfg(windows)]
fn is_unc_path(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Component, Prefix};

    let mut spelling = path.as_os_str().encode_wide();
    let first = spelling.next();
    let second = spelling.next();
    let third = spelling.next();
    let fourth = spelling.next();
    let is_separator = |unit| unit == 92 || unit == 47;
    if first.is_some_and(is_separator) && second.is_some_and(is_separator) {
        if third == Some(b'?' as u16) && fourth.is_some_and(is_separator) {
            let ascii_lower = |unit: u16| {
                if (b'A' as u16..=b'Z' as u16).contains(&unit) {
                    unit + (b'a' - b'A') as u16
                } else {
                    unit
                }
            };
            let unc = [spelling.next(), spelling.next(), spelling.next()];
            return unc
                .into_iter()
                .zip([b'u' as u16, b'n' as u16, b'c' as u16])
                .all(|(actual, expected)| {
                    actual.is_some_and(|actual| ascii_lower(actual) == expected)
                })
                && spelling.next().is_some_and(is_separator);
        }
        return third != Some(b'.' as u16);
    }

    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix))
            if matches!(prefix.kind(), Prefix::UNC(_, _) | Prefix::VerbatimUNC(_, _))
    )
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
    #[cfg(windows)]
    let mut offline_io = prepare_offline_io(offline, config.as_deref(), &request)?;
    #[cfg(not(windows))]
    if offline {
        // Path lookup itself can activate a remote mount. Without a portable pre-open locality
        // check, reject before validation inspects the destination or conversion opens an input.
        return Err(CliError::usage(
            "offline mode cannot guarantee that filesystem path resolution stays local on this platform",
        )
        .into());
    }
    #[cfg(windows)]
    let (format, repertoire) = if let Some(output) = offline_io
        .as_ref()
        .and_then(|prepared| prepared.output.as_ref())
    {
        request.validate_with_output_state(json, output.state)?
    } else {
        request.validate(json)?
    };
    #[cfg(not(windows))]
    let (format, repertoire) = request.validate(json)?;

    #[cfg(all(windows, test))]
    if offline {
        windows_path::test_hook(windows_path::TestHookPoint::Validated);
    }

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
    #[cfg(windows)]
    let font_result = if offline {
        match offline_io
            .as_mut()
            .and_then(|prepared| prepared.font.take())
        {
            Some(input) => input.into_file().and_then(|file| {
                load_glyph_table_with_repertoire_from_file(
                    file,
                    &repertoire,
                    &geometry,
                    &budget,
                    cancellation,
                )
            }),
            None => Err(FontLoadError::NotRegularFile),
        }
    } else {
        load_glyph_table_with_repertoire(font_path, &repertoire, &geometry, &budget, cancellation)
    };
    #[cfg(not(windows))]
    let font_result =
        load_glyph_table_with_repertoire(font_path, &repertoire, &geometry, &budget, cancellation);
    let glyphs = font_result.map_err(font_error)?;
    #[cfg(windows)]
    let mut input = if offline {
        match offline_io
            .as_mut()
            .and_then(|prepared| prepared.image.take())
        {
            Some(image) => image.into_file(),
            None => Err(FontLoadError::NotRegularFile),
        }
        .map_err(image_open_error)?
    } else {
        open_regular_input_file(&request.image, cancellation)
            .map_err(image_open_error)?
            .0
    };
    #[cfg(not(windows))]
    let mut input = open_regular_input_file(&request.image, cancellation)
        .map_err(image_open_error)?
        .0;
    let decode_limits = DecodeLimits::new(
        budget,
        DecodeResources::new(1, budget.max_frame_bytes()),
        16_384,
        16_384,
        budget.max_raster_bytes(),
        budget.max_frame_bytes(),
    );
    progress(verbose, "decoding source image");
    let decoded = decode_image(&mut input, &decode_limits, cancellation).map_err(core_error)?;
    drop(input);

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
        #[cfg(windows)]
        let publish_result = if offline {
            let prepared = offline_io
                .as_ref()
                .and_then(|prepared| prepared.output.as_ref())
                .ok_or_else(offline_path_error)?;
            prepared.publish(request.overwrite, cancellation, |file| {
                file.write_all(bytes)
            })
        } else {
            output::publish_file(&request.output, request.overwrite, cancellation, |file| {
                file.write_all(bytes)
            })
        };
        #[cfg(not(windows))]
        let publish_result =
            output::publish_file(&request.output, request.overwrite, cancellation, |file| {
                file.write_all(bytes)
            });
        publish_result.map_err(publish_error)?;
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

        output::PublishError::NoClobberUnsupported => {
            CliError::io("destination filesystem does not support atomic no-clobber publication")
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
    #[cfg(windows)]
    use std::fs;
    use std::io::{self, Write};
    use std::process::ExitCode;
    #[cfg(windows)]
    use std::sync::Arc;

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
        assert!(!is_unc_path(Path::new(r"\\?\C:\local\source.png")));
        assert!(!is_unc_path(Path::new("//?/C:/local/source.png")));

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
    #[cfg(windows)]
    #[test]
    fn offline_conversion_denies_source_rename_and_keeps_output_bound_across_junction_swaps() {
        use std::cell::Cell;
        use std::fs;
        use std::rc::Rc;

        use super::windows_path::{ERROR_SHARING_VIOLATION, TestHookPoint, install_test_hook};

        const RIGHTS_CLEARED_PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86,
            0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];

        let temp = super::windows_path::offline_test_tempdir_for_directory_rename();
        let root = temp.path();
        let input_dir = root.join("input");
        let moved_input_dir = root.join("input-pinned");
        let output_dir = root.join("output");
        let moved_output_dir = root.join("output-pinned");
        let input_decoy = root.join("input-decoy");
        let output_decoy_one = root.join("output-decoy-one");
        let output_decoy_two = root.join("output-decoy-two");
        fs::create_dir(&input_dir).unwrap();
        fs::create_dir(&output_dir).unwrap();
        fs::create_dir(&input_decoy).unwrap();
        fs::create_dir(&output_decoy_one).unwrap();
        fs::create_dir(&output_decoy_two).unwrap();

        let image = input_dir.join("source.png");
        let font = input_dir.join("font.ttf");
        fs::write(&image, RIGHTS_CLEARED_PNG).unwrap();
        fs::write(&font, synthetic_font::synthetic_font(false)).unwrap();
        fs::write(input_decoy.join("source.png"), b"not a PNG").unwrap();
        fs::write(input_decoy.join("font.ttf"), b"not a font").unwrap();

        let output = output_dir.join("result.txt");
        fs::write(&output, b"old destination").unwrap();
        let calls = Rc::new(Cell::new((false, false)));
        let staging_reached = Rc::new(Cell::new(false));
        let hook_staging_reached = Rc::clone(&staging_reached);
        let hook_calls = Rc::clone(&calls);
        let hook_input = input_dir.clone();
        let hook_output = output_dir.clone();
        let hook_output_pinned = moved_output_dir.clone();
        let hook_output_decoy_one = output_decoy_one.clone();
        let hook_output_decoy_two = output_decoy_two.clone();
        let _hook = install_test_hook(move |point| match point {
            TestHookPoint::RootResolved => {
                // Output-path junction swaps happen only after its parent handle is pinned.
            }
            TestHookPoint::Validated => {
                assert!(!hook_calls.get().0);
                let input_leaf = hook_input.join("source.png");
                let error = fs::rename(&input_leaf, hook_input.join("source-pinned.png"))
                    .expect_err("the pinned input handle must deny delete-sharing rename");
                assert_eq!(
                    error.raw_os_error(),
                    Some(ERROR_SHARING_VIOLATION),
                    "the input share mode must deny the source rename: {error:?}"
                );
                fs::rename(&hook_output, &hook_output_pinned).unwrap();
                create_directory_junction(&hook_output, &hook_output_decoy_one).unwrap();
                hook_calls.set((true, false));
            }
            TestHookPoint::Staged => {
                hook_staging_reached.set(true);
                assert!(hook_calls.get().0);
                assert!(!hook_calls.get().1);
                let staging_directory = fs::read_dir(&hook_output_pinned)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name().is_some_and(|name| {
                            name.to_string_lossy().starts_with(".asciiwall-stage-")
                        })
                    })
                    .expect("the private staging directory must exist before publication");
                let staging = fs::read_dir(&staging_directory)
                    .unwrap()
                    .map(|entry| entry.unwrap().path())
                    .find(|path| {
                        path.file_name()
                            .is_some_and(|name| name.to_string_lossy().starts_with(".asciiwall-"))
                    })
                    .expect("the private staging file must exist before publication");
                let staging_moved = staging_directory.join(".saved-staging.tmp");
                fs::rename(&staging, &staging_moved).unwrap();
                fs::write(&staging, b"unrelated replacement").unwrap();
                fs::remove_dir(&hook_output).unwrap();
                create_directory_junction(&hook_output, &hook_output_decoy_two).unwrap();
                hook_calls.set((true, true));
            }
        });

        let args = vec![
            OsString::from("asciiwall"),
            OsString::from("--quiet"),
            OsString::from("--offline"),
            OsString::from("convert"),
            image.as_os_str().to_owned(),
            OsString::from("--font"),
            font.as_os_str().to_owned(),
            OsString::from("--output"),
            output.as_os_str().to_owned(),
            OsString::from("--overwrite"),
            OsString::from("--columns"),
            OsString::from("12"),
            OsString::from("--charset"),
            OsString::from(" AB"),
        ];
        let cancellation = CancellationToken::new();
        let mut stdout = Vec::new();
        let exit = super::run_with_cancellation_to(args.clone(), &cancellation, &mut stdout);

        assert_eq!(
            exit,
            ExitCode::SUCCESS,
            "argv={args:?}; output={output:?}; AfterStaging reached={}; hook state={:?}; publish rename OS error={:?}; stdout={:?}; stderr is suppressed by --quiet",
            staging_reached.get(),
            calls.get(),
            super::windows_path::test_last_publish_rename_error(),
            String::from_utf8_lossy(&stdout),
        );

        assert_eq!(calls.get(), (true, true));
        fs::rename(&input_dir, &moved_input_dir)
            .expect("input directory rename succeeds after the pinned handles are released");
        create_directory_junction(&input_dir, &input_decoy).unwrap();
        let published = moved_output_dir.join("result.txt");
        assert!(
            fs::metadata(&published).is_ok(),
            "publication must remain relative to the pinned output directory handle"
        );
        assert!(
            !output_decoy_one.join("result.txt").exists()
                && !output_decoy_two.join("result.txt").exists(),
            "replacing the output path with local junctions must not redirect writes"
        );
        assert_eq!(
            fs::read(moved_input_dir.join("source.png")).unwrap(),
            RIGHTS_CLEARED_PNG
        );
        assert_eq!(
            fs::read(input_dir.join("source.png")).unwrap(),
            b"not a PNG"
        );
        assert_ne!(
            published.metadata().unwrap().len(),
            0,
            "the artifact must have been produced from the already-opened valid source and font"
        );
        assert_ne!(fs::read(&published).unwrap(), b"old destination");
        let staging_directories: Vec<_> = fs::read_dir(&moved_output_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".asciiwall-stage-"))
            })
            .collect();
        assert_eq!(staging_directories.len(), 1);
        let staging_decoys: Vec<_> = fs::read_dir(&staging_directories[0])
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(".asciiwall-"))
            })
            .collect();
        assert_eq!(staging_decoys.len(), 1);
        assert_eq!(
            fs::read(&staging_decoys[0]).unwrap(),
            b"unrelated replacement",
            "handle-based publication and cleanup must leave a replaced staging name untouched"
        );

        fn create_directory_junction(
            link: &std::path::Path,
            target: &std::path::Path,
        ) -> std::io::Result<()> {
            use std::ffi::c_void;
            use std::io;
            use std::os::windows::ffi::OsStrExt;
            use std::os::windows::io::{AsRawHandle, FromRawHandle};
            use std::ptr;
            const GENERIC_WRITE: u32 = 0x4000_0000;
            const SHARE_ALL: u32 = 7;
            const OPEN_EXISTING: u32 = 3;
            const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
            const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
            const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00a4;
            const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xa000_0003;
            const INVALID_HANDLE_VALUE: *mut c_void = -1isize as *mut c_void;

            unsafe extern "system" {
                fn CreateDirectoryW(path_name: *const u16, security_attributes: *mut c_void)
                -> i32;
                fn CreateFileW(
                    file_name: *const u16,
                    desired_access: u32,
                    share_mode: u32,
                    security_attributes: *mut c_void,
                    creation_disposition: u32,
                    flags_and_attributes: u32,
                    template_file: *mut c_void,
                ) -> *mut c_void;
                fn DeviceIoControl(
                    device: *mut c_void,
                    control_code: u32,
                    input_buffer: *mut c_void,
                    input_buffer_size: u32,
                    output_buffer: *mut c_void,
                    output_buffer_size: u32,
                    bytes_returned: *mut u32,
                    overlapped: *mut c_void,
                ) -> i32;
            }

            let link_wide: Vec<u16> = link.as_os_str().encode_wide().chain(Some(0)).collect();
            if unsafe { CreateDirectoryW(link_wide.as_ptr(), ptr::null_mut()) } == 0 {
                return Err(io::Error::last_os_error());
            }
            let handle = unsafe {
                CreateFileW(
                    link_wide.as_ptr(),
                    GENERIC_WRITE,
                    SHARE_ALL,
                    ptr::null_mut(),
                    OPEN_EXISTING,
                    FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS,
                    ptr::null_mut(),
                )
            };
            if handle == INVALID_HANDLE_VALUE {
                let error = io::Error::last_os_error();
                std::fs::remove_dir(link)?;
                return Err(error);
            }
            let handle = unsafe { std::fs::File::from_raw_handle(handle as _) };

            let target = std::fs::canonicalize(target)?;
            let mut print_name: Vec<u16> = target.as_os_str().encode_wide().collect();
            if print_name.starts_with(&[92, 92, 63, 92]) {
                drop(print_name.drain(..4));
            }
            let mut substitute_name: Vec<u16> = vec![92, 63, 63, 92];
            substitute_name.extend_from_slice(&print_name);
            let substitute_len = substitute_name.len() * 2;
            let print_len = print_name.len() * 2;
            let path_bytes = (substitute_name.len() + 1 + print_name.len() + 1) * 2;
            let data_length = 8 + path_bytes;
            let mut reparse = vec![0_u8; 16 + path_bytes];
            reparse[0..4].copy_from_slice(&IO_REPARSE_TAG_MOUNT_POINT.to_le_bytes());
            reparse[4..6].copy_from_slice(&(data_length as u16).to_le_bytes());
            reparse[8..10].copy_from_slice(&0_u16.to_le_bytes());
            reparse[10..12].copy_from_slice(&(substitute_len as u16).to_le_bytes());
            reparse[12..14].copy_from_slice(&((substitute_len + 2) as u16).to_le_bytes());
            reparse[14..16].copy_from_slice(&(print_len as u16).to_le_bytes());
            let path = &mut reparse[16..];
            for (index, unit) in substitute_name.iter().copied().enumerate() {
                path[index * 2..index * 2 + 2].copy_from_slice(&unit.to_le_bytes());
            }
            let print_offset = substitute_name.len() + 1;
            for (index, unit) in print_name.iter().copied().enumerate() {
                let offset = (print_offset + index) * 2;
                path[offset..offset + 2].copy_from_slice(&unit.to_le_bytes());
            }
            let mut returned = 0;
            let succeeded = unsafe {
                DeviceIoControl(
                    handle.as_raw_handle(),
                    FSCTL_SET_REPARSE_POINT,
                    reparse.as_mut_ptr().cast(),
                    reparse.len() as u32,
                    ptr::null_mut(),
                    0,
                    &mut returned,
                    ptr::null_mut(),
                )
            };
            if succeeded == 0 {
                let error = io::Error::last_os_error();
                drop(handle);
                std::fs::remove_dir(link)?;
                return Err(error);
            }
            Ok(())
        }
    }
    #[cfg(windows)]
    #[test]
    fn offline_overwrite_replaces_decoded_input_path() {
        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86,
            0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];

        let temp = super::windows_path::offline_test_tempdir();
        let image_and_output = temp.path().join("source.txt");
        let font = temp.path().join("font.ttf");
        fs::write(&image_and_output, PNG).unwrap();
        fs::write(&font, synthetic_font::synthetic_font(false)).unwrap();
        let args = [
            OsString::from("asciiwall"),
            OsString::from("--quiet"),
            OsString::from("--offline"),
            OsString::from("convert"),
            image_and_output.as_os_str().to_owned(),
            OsString::from("--font"),
            font.as_os_str().to_owned(),
            OsString::from("--output"),
            image_and_output.as_os_str().to_owned(),
            OsString::from("--overwrite"),
            OsString::from("--columns"),
            OsString::from("12"),
            OsString::from("--charset"),
            OsString::from(" AB"),
        ];
        let mut stdout = Vec::new();
        let exit = run_with_cancellation_to(args, &CancellationToken::new(), &mut stdout);

        assert_eq!(exit, ExitCode::SUCCESS);
        let converted = fs::read(&image_and_output).unwrap();
        assert_ne!(converted, PNG);
        assert!(!converted.is_empty());
        assert!(converted.ends_with(b"\n"));
    }

    #[cfg(windows)]
    #[test]
    fn offline_output_preserves_existing_precedence_and_no_clobber_cleanup() {
        use std::cell::{Cell, RefCell};
        use std::rc::Rc;

        use super::windows_path::{TestHookPoint, install_test_hook};

        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86,
            0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];

        let temp = super::windows_path::offline_test_tempdir();
        let image = temp.path().join("source.png");
        let font = temp.path().join("font.ttf");
        let output = temp.path().join("result.txt");
        fs::write(&image, PNG).unwrap();
        fs::write(&font, synthetic_font::synthetic_font(false)).unwrap();
        fs::write(&output, b"original").unwrap();
        let make_args = |charset: &str| {
            vec![
                OsString::from("asciiwall"),
                OsString::from("--quiet"),
                OsString::from("--offline"),
                OsString::from("convert"),
                image.as_os_str().to_owned(),
                OsString::from("--font"),
                font.as_os_str().to_owned(),
                OsString::from("--output"),
                output.as_os_str().to_owned(),
                OsString::from("--columns"),
                OsString::from("12"),
                OsString::from("--charset"),
                OsString::from(charset),
            ]
        };

        let invalid_args = make_args("\u{1b}");
        let cancellation = CancellationToken::new();
        let mut invalid_stdout = Vec::new();
        let invalid_exit = super::run_with_cancellation_to(
            invalid_args.clone(),
            &cancellation,
            &mut invalid_stdout,
        );
        assert_eq!(
            invalid_exit,
            ExitCode::from(2),
            "argv={invalid_args:?}; output={output:?}; stdout={:?}; stderr is suppressed by --quiet",
            String::from_utf8_lossy(&invalid_stdout),
        );
        assert_eq!(fs::read(&output).unwrap(), b"original");

        fs::remove_file(&output).unwrap();
        let racing_output = output.clone();
        let staging_reached = Rc::new(Cell::new(false));
        let hook_staging_reached = Rc::clone(&staging_reached);
        let hook_write_error = Rc::new(RefCell::new(None::<String>));
        let hook_error = Rc::clone(&hook_write_error);
        let _hook = install_test_hook(move |point| {
            if point == TestHookPoint::Staged {
                hook_staging_reached.set(true);
                let parent = racing_output.parent();
                let before_write = format!(
                    "path={racing_output:?}, path_exists={}, parent={parent:?}, parent_exists={}, parent_is_dir={}",
                    racing_output.exists(),
                    parent.is_some_and(|parent| parent.exists()),
                    parent.is_some_and(|parent| parent.is_dir()),
                );
                if let Err(error) = fs::write(&racing_output, b"concurrent creator") {
                    *hook_error.borrow_mut() = Some(format!("{before_write}, write_error={error}"));
                }
            }
        });
        let args = make_args(" AB");
        let mut stdout = Vec::new();
        let exit = super::run_with_cancellation_to(args.clone(), &cancellation, &mut stdout);
        assert_eq!(
            exit,
            ExitCode::from(2),
            "argv={args:?}; output={output:?}; AfterStaging reached={}; hook write error={:?}; stdout={:?}; stderr is suppressed by --quiet",
            staging_reached.get(),
            hook_write_error.borrow(),
            String::from_utf8_lossy(&stdout),
        );
        assert!(
            staging_reached.get(),
            "AfterStaging was not reached; argv={args:?}; output={output:?}; stdout={:?}; stderr is suppressed by --quiet",
            String::from_utf8_lossy(&stdout),
        );
        assert!(
            hook_write_error.borrow().is_none(),
            "concurrent destination creation failed in AfterStaging: {:?}; argv={args:?}; output={output:?}; stdout={:?}; stderr is suppressed by --quiet",
            hook_write_error.borrow(),
            String::from_utf8_lossy(&stdout),
        );
        assert_eq!(fs::read(&output).unwrap(), b"concurrent creator");
        let entries: Vec<_> = fs::read_dir(temp.path()).unwrap().collect();
        assert_eq!(
            entries.len(),
            3,
            "failed publication must remove its staging file"
        );
    }

    #[cfg(windows)]
    #[test]
    fn offline_cancellation_after_staging_removes_the_private_file() {
        use std::cell::Cell;
        use std::rc::Rc;

        use super::windows_path::{TestHookPoint, install_test_hook};

        const PNG: &[u8] = &[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48,
            0x44, 0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00,
            0x00, 0x1f, 0x15, 0xc4, 0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78,
            0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff, 0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86,
            0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
        ];

        let temp = super::windows_path::offline_test_tempdir();
        let image = temp.path().join("source.png");
        let font = temp.path().join("font.ttf");
        let output = temp.path().join("result.txt");
        fs::write(&image, PNG).unwrap();
        fs::write(&font, synthetic_font::synthetic_font(false)).unwrap();
        let cancellation = Arc::new(CancellationToken::new());
        let hook_cancellation = Arc::clone(&cancellation);
        let staging_reached = Rc::new(Cell::new(false));
        let hook_staging_reached = Rc::clone(&staging_reached);
        let _hook = install_test_hook(move |point| {
            if point == TestHookPoint::Staged {
                hook_staging_reached.set(true);
                hook_cancellation.cancel();
            }
        });
        let args = vec![
            OsString::from("asciiwall"),
            OsString::from("--quiet"),
            OsString::from("--offline"),
            OsString::from("convert"),
            image.as_os_str().to_owned(),
            OsString::from("--font"),
            font.as_os_str().to_owned(),
            OsString::from("--output"),
            output.as_os_str().to_owned(),
            OsString::from("--columns"),
            OsString::from("12"),
            OsString::from("--charset"),
            OsString::from(" AB"),
        ];
        let mut stdout = Vec::new();
        let exit = super::run_with_cancellation_to(args.clone(), &cancellation, &mut stdout);

        assert_eq!(
            exit,
            ExitCode::from(10),
            "argv={args:?}; output={output:?}; AfterStaging reached={}; output exists={}; stdout={:?}; stderr is suppressed by --quiet",
            staging_reached.get(),
            output.exists(),
            String::from_utf8_lossy(&stdout),
        );
        assert!(!output.exists());
        assert_eq!(
            fs::read_dir(temp.path()).unwrap().count(),
            2,
            "cancelled publication must remove its staging file"
        );
    }
}
