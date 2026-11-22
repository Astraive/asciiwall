use std::ffi::OsStr;
use std::fs;
use std::num::NonZeroU32;
use std::path::Path;
use std::process::{Command, Output};

use ascii_convert::{GlyphConfig, GlyphRepertoire, NormalizedRasterResources};
use ascii_core::{Budget, CancellationToken};
use ascii_media::{DecodeLimits, DecodeResources, decode_image};
use ascii_render::{AnsiColorMode, ExportTarget, export};
use asciiwall_cli::convert::monochrome_density;
use asciiwall_cli::font::load_glyph_table_with_repertoire;

#[allow(dead_code)]
#[path = "../../../crates/ascii-convert/tests/support/synthetic_font.rs"]
mod synthetic_font;

// A generated 1×1 RGBA PNG fixture; no third-party image asset is used.
const RIGHTS_CLEARED_PNG: &[u8] = &[
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1f, 0x15, 0xc4,
    0x89, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xff, 0xff, 0xff,
    0x7f, 0x00, 0x09, 0xfb, 0x03, 0xfd, 0x2a, 0x86, 0xe3, 0x8a, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45,
    0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
];

fn run<I, S>(args: I) -> Output
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new(env!("CARGO_BIN_EXE_asciiwall"))
        .args(args)
        .output()
        .expect("CLI process starts")
}

fn status(output: &Output) -> i32 {
    output.status.code().expect("CLI exited with a status")
}

fn shared_engine_export(source_path: &Path, font_path: &Path, target: ExportTarget) -> Vec<u8> {
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let raster_resources = NormalizedRasterResources::new(budget.max_raster_bytes());
    let cancellation = CancellationToken::new();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let repertoire = GlyphRepertoire::new(" AB").unwrap();
    let glyphs =
        load_glyph_table_with_repertoire(font_path, &repertoire, &geometry, &budget, &cancellation)
            .unwrap();
    let decode_limits = DecodeLimits::new(
        budget,
        DecodeResources::new(1, budget.max_frame_bytes()),
        16_384,
        16_384,
        budget.max_raster_bytes(),
        budget.max_frame_bytes(),
    );
    let mut source = fs::File::open(source_path).unwrap();
    let decoded = decode_image(&mut source, &decode_limits, &cancellation).unwrap();
    let converted = monochrome_density(
        decoded.view(),
        &glyphs,
        NonZeroU32::new(12).unwrap(),
        &budget,
        &raster_resources,
        &cancellation,
    )
    .unwrap();
    export(converted.frame(), &glyphs, target, &budget, &cancellation)
        .unwrap()
        .bytes()
        .to_vec()
}

#[test]
fn help_describes_headless_convert_and_missing_integrations() {
    let output = run(["--help"]);
    assert_eq!(status(&output), 0);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("convert"));
    assert!(help.contains("not yet available"));
    assert!(help.contains("--config"));
    assert!(help.contains("unavailable in this build"));
    assert!(help.contains("capability error"));
    assert!(help.contains("exit 4"));
    let output = run(["convert", "--help"]);
    assert_eq!(status(&output), 0);
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--output",
        "--font",
        "--columns",
        "--matcher",
        "--charset",
        "--color",
        "--format",
        "--overwrite",
    ] {
        assert!(help.contains(flag), "missing {flag}");
    }
}

#[test]
fn malformed_requests_return_usage_before_any_conversion() {
    for args in [
        vec!["convert", "input.png"],
        vec![
            "convert",
            "input.png",
            "--output",
            "image.txt",
            "--columns",
            "0",
        ],
        vec![
            "convert",
            "input.png",
            "--output",
            "image.txt",
            "--matcher",
            "unknown",
        ],
        vec!["convert", "input.png", "--output", "image.unknown"],
        vec![
            "convert",
            "input.png",
            "--output",
            "image.txt",
            "--format",
            "png",
        ],
        vec!["--json", "convert", "input.png", "--output", "-"],
        vec![
            "convert",
            "input.png",
            "--output",
            "-",
            "--format",
            "png",
            "--json",
        ],
    ] {
        let output = run(&args);
        assert_eq!(status(&output), 2, "args: {args:?}");
        if args.contains(&"--json") {
            let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(body["schema_version"], 1);
            assert_eq!(body["status"], "error");
            assert_eq!(body["error"]["category"], "usage");
            assert!(output.stderr.is_empty());
        } else {
            assert!(output.stdout.is_empty());
            assert!(!output.stderr.is_empty());
        }
    }
}
#[test]
fn charset_validation_uses_the_shared_single_cell_repertoire() {
    for charset in ["A", "░", "⣿"] {
        let output = run([
            "convert",
            "input.png",
            "--output",
            "-",
            "--charset",
            charset,
        ]);
        assert_eq!(status(&output), 4, "charset: {charset}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("no built-in font"),
            "charset: {charset}"
        );
    }
}

