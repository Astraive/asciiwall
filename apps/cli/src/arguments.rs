use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ascii_convert::GlyphRepertoire;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::diagnostic::CliError;

#[derive(Debug, Parser)]
#[command(
    name = "asciiwall",
    about = "Headless AsciiWall conversion (some engine integrations are not yet available)",
    color = clap::ColorChoice::Never
)]
pub struct Cli {
    /// Emit a single machine-readable status object on stdout.
    #[arg(long, global = true)]
    pub json: bool,
    /// Suppress non-error human output.
    #[arg(long, global = true)]
    pub quiet: bool,
    /// Request additional diagnostics on stderr.
    #[arg(long, global = true)]
    pub verbose: bool,
    /// Read application settings from this path; the settings registry is unavailable in this build.
    /// A valid `convert` invocation using `--config` returns a capability error (exit 4).
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Disable diagnostic color; explicit ANSI artwork is unaffected.
    #[arg(long, global = true)]
    pub no_color: bool,
    /// Windows rejects recognized remote roots/provider flags and reparse points, then pins traversal and file I/O to checked handles; this does not guarantee no network I/O. Unsupported platforms reject offline conversion.
    #[arg(long, global = true)]
    pub offline: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Convert a local image without starting a controller or wallpaper host.
    Convert(ConvertArgs),
}

#[derive(Debug, Args)]
pub struct ConvertArgs {
    /// Local source image path.
    pub image: PathBuf,
    /// Destination path, or '-' for stdout.
    #[arg(long, value_name = "FILE")]
    pub output: PathBuf,
    /// Explicit local font path; no built-in font is currently supplied.
    #[arg(long, value_name = "PATH")]
    pub font: Option<PathBuf>,
    /// Requested output grid width.
    #[arg(long, default_value_t = 240, value_parser = clap::value_parser!(u32).range(1..))]
    pub columns: u32,
    /// Matching algorithm. Density is available; structural is recognized but unavailable
    /// (capability error, exit 4).
    #[arg(long, value_enum, default_value = "density")]
    pub matcher: Matcher,
    /// Strict supported single-cell glyph repertoire.
    #[arg(long, default_value = " .:-=+*#%@", value_name = "GLYPHS")]
    pub charset: String,
    /// Cell color. Monochrome is available; source color is recognized but unavailable
    /// (capability error, exit 4).
    #[arg(long, value_enum, default_value = "mono")]
    pub color: Color,
    /// Output format; inferred from .txt, .ans/.ansi, or .png. PNG is recognized but
    /// unavailable (capability error, exit 4).
    #[arg(long, value_enum)]
    pub format: Option<Format>,
    /// Replace an existing output only after a complete successful export.
    #[arg(long)]
    pub overwrite: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Matcher {
    Density,
    Structural,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Color {
    Mono,
    Source,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
pub enum Format {
    Txt,
    Ansi,
    Png,
}

impl Format {
    fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        if extension.eq_ignore_ascii_case("txt") {
            Some(Self::Txt)
        } else if extension.eq_ignore_ascii_case("ans") || extension.eq_ignore_ascii_case("ansi") {
            Some(Self::Ansi)
        } else if extension.eq_ignore_ascii_case("png") {
            Some(Self::Png)
        } else {
            None
        }
    }
}

impl ConvertArgs {
    /// Validates the request without opening or modifying the destination.
    pub fn validate(&self, json: bool) -> Result<(Format, GlyphRepertoire), CliError> {
        #[cfg(windows)]
        {
            self.validate_inner(json, None)
        }
        #[cfg(not(windows))]
        {
            self.validate_inner(json)
        }
    }

    #[cfg(windows)]
    pub(crate) fn validate_with_output_state(
        &self,
        json: bool,
        output_state: OutputPathState,
    ) -> Result<(Format, GlyphRepertoire), CliError> {
        self.validate_inner(json, Some(output_state))
    }

    fn validate_inner(
        &self,
        json: bool,
        #[cfg(windows)] bound_output_state: Option<OutputPathState>,
    ) -> Result<(Format, GlyphRepertoire), CliError> {
        if self.image.as_os_str().is_empty()
            || self.output.as_os_str().is_empty()
            || self
                .font
                .as_ref()
                .is_some_and(|path| path.as_os_str().is_empty())
        {
            return Err(CliError::usage(
                "input, font, and output paths must not be empty",
            ));
        }

        let stdout = self.output.as_os_str() == OsStr::new("-");
        if json && stdout {
            return Err(CliError::usage(
                "artwork and JSON status cannot both be written to stdout",
            ));
        }

        // Offline Windows callers supply state from the pinned destination directory handle;
        // other callers retain the path-based validation contract.
        #[cfg(windows)]
        let output_metadata = if !stdout && bound_output_state.is_none() {
            Some(fs::symlink_metadata(&self.output))
        } else {
            None
        };
        #[cfg(not(windows))]
        let output_metadata = if !stdout {
            Some(fs::symlink_metadata(&self.output))
        } else {
            None
        };
        #[cfg(windows)]
        let output_exists = match bound_output_state {
            Some(OutputPathState::Missing) => false,
            Some(OutputPathState::File | OutputPathState::Directory) => true,
            None => matches!(&output_metadata, Some(Ok(_))),
        };
        #[cfg(not(windows))]
        let output_exists = matches!(&output_metadata, Some(Ok(_)));
        if !stdout && !self.overwrite && output_exists {
            return Err(CliError::usage(
                "output already exists; use --overwrite to replace it",
            ));
        }

        if !stdout {
            #[cfg(windows)]
            match bound_output_state {
                Some(OutputPathState::Directory) => {
                    return Err(CliError::io("output path is a directory"));
                }
                Some(OutputPathState::Missing | OutputPathState::File) => {}
                None => match output_metadata.unwrap_or_else(|| fs::symlink_metadata(&self.output))
                {
                    Ok(metadata) if metadata.is_dir() => {
                        return Err(CliError::io("output path is a directory"));
                    }
                    Ok(_) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                        return Err(CliError::permission("cannot inspect output path"));
                    }
                    Err(_) => return Err(CliError::io("cannot inspect output path")),
                },
            }
            #[cfg(not(windows))]
            match output_metadata.unwrap_or_else(|| fs::symlink_metadata(&self.output)) {
                Ok(metadata) if metadata.is_dir() => {
                    return Err(CliError::io("output path is a directory"));
                }
                Ok(_) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => {
                    return Err(CliError::permission("cannot inspect output path"));
                }
                Err(_) => return Err(CliError::io("cannot inspect output path")),
            }
        }

        let inferred = if stdout {
            None
        } else {
            Format::from_path(&self.output)
        };
        if self.format.is_some() && inferred.is_some() && self.format != inferred {
            return Err(CliError::usage(
                "output format conflicts with the file extension",
            ));
        }
        let format = match (self.format, inferred, stdout) {
            (Some(format), _, _) | (None, Some(format), _) => format,
            (None, None, true) => Format::Txt,
            (None, None, false) => {
                return Err(CliError::usage(
                    "specify --format when the output extension does not identify a format",
                ));
            }
        };

        let repertoire = GlyphRepertoire::new(&self.charset).map_err(|_| {
            CliError::validation("charset must contain unique supported single-cell glyphs")
        })?;
        Ok((format, repertoire))
    }
}

#[cfg(windows)]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OutputPathState {
    Missing,
    File,
    Directory,
}
