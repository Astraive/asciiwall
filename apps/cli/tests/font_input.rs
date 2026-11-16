// Reuse the repository's generated, rights-cleared test font; it is not a production font.
#[path = "../../../crates/ascii-convert/tests/support/synthetic_font.rs"]
mod synthetic_font;

use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::process::Command;

use ascii_convert::{GlyphConfig, build_glyph_table};
use ascii_core::{Budget, CancellationToken, Error};
use asciiwall_cli::font::{FontLoadError, load_glyph_table};

#[test]
fn explicit_font_path_builds_the_same_measured_table_as_the_engine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("font with spaces.ttf");
    let font = synthetic_font::synthetic_font(false);
    fs::write(&path, &font).unwrap();
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let actual = load_glyph_table(&path, " AB", &geometry, &budget, &cancellation).unwrap();
    let expected = build_glyph_table(&font, " AB", &geometry, &budget, &cancellation).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn missing_explicit_font_is_an_error_not_a_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing-font.ttf");
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    assert!(matches!(
        load_glyph_table(&path, " AB", &geometry, &budget, &cancellation),
        Err(FontLoadError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound
    ));
}

#[test]
fn font_input_honors_budget_and_cancellation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("font.ttf");
    fs::write(&path, synthetic_font::synthetic_font(false)).unwrap();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let budget = Budget::new(8_000_000, 10, 16_000_000);
    let cancellation = CancellationToken::new();
    assert!(matches!(
        load_glyph_table(&path, " AB", &geometry, &budget, &cancellation),
        Err(FontLoadError::TooLarge { max_bytes: 10 })
    ));
    cancellation.cancel();
    assert!(matches!(
        load_glyph_table(&path, " AB", &geometry, &budget, &cancellation),
        Err(FontLoadError::Engine(Error::Cancelled))
    ));
}

#[test]
fn non_regular_font_paths_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    assert!(matches!(
        load_glyph_table(dir.path(), " AB", &geometry, &budget, &cancellation),
        Err(FontLoadError::NotRegularFile)
    ));
}

#[cfg(unix)]
#[test]
fn fifo_font_path_is_rejected_before_opening_without_a_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("font.fifo");
    let status = Command::new("mkfifo")
        .arg(&path)
        .status()
        .expect("mkfifo is available on Unix");
    assert!(status.success());

    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    assert!(matches!(
        load_glyph_table(&path, " AB", &geometry, &budget, &cancellation),
        Err(FontLoadError::NotRegularFile)
    ));
}

#[cfg(unix)]
#[test]
fn symlinks_to_regular_fonts_remain_supported() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("actual-font.ttf");
    let link = dir.path().join("font-link.ttf");
    let font = synthetic_font::synthetic_font(false);
    fs::write(&target, &font).unwrap();
    symlink(&target, &link).unwrap();

    let geometry = GlyphConfig::new(8.0, 8, 12, 9).unwrap();
    let budget = Budget::new(8_000_000, 64_000_000, 16_000_000);
    let cancellation = CancellationToken::new();
    let actual = load_glyph_table(&link, " AB", &geometry, &budget, &cancellation).unwrap();
    let expected = build_glyph_table(&font, " AB", &geometry, &budget, &cancellation).unwrap();
    assert_eq!(actual, expected);
}