#[test]
fn existing_spaced_paths_and_format_conflicts_never_modify_output() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("existing artwork.txt");
    fs::write(&destination, b"previous output").unwrap();
    let input = dir.path().join("input image.png");

    let output = run([
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--charset"),
        OsStr::new("aa"),
    ]);
    assert_eq!(status(&output), 2);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
    let output = run([
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--format"),
        OsStr::new("png"),
    ]);
    assert_eq!(status(&output), 2);
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("output already exists"), "{message}");
    assert!(!message.contains("output format conflicts"), "{message}");
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    let output = run([
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--overwrite"),
        OsStr::new("--charset"),
        OsStr::new("aa"),
    ]);
    assert_eq!(status(&output), 3);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    let output = run([
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--overwrite"),
        OsStr::new("--format"),
        OsStr::new("png"),
    ]);
    assert_eq!(status(&output), 2);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    // An explicit but missing font blocks conversion and cannot partially overwrite the target.
    let output = run([
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--font"),
        OsStr::new("font with spaces.ttf"),
        OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&output), 8);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
}

#[test]
fn unavailable_matcher_color_and_configuration_are_explicit() {
    for (extra, expected) in [
        (vec!["--matcher", "structural"], "structural matching"),
        (vec!["--color", "source"], "source-color"),
        (
            vec!["--config", "settings.toml", "--offline"],
            "settings registry",
        ),
        (vec![], "no built-in font"),
    ] {
        let mut args = vec!["--quiet", "convert", "image.png", "--output", "-"];
        args.extend(extra);
        let output = run(args);
        assert_eq!(status(&output), 4);
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8(output.stderr).unwrap().contains(expected));
    }
}

#[cfg(windows)]
#[test]
fn offline_unc_paths_are_rejected_before_file_access() {
    let dir = tempfile::tempdir().unwrap();
    let existing_output = dir.path().join("existing output.txt");
    fs::write(&existing_output, b"preserve this output").unwrap();
    let assert_rejected = |output: Output| {
        assert_eq!(status(&output), 2);
        assert!(output.stdout.is_empty());
        assert!(
            String::from_utf8(output.stderr)
                .unwrap()
                .contains("offline mode does not allow network-backed UNC paths")
        );
    };

    // Existing local output would win request validation before any source/font access if the
    // offline lexical preflight ran too late.
    assert_rejected(run([
        OsStr::new("--offline"),
        OsStr::new("convert"),
        OsStr::new(r"\\offline.invalid\share\source.png"),
        OsStr::new("--font"),
        OsStr::new("local font.ttf"),
        OsStr::new("--output"),
        existing_output.as_os_str(),
    ]));
    assert_rejected(run([
        OsStr::new("--offline"),
        OsStr::new("convert"),
        OsStr::new("missing source.png"),
        OsStr::new("--font"),
        OsStr::new(r"\\?\UNC\offline.invalid\share\font.ttf"),
        OsStr::new("--output"),
        existing_output.as_os_str(),
    ]));
    assert_rejected(run([
        OsStr::new("--offline"),
        OsStr::new("--config"),
        OsStr::new(r"\\offline.invalid\share\settings.toml"),
        OsStr::new("convert"),
        OsStr::new("missing source.png"),
        OsStr::new("--font"),
        OsStr::new("local font.ttf"),
        OsStr::new("--output"),
        existing_output.as_os_str(),
    ]));

    // An empty image fails request validation before destination metadata if offline preflight
    // were absent, keeping this output-path regression away from share access.
    assert_rejected(run([
        OsStr::new("--offline"),
        OsStr::new("convert"),
        OsStr::new(""),
        OsStr::new("--output"),
        OsStr::new("//offline.invalid/share/output.txt"),
    ]));
    assert_eq!(fs::read(existing_output).unwrap(), b"preserve this output");
}

