use std::ffi::OsStr;
use std::fs;
#[cfg(unix)]
use std::path::Path;
use std::process::{Command, Output};

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

#[test]
fn help_describes_headless_convert_and_missing_integrations() {
    let output = run(["--help"]);
    assert_eq!(status(&output), 0);
    let help = String::from_utf8(output.stdout).unwrap();
    assert!(help.contains("convert"));
    assert!(help.contains("not yet available"));
    let output = run(["convert", "--help"]);
    assert_eq!(status(&output), 0);
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in ["--output", "--font", "--columns", "--matcher", "--charset", "--color", "--format", "--overwrite"] {
        assert!(help.contains(flag), "missing {flag}");
    }
}

#[test]
fn malformed_requests_return_usage_before_any_conversion() {
    for args in [
        vec!["convert", "input.png"],
        vec!["convert", "input.png", "--output", "image.txt", "--columns", "0"],
        vec!["convert", "input.png", "--output", "image.txt", "--matcher", "unknown"],
        vec!["convert", "input.png", "--output", "image.unknown"],
        vec!["convert", "input.png", "--output", "image.txt", "--format", "png"],
        vec!["--json", "convert", "input.png", "--output", "-"],
        vec!["convert", "input.png", "--output", "-", "--format", "png", "--json"],
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
fn existing_spaced_paths_and_format_conflicts_never_modify_output() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("existing artwork.txt");
    fs::write(&destination, b"previous output").unwrap();
    let input = dir.path().join("input image.png");

    let output = run([
        OsStr::new("convert"), input.as_os_str(), OsStr::new("--output"), destination.as_os_str(),
        OsStr::new("--charset"), OsStr::new("aa"),
    ]);
    assert_eq!(status(&output), 2);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
    let output = run([
        OsStr::new("convert"), input.as_os_str(), OsStr::new("--output"), destination.as_os_str(),
        OsStr::new("--format"), OsStr::new("png"),
    ]);
    assert_eq!(status(&output), 2);
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("output already exists"), "{message}");
    assert!(!message.contains("output format conflicts"), "{message}");
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");


    let output = run([
        OsStr::new("convert"), input.as_os_str(), OsStr::new("--output"), destination.as_os_str(),
        OsStr::new("--overwrite"), OsStr::new("--charset"), OsStr::new("aa"),
    ]);
    assert_eq!(status(&output), 3);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    let output = run([
        OsStr::new("convert"), input.as_os_str(), OsStr::new("--output"), destination.as_os_str(),
        OsStr::new("--overwrite"), OsStr::new("--format"), OsStr::new("png"),
    ]);
    assert_eq!(status(&output), 2);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");

    // The missing backend is a capability failure, never a successful or partial overwrite.
    let output = run([
        OsStr::new("convert"), input.as_os_str(), OsStr::new("--output"), destination.as_os_str(),
        OsStr::new("--font"), OsStr::new("font with spaces.ttf"), OsStr::new("--overwrite"),
    ]);
    assert_eq!(status(&output), 4);
    assert_eq!(fs::read(&destination).unwrap(), b"previous output");
}

#[test]
fn unavailable_matcher_color_and_configuration_are_explicit() {
    for (extra, expected) in [
        (vec!["--matcher", "structural"], "structural matching"),
        (vec!["--color", "source"], "source-color"),
        (vec!["--config", "settings.toml", "--offline"], "settings registry"),
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

#[test]
fn missing_export_integration_is_not_reported_as_success() {
    for format in ["txt", "ansi", "png"] {
        let output = run([
            "--offline", "--no-color", "convert", "source.png", "--font", "chosen.ttf",
            "--format", format, "--output", "-",
        ]);
        assert_eq!(status(&output), 4);
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.contains(&0x1b));
    }
}

#[test]
fn unsafe_glyphs_and_terminal_sequences_cannot_escape_diagnostics() {
    let output = run([
        "convert", "image.png", "--output", "-", "--charset", "a\u{1b}]52;c;c2VjcmV0\u{7}b",
    ]);
    assert_eq!(status(&output), 3);
    assert!(!output.stderr.contains(&0x1b));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("c2VjcmV0"));

    let output = run(["--json", "convert", "image.png", "--output", "-", "--bad-\u{1b}]0;owned\u{7}"]);
    assert_eq!(status(&output), 2);
    assert!(!output.stdout.contains(&0x1b));
    let body: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!body["error"]["message"].as_str().unwrap().contains('\u{1b}'));
}

#[test]
fn parser_diagnostics_do_not_echo_values_and_identify_the_parsed_command() {
    let private_path = "C:/Users/alice/private-input.png";
    let private_matcher = "super-secret-matcher-value";
    let output = run([
        "--json", "convert", private_path, "--output", "-", "--matcher", private_matcher,
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
    assert!(!body["error"]["message"].as_str().unwrap().contains("convert"));
}

#[test]
fn missing_font_blocks_conversion_without_disclosing_input_path() {
    let private_path = "C:/Users/alice/private-input.png";
    let output = run(["--json", "convert", private_path, "--output", "safe-output.txt"]);
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
        "--json", "convert", "input.png", "--output", "safe-output.txt",
        "--config", private_config,
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
    let output = run([OsStr::new("convert"), OsStr::new("input.png"), OsStr::new("--output"), output_path.as_os_str()]);
    assert_eq!(status(&output), 2);
    assert_eq!(fs::read_link(output_path).unwrap().as_path(), Path::new("missing-target"));
}
