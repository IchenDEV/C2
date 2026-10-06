//! Credential-file and admin data-dir hardening for the CLI (unix file semantics).
#![cfg(unix)]

use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use codetwo_server::cli::config::read_token_file;
use codetwo_server::cli::{run_mcp_client, GlobalOpts, McpClientAction};

fn code(outcome: Result<String, codetwo_server::cli::CliOutcome>) -> String {
    outcome.unwrap_err().envelope.error.unwrap().code
}

#[test]
fn read_token_file_does_not_hang_on_fifo() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("fifo");
    assert!(std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap()
        .success());
    std::fs::set_permissions(&fifo, std::fs::Permissions::from_mode(0o600)).unwrap();

    // A blocking open() on a writer-less FIFO never returns; bound it with a channel.
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(code(read_token_file(&fifo)));
    });
    let got = rx
        .recv_timeout(Duration::from_secs(5))
        .expect("read_token_file hung on a FIFO");
    assert_eq!(got, "invalid_token");
}

#[test]
fn read_token_file_refuses_symlink_even_to_a_private_file() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::write(&real, "ctmcp_symlinktarget0123456789\n").unwrap();
    std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = dir.path().join("link");
    symlink(&real, &link).unwrap();
    assert_eq!(code(read_token_file(&link)), "insecure_token_file");
    assert!(read_token_file(&real).is_ok());
}

#[test]
fn admin_create_fails_closed_when_data_dir_is_unusable_and_makes_private_dirs() {
    let root = tempfile::tempdir().unwrap();

    // A regular file where the data directory should be: handled error, no credential created.
    let blocked: PathBuf = root.path().join("blocked");
    std::fs::write(&blocked, "x").unwrap();
    let opts = GlobalOpts {
        data_dir: Some(blocked.clone()),
        ..GlobalOpts::default()
    };
    let outcome = run_mcp_client(&opts, McpClientAction::List);
    assert_eq!(outcome.envelope.error.unwrap().code, "data_dir");

    // A missing data directory is created 0700.
    let fresh = root.path().join("fresh");
    let opts = GlobalOpts {
        data_dir: Some(fresh.clone()),
        ..GlobalOpts::default()
    };
    let outcome = run_mcp_client(&opts, McpClientAction::List);
    assert!(outcome.envelope.error.is_none());
    let mode = std::fs::metadata(&fresh).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}