#[test]
fn shared_decoder_density_and_text_ansi_export_are_wired_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let source_path = dir.path().join("rights-cleared source image.png");
    let invalid_path = dir.path().join("malformed source image.png");
    let font_path = dir.path().join("rights-cleared font.ttf");
    fs::write(&source_path, RIGHTS_CLEARED_PNG).unwrap();
    fs::write(&invalid_path, b"not a PNG or JPEG").unwrap();
    fs::write(&font_path, synthetic_font::synthetic_font(false)).unwrap();

    let text_output = run([
        OsStr::new("--verbose"),
        OsStr::new("--no-color"),
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--columns"),
        OsStr::new("12"),
        OsStr::new("--charset"),
        OsStr::new(" AB"),
        OsStr::new("--format"),
        OsStr::new("txt"),
        OsStr::new("--output"),
        OsStr::new("-"),
    ]);
    assert_eq!(status(&text_output), 0);
    assert!(!text_output.stdout.is_empty());
    let text = String::from_utf8(text_output.stdout.clone()).unwrap();
    assert_eq!(
        text_output.stdout.as_slice(),
        shared_engine_export(&source_path, &font_path, ExportTarget::Text)
    );
    assert!(text.ends_with('\n'));
    assert!(text.lines().all(|line| line.chars().count() == 12));
    assert!(!text.contains('\x1b'));
    assert!(
        text.chars()
            .all(|glyph| glyph == '\n' || " AB".contains(glyph))
    );
    let progress = String::from_utf8(text_output.stderr).unwrap();
    assert!(progress.contains("decoding source image"));
    assert!(!progress.contains(source_path.to_string_lossy().as_ref()));
    assert!(!progress.contains('\x1b'));

    let text_path = dir.path().join("converted output with spaces.txt");
    fs::write(&text_path, b"previous text output").unwrap();
    let text_file_output = run([
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--quiet"),
        OsStr::new("--offline"),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--columns"),
        OsStr::new("12"),
        OsStr::new("--charset"),
        OsStr::new(" AB"),
        OsStr::new("--format"),
        OsStr::new("txt"),
        OsStr::new("--output"),
        text_path.as_os_str(),
        OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&text_file_output), 0);
    assert!(text_file_output.stdout.is_empty());
    assert!(text_file_output.stderr.is_empty());
    assert_eq!(fs::read(&text_path).unwrap(), text.as_bytes());

    let ansi_output = run([
        OsStr::new("--quiet"),
        OsStr::new("--offline"),
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--columns"),
        OsStr::new("12"),
        OsStr::new("--charset"),
        OsStr::new(" AB"),
        OsStr::new("--format"),
        OsStr::new("ansi"),
        OsStr::new("--output"),
        OsStr::new("-"),
    ]);
    assert_eq!(status(&ansi_output), 0);
    let ansi = String::from_utf8(ansi_output.stdout.clone()).unwrap();
    assert_eq!(
        ansi_output.stdout.as_slice(),
        shared_engine_export(
            &source_path,
            &font_path,
            ExportTarget::Ansi(AnsiColorMode::TrueColor)
        )
    );
    assert!(ansi.starts_with("\x1b[0m\x1b[38;2;255;255;255;48;2;0;0;0m"));
    assert!(ansi.ends_with("\x1b[0m\n"));
    assert!(!ansi.contains("\x1b]"));
    assert!(!ansi.contains("\x1bP"));
    assert!(ansi_output.stderr.is_empty());

    let ansi_path = dir.path().join("converted output with spaces.ans");
    let ansi_file_output = run([
        OsStr::new("--quiet"),
        OsStr::new("--offline"),
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--columns"),
        OsStr::new("12"),
        OsStr::new("--charset"),
        OsStr::new(" AB"),
        OsStr::new("--format"),
        OsStr::new("ansi"),
        OsStr::new("--output"),
        ansi_path.as_os_str(),
    ]);
    assert_eq!(status(&ansi_file_output), 0);
    assert!(ansi_file_output.stderr.is_empty());
    assert_eq!(fs::read(&ansi_path).unwrap(), ansi_output.stdout);

    let json_path = dir.path().join("machine-readable output.txt");
    let json_output = run([
        OsStr::new("--json"),
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--columns"),
        OsStr::new("12"),
        OsStr::new("--charset"),
        OsStr::new(" AB"),
        OsStr::new("--format"),
        OsStr::new("txt"),
        OsStr::new("--output"),
        json_path.as_os_str(),
    ]);
    assert_eq!(status(&json_output), 0);
    assert!(json_output.stderr.is_empty());
    let json: serde_json::Value = serde_json::from_slice(&json_output.stdout).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["command"], "convert");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["format"], "txt");
    assert_eq!(json["columns"], 12);
    assert_eq!(fs::read(&json_path).unwrap(), text.as_bytes());

    for (format, destination) in [("txt", &text_path), ("ansi", &ansi_path)] {
        fs::write(destination, b"preserve this output").unwrap();
        let failed = run([
            OsStr::new("--quiet"),
            OsStr::new("--offline"),
            OsStr::new("convert"),
            invalid_path.as_os_str(),
            OsStr::new("--font"),
            font_path.as_os_str(),
            OsStr::new("--columns"),
            OsStr::new("12"),
            OsStr::new("--charset"),
            OsStr::new(" AB"),
            OsStr::new("--format"),
            OsStr::new(format),
            OsStr::new("--output"),
            destination.as_os_str(),
            OsStr::new("--overwrite"),
        ]);
        assert_eq!(status(&failed), 6);
        assert!(failed.stdout.is_empty());
        let message = String::from_utf8(failed.stderr).unwrap();
        assert!(!message.contains(invalid_path.to_string_lossy().as_ref()));
        assert!(!message.contains(font_path.to_string_lossy().as_ref()));
        assert!(!message.contains('\x1b'));
        assert_eq!(fs::read(destination).unwrap(), b"preserve this output");
    }

    let png_path = dir.path().join("preserved output.png");
    fs::write(&png_path, b"preserve this output").unwrap();
    let png_output = run([
        OsStr::new("--quiet"),
        OsStr::new("--offline"),
        OsStr::new("convert"),
        source_path.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--format"),
        OsStr::new("png"),
        OsStr::new("--output"),
        png_path.as_os_str(),
        OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&png_output), 4);
    assert!(png_output.stdout.is_empty());
    assert!(
        String::from_utf8(png_output.stderr)
            .unwrap()
            .contains("PNG still export")
    );
    assert_eq!(fs::read(&png_path).unwrap(), b"preserve this output");
}

