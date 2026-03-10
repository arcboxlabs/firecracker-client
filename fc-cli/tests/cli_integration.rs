//! Integration tests for fc-cli binary.
//!
//! These tests run the actual compiled binary and verify its behavior,
//! including command output, error messages, and exit codes.

use assert_cmd::Command;
use predicates::prelude::*;

// ==================== Platform Command Tests ====================

/// Platform command should output os, arch, and bundled support info
#[test]
fn test_platform_command_output() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .arg("platform")
        .assert()
        .success()
        .stdout(predicate::str::contains("os="))
        .stdout(predicate::str::contains("arch="))
        .stdout(predicate::str::contains("bundled_release_supported="));
}

/// Platform command output should be in key=value format
#[test]
fn test_platform_command_format() {
    let output = Command::cargo_bin("fc-cli")
        .unwrap()
        .arg("platform")
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);

    for line in stdout.lines() {
        assert!(
            line.contains('='),
            "Each line should be key=value format, got: {}",
            line
        );
    }
}

// ==================== Start Command Validation Tests ====================

/// Start command without --kernel should fail with helpful error
#[test]
fn test_start_missing_kernel_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["start", "--rootfs", "/tmp/rootfs.ext4"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--kernel"));
}

/// Start command without --rootfs should fail with helpful error
#[test]
fn test_start_missing_rootfs_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["start", "--kernel", "/tmp/vmlinux"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--rootfs"));
}

/// Jailer backend without --uid should fail
#[test]
fn test_start_jailer_missing_uid_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--backend",
            "jailer",
            "--gid",
            "1000",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--uid"));
}

/// Jailer backend without --gid should fail
#[test]
fn test_start_jailer_missing_gid_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--backend",
            "jailer",
            "--uid",
            "1000",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--gid"));
}

/// --vcpu-count=0 should fail validation
#[test]
fn test_start_vcpu_count_zero_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
            "--vcpu-count",
            "0",
        ])
        .assert()
        .failure();
}

/// --mem-size-mib=0 should fail validation
#[test]
fn test_start_mem_size_zero_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
            "--mem-size-mib",
            "0",
        ])
        .assert()
        .failure();
}

/// --daemonize without --detach should fail for jailer backend
#[test]
fn test_start_jailer_daemonize_without_detach_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--backend",
            "jailer",
            "--uid",
            "1000",
            "--gid",
            "1000",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
            "--daemonize",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--detach"));
}

/// --socket-path is not allowed with jailer backend
#[test]
fn test_start_socket_path_with_jailer_error() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--backend",
            "jailer",
            "--uid",
            "1000",
            "--gid",
            "1000",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
            "--socket-path",
            "/custom/socket.sock",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("--socket-path"));
}

// ==================== Resolve Command Tests ====================

/// Resolve with system-only mode should fail when binary not in PATH
#[test]
fn test_resolve_system_only_not_found() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "firecracker", "--mode", "system-only"])
        .env("PATH", "/nonexistent")
        .assert()
        .failure();
}

/// Resolve with bundled-only mode without bundle root should fail
#[test]
fn test_resolve_bundled_only_without_bundle_root() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "firecracker", "--mode", "bundled-only"])
        .assert()
        .failure();
}

// ==================== Help and Version Tests ====================

/// --help should show usage information
#[test]
fn test_help_output() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("CLI utilities for Firecracker"));
}

/// --version should show version
#[test]
fn test_version_output() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("fc-cli"));
}

/// start --help should show all start options
#[test]
fn test_start_help() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--kernel"))
        .stdout(predicate::str::contains("--rootfs"))
        .stdout(predicate::str::contains("--vcpu-count"))
        .stdout(predicate::str::contains("--mem-size-mib"))
        .stdout(predicate::str::contains("--backend"));
}

/// resolve --help should show resolve options
#[test]
fn test_resolve_help() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("firecracker"))
        .stdout(predicate::str::contains("jailer"));
}

// ==================== Invalid Input Tests ====================

/// Invalid subcommand should fail
#[test]
fn test_invalid_subcommand() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .arg("invalid-command")
        .assert()
        .failure();
}

/// Invalid backend value should fail
#[test]
fn test_invalid_backend() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--backend",
            "invalid",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid"));
}

/// Invalid resolve target should fail
#[test]
fn test_invalid_resolve_target() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "invalid-target"])
        .assert()
        .failure();
}

/// Negative mem-size-mib should fail
#[test]
fn test_negative_mem_size() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            "/tmp/vmlinux",
            "--rootfs",
            "/tmp/rootfs.ext4",
            "--mem-size-mib",
            "-100",
        ])
        .assert()
        .failure();
}
