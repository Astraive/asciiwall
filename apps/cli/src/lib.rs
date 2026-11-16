//! Standalone CLI request validation and shared-engine integration points.

mod arguments;
mod diagnostic;
pub mod convert;
pub mod font;
pub mod output;

use std::ffi::{OsStr, OsString};
use std::process::ExitCode;

use clap::{
    Parser,
    error::{ContextKind, ContextValue, ErrorKind},
};

use arguments::{Cli, Color, Command, Format, Matcher};
use diagnostic::{CliError, print_error, print_help};

/// Parses and handles one invocation without starting a controller or wallpaper host.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let args: Vec<OsString> = args.into_iter().collect();
    let json_requested = args
        .iter()
        .skip(1)
        .take_while(|arg| arg.as_os_str() != OsStr::new("--"))
        .any(|arg| arg.as_os_str() == OsStr::new("--json"));
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(error) if matches!(error.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            return if print_help(&error.to_string()).is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(8)
            };
        }
        Err(error) => {
            let command = parse_error_command(&error);
            let error = CliError::usage(
                "invalid command-line arguments; consult --help for usage details",
            );
            return report_error(&error, json_requested, command);
        }
    };

    let json = cli.json;
    match execute(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => report_error(&error, json, "convert"),
    }
}

fn parse_error_command(error: &clap::Error) -> &'static str {
    // The usage in Clap's error context identifies the command that parsed the failing arguments;
    // raw values must not be scanned for a token that happens to equal a subcommand.
    let Some(ContextValue::StyledStr(usage)) = error.get(ContextKind::Usage) else {
        return "asciiwall";
    };
    if usage.to_string().split_ascii_whitespace().nth(2) == Some("convert") {
        "convert"
    } else {
        "asciiwall"
    }
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
    let format = request.validate(cli.json)?;

    if request.matcher == Matcher::Structural {
        return Err(CliError::capability("structural matching is not available in this build"));
    }
    if request.color == Color::Source {
        return Err(CliError::capability("source-color density conversion is not available in this build"));
    }
    if cli.config.is_some() {
        return Err(CliError::capability("configuration loading requires the shared settings registry"));
    }
    if request.font.is_none() {
        return Err(CliError::capability("no built-in font is available; select a local font with --font"));
    }
    if format == Format::Png {
        return Err(CliError::capability("PNG still export is not available in this build"));
    }

    // Decode and text/ANSI export are owned by separate engine crates, which are not present on
    // this branch. Fail closed instead of guessing their API or claiming an artifact was created.
    Err(CliError::capability(
        "image decoding and text/ANSI export are not integrated in this build",
    ))
}
