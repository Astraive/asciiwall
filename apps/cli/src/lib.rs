//! Standalone CLI request validation and shared-engine integration points.

mod arguments;
pub mod convert;
mod diagnostic;
pub mod font;
pub mod output;

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use clap::{Parser, error::ErrorKind};

use arguments::{Cli, Color, Command, Format, Matcher};
use diagnostic::{CliError, print_error, print_help};

use ascii_convert::GlyphConfig;
use ascii_core::{Budget, CancellationToken};
use font::{FontLoadError, load_glyph_table_with_repertoire};
/// Parses and handles one invocation without starting a controller or wallpaper host.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
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
            return if print_help(&error.to_string()).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(8)
            };
        }
        Err(_) => {
            let error =
                CliError::usage("invalid command-line arguments; consult --help for usage details");
            return report_error(&error, json_requested, command);
        }
    };

    let json = cli.json;
    match execute(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(&error, json, "convert"),
    }
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

fn report_error(error: &CliError, json: bool, command: &'static str) -> ExitCode {
    if print_error(error, json, command).is_ok() {
        ExitCode::from(error.exit)
    } else {
        ExitCode::from(8)
    }
}

fn execute(cli: Cli) -> Result<(), CliError> {
    let Command::Convert(request) = cli.command;
    let (format, repertoire) = request.validate(cli.json)?;

    if cli.config.is_some() {
        return Err(CliError::capability(
            "configuration loading requires the shared settings registry",
        ));
    }
    if request.matcher == Matcher::Structural {
        return Err(CliError::capability(
            "structural matching is not available in this build",
        ));
    }
    if request.color == Color::Source {
        return Err(CliError::capability(
            "source-color density conversion is not available in this build",
        ));
    }

    let font_path = request.font.as_deref().ok_or_else(|| {
        CliError::capability("no built-in font is available; select a local font with --font")
    })?;
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9)
        .map_err(|_| CliError::validation("configured font geometry is invalid"))?;
    let _glyphs =
        load_glyph_table_with_repertoire(font_path, &repertoire, &geometry, &budget, &cancellation)
            .map_err(font_error)?;

    if format == Format::Png {
        return Err(CliError::capability(
            "PNG still export is not available in this build",
        ));
    }

    // The bounded media decoder interface is not present in this workspace, so no input bytes
    // are interpreted as a raster and no output is published by this CLI invocation.
    Err(CliError::capability(
        "image decoding and text/ANSI export are not integrated in this build",
    ))
}

fn font_error(error: FontLoadError) -> CliError {
    match error {
        FontLoadError::Io(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            CliError::permission("cannot access selected font input")
        }
        FontLoadError::Io(_) => CliError::io("cannot read selected font input"),
        FontLoadError::NotRegularFile => CliError::validation("font input must be a regular file"),
        FontLoadError::TooLarge { .. } => {
            CliError::validation("font input exceeds the configured resource limit")
        }
        FontLoadError::Engine(error) => match error {
            ascii_core::Error::PermissionDenied => {
                CliError::permission("cannot access selected font input")
            }
            ascii_core::Error::MissingAsset => CliError::io("cannot read selected font input"),
            ascii_core::Error::TemporarilyUnavailable => {
                CliError::capability("font measurement is temporarily unavailable")
            }
            ascii_core::Error::Malformed(_) | ascii_core::Error::Unsupported(_) => {
                CliError::validation("font is invalid or unsupported")
            }
            ascii_core::Error::BudgetExceeded(_) | ascii_core::Error::SizeOverflow => {
                CliError::validation("font exceeds resource limits")
            }
            ascii_core::Error::AllocationFailed => CliError::io("font could not be loaded"),
            ascii_core::Error::Cancelled => CliError::validation("font loading was cancelled"),
        },
    }
}
