use std::fs;
use std::io::{self, Write};

use ascii_core::CancellationToken;
use asciiwall_cli::output::{PublishError, publish_file};

#[test]
fn successful_publication_respects_overwrite() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("artwork with spaces.txt");
    let token = CancellationToken::new();
    publish_file(&destination, false, &token, |file| {
        file.write_all(b"first\n")
    })
    .unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"first\n");

    let error = publish_file(&destination, false, &token, |file| {
        file.write_all(b"second\n")
    })
    .unwrap_err();
    assert!(
        matches!(error, PublishError::Io(error) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&destination).unwrap(), b"first\n");

    publish_file(&destination, true, &token, |file| {
        file.write_all(b"second\n")
    })
    .unwrap();
    assert_eq!(fs::read(&destination).unwrap(), b"second\n");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn failed_write_and_cancellation_preserve_original() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("output.txt");
    fs::write(&destination, b"original").unwrap();
    let token = CancellationToken::new();
    let error = publish_file(&destination, true, &token, |file| {
        file.write_all(b"partial")?;
        Err(io::Error::other("simulated disk failure"))
    })
    .unwrap_err();
    assert!(matches!(error, PublishError::Io(_)));
    assert_eq!(fs::read(&destination).unwrap(), b"original");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);

    let error = publish_file(&destination, true, &token, |file| {
        file.write_all(b"complete but cancelled")?;
        token.cancel();
        Ok(())
    })
    .unwrap_err();
    assert!(matches!(error, PublishError::Cancelled));
    assert_eq!(fs::read(&destination).unwrap(), b"original");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_creation_cannot_be_overwritten_without_permission() {
    let dir = tempfile::tempdir().unwrap();
    let destination = dir.path().join("new.txt");
    let token = CancellationToken::new();
    let error = publish_file(&destination, false, &token, |file| {
        file.write_all(b"our output")?;
        fs::write(&destination, b"racing output")?;
        Ok(())
    })
    .unwrap_err();
    assert!(
        matches!(error, PublishError::Io(error) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&destination).unwrap(), b"racing output");
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}