#[test]
fn explicit_font_failures_block_conversion_without_disclosure_or_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("private input.png");
    let font_path = dir.path().join("private font.ttf");
    let destination = dir.path().join("existing artwork.txt");
    fs::write(&font_path, b"not a font").unwrap();
    fs::write(&destination, b"previous output").unwrap();

    let output = run([
        OsStr::new("--json"),
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&output), 3);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["command"], "convert");
    assert_eq!(body["error"]["category"], "validation");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("font is invalid"));
    assert!(!message.contains(input.to_string_lossy().as_ref()));
    assert!(!message.contains(font_path.to_string_lossy().as_ref()));
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    let missing_font = dir.path().join("missing private font.ttf");
    let output = run([
        OsStr::new("--json"),
        OsStr::new("convert"),
        input.as_os_str(),
        OsStr::new("--font"),
        missing_font.as_os_str(),
        OsStr::new("--output"),
        destination.as_os_str(),
        OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&output), 8);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["error"]["category"], "io");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("cannot read selected font input"));
    assert!(!message.contains(input.to_string_lossy().as_ref()));
    assert!(!message.contains(missing_font.to_string_lossy().as_ref()));
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
}

#[test]
fn missing_selected_font_glyph_returns_stable_validation_diagnostic() {
    let dir = tempfile::tempdir().unwrap();
    let font_path = dir.path().join("private font.ttf");
    let output_path = dir.path().join("artwork.txt");
    fs::write(&font_path, synthetic_font::synthetic_font(false)).unwrap();

    let output = run([
        OsStr::new("--json"),
        OsStr::new("convert"),
        OsStr::new("unopened input.png"),
        OsStr::new("--font"),
        font_path.as_os_str(),
        OsStr::new("--charset"),
        OsStr::new("C"),
        OsStr::new("--format"),
        OsStr::new("txt"),
        OsStr::new("--output"),
        output_path.as_os_str(),
    ]);

    assert_eq!(status(&output), 3);
    assert!(output.stderr.is_empty());
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["schema_version"], 1);
    assert_eq!(body["command"], "convert");
    assert_eq!(body["status"], "error");
    assert_eq!(body["error"]["code"], "cli.validation");
    assert_eq!(body["error"]["category"], "validation");
    assert_eq!(
        body["error"]["message"],
        "selected font lacks a requested glyph"
    );
    assert_eq!(body["error"]["retryable"], false);
    assert!(!output_path.exists());
}

#[test]
fn unsafe_glyphs_and_terminal_sequences_cannot_escape_diagnostics() {
    let output = run([
        "convert",
        "image.png",
        "--output",
        "-",
        "--charset",
        "a\u{1b}]52;c;c2VjcmV0\u{7}b",
    ]);
    assert_eq!(status(&output), 3);
    assert!(!output.stderr.contains(&0x1b));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("c2VjcmV0"));

    let output = run([
        "--json",
        "convert",
        "image.png",
        "--output",
        "-",
        "--bad-\u{1b}]0;owned\u{7}",
    ]);
    assert_eq!(status(&output), 2);
    assert!(!output.stdout.contains(&0x1b));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        !body["error"]["message"]
            .as_str()
            .unwrap()
            .contains('\u{1b}')
    );
}

#[test]
fn parser_diagnostics_do_not_echo_values_and_identify_the_parsed_command() {
    let private_path = "C:/Users/alice/private-input.png";
    let private_matcher = "super-secret-matcher-value";
    let output = run([
        "--json",
        "convert",
        private_path,
        "--output",
        "-",
        "--matcher",
        private_matcher,
    ]);
    assert_eq!(status(&output), 2);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["command"], "convert");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(!message.contains(private_path));
    assert!(!message.contains(private_matcher));

    let output = run(["--json", "--config", "convert"]);
    assert_eq!(status(&output), 2);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["command"], "asciiwall");
    assert!(
        !body["error"]["message"]
            .as_str()
            .unwrap()
            .contains("convert")
    );
}

#[test]
fn missing_font_blocks_conversion_without_disclosing_input_path() {
    let private_path = "C:/Users/alice/private-input.png";
    let output = run([
        "--json",
        "convert",
        private_path,
        "--output",
        "safe-output.txt",
    ]);
    assert_eq!(status(&output), 4);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["command"], "convert");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(message.contains("no built-in font"));
    assert!(!message.contains(private_path));
}

#[test]
fn capability_diagnostics_do_not_disclose_private_config_hosts() {
    let private_host = "private-host.example";
    let private_config = "https://private-host.example/private/settings.toml";
    let output = run([
        "--json",
        "convert",
        "input.png",
        "--output",
        "safe-output.txt",
        "--config",
        private_config,
    ]);
    assert_eq!(status(&output), 4);
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(body["command"], "convert");
    let message = body["error"]["message"].as_str().unwrap();
    assert!(!message.contains(private_host));
    assert!(!message.contains(private_config));
}

#[cfg(unix)]
#[test]
fn dangling_output_symlink_is_treated_as_existing() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.txt");
    symlink(Path::new("missing-target"), &output_path).unwrap();
    let output = run([
        OsStr::new("convert"),
        OsStr::new("input.png"),
        OsStr::new("--output"),
        output_path.as_os_str(),
    ]);
    assert_eq!(status(&output), 2);
    assert_eq!(
        fs::read_link(output_path).unwrap().as_path(),
        Path::new("missing-target")
    );
}
