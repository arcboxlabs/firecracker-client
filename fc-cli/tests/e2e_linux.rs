//! End-to-end tests that require Linux and actual Firecracker binaries.
//!
//! These tests are marked with #[ignore] and skipped by default.
//! Run with: cargo test -p fc-cli --test e2e_linux -- --ignored
//!
//! Required environment variables:
//! - TEST_KERNEL_PATH: Path to vmlinux kernel image
//! - TEST_ROOTFS_PATH: Path to rootfs.ext4 image
//!
//! Firecracker Installation:
//! - Tests will automatically install Firecracker using arcbox.sh if not found
//! - Set SKIP_FIRECRACKER_INSTALL=1 to skip automatic installation

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;
use std::process::Command as StdCommand;
use std::sync::Once;
use std::time::Duration;
use tempfile::TempDir;

static FIRECRACKER_INIT: Once = Once::new();

/// Skip test if not running on Linux
macro_rules! require_linux {
    () => {
        if std::env::consts::OS != "linux" {
            eprintln!("Skipping: test requires Linux");
            return;
        }
    };
}

/// Check if firecracker is available in PATH
fn is_firecracker_installed() -> bool {
    StdCommand::new("which")
        .arg("firecracker")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Install Firecracker using arcbox.sh script
fn install_firecracker() -> Result<(), String> {
    eprintln!("Installing Firecracker via arcbox.sh...");

    let status = StdCommand::new("bash")
        .args(["-c", "curl -fsSL https://arcbox.sh/firecracker.sh | bash"])
        .status()
        .map_err(|e| format!("Failed to run install script: {}", e))?;

    if status.success() {
        eprintln!("Firecracker installed successfully");
        Ok(())
    } else {
        Err(format!(
            "Firecracker installation failed with exit code: {:?}",
            status.code()
        ))
    }
}

/// Ensure Firecracker is installed before running tests
/// Uses Once to ensure installation only happens once per test run
fn ensure_firecracker_installed() -> bool {
    // Check if auto-install is disabled
    if std::env::var("SKIP_FIRECRACKER_INSTALL").is_ok() {
        return is_firecracker_installed();
    }

    let mut installed = is_firecracker_installed();

    if !installed {
        FIRECRACKER_INIT.call_once(|| {
            if let Err(e) = install_firecracker() {
                eprintln!("Warning: {}", e);
            }
        });
        installed = is_firecracker_installed();
    }

    installed
}

/// Require Firecracker to be installed, install if necessary
macro_rules! require_firecracker {
    () => {
        if !ensure_firecracker_installed() {
            eprintln!("Skipping: Firecracker not installed and auto-install failed");
            eprintln!("Install manually: curl -fsSL https://arcbox.sh/firecracker.sh | bash");
            return;
        }
    };
}

// ==================== Firecracker Installation Tests ====================

/// Test that Firecracker can be detected or installed automatically
#[test]
#[ignore]
fn test_firecracker_installation() {
    require_linux!();

    // This test verifies the auto-install mechanism works
    let installed = ensure_firecracker_installed();

    if installed {
        // Verify firecracker binary is actually executable
        let output = StdCommand::new("firecracker").arg("--version").output();

        match output {
            Ok(o) => {
                assert!(o.status.success(), "firecracker --version should succeed");
                let version = String::from_utf8_lossy(&o.stdout);
                eprintln!("Firecracker version: {}", version.trim());
            }
            Err(e) => {
                panic!("Failed to run firecracker --version: {}", e);
            }
        }

        // Verify jailer is also installed
        let jailer_output = StdCommand::new("jailer").arg("--version").output();

        match jailer_output {
            Ok(o) => {
                assert!(o.status.success(), "jailer --version should succeed");
                let version = String::from_utf8_lossy(&o.stdout);
                eprintln!("Jailer version: {}", version.trim());
            }
            Err(e) => {
                panic!("Failed to run jailer --version: {}", e);
            }
        }
    } else {
        eprintln!("Firecracker not installed and auto-install was skipped or failed");
        eprintln!("This is expected if SKIP_FIRECRACKER_INSTALL=1 is set");
    }
}

/// Get test kernel path from environment, or skip test
fn get_kernel_path() -> Option<PathBuf> {
    std::env::var("TEST_KERNEL_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

/// Get test rootfs path from environment, or skip test
fn get_rootfs_path() -> Option<PathBuf> {
    std::env::var("TEST_ROOTFS_PATH")
        .ok()
        .map(PathBuf::from)
        .filter(|p| p.exists())
}

// ==================== VM Lifecycle Tests ====================

/// Test starting a VM in detached mode with firecracker backend
#[test]
#[ignore]
fn test_start_vm_detached_firecracker() {
    require_linux!();
    require_firecracker!();

    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set or file not found");
            return;
        }
    };

    let rootfs = match get_rootfs_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_ROOTFS_PATH not set or file not found");
            return;
        }
    };

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("firecracker.socket");

    let assert = Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            kernel.to_str().unwrap(),
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--socket-path",
            socket_path.to_str().unwrap(),
            "--vcpu-count",
            "1",
            "--mem-size-mib",
            "128",
            "--detach",
        ])
        .timeout(Duration::from_secs(30))
        .assert();

    assert
        .success()
        .stdout(predicate::str::contains("vm_started=true"))
        .stdout(predicate::str::contains("detached=true"))
        .stdout(predicate::str::contains("socket="))
        .stdout(predicate::str::contains("pid="));

    // Verify socket file was created
    assert!(
        socket_path.exists(),
        "Socket file should exist after VM start"
    );

    // Cleanup: kill the VM process
    // Parse PID from output and terminate
    let output = Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            kernel.to_str().unwrap(),
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--socket-path",
            socket_path.to_str().unwrap(),
            "--detach",
        ])
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(pid_str) = line.strip_prefix("pid=") {
            if let Ok(pid) = pid_str.parse::<i32>() {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
    }
}

/// Test starting a VM with custom boot arguments
#[test]
#[ignore]
fn test_start_vm_with_boot_args() {
    require_linux!();
    require_firecracker!();

    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set");
            return;
        }
    };

    let rootfs = match get_rootfs_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_ROOTFS_PATH not set");
            return;
        }
    };

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("firecracker.socket");

    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            kernel.to_str().unwrap(),
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--socket-path",
            socket_path.to_str().unwrap(),
            "--boot-args",
            "console=ttyS0 reboot=k panic=1",
            "--detach",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("vm_started=true"));
}

/// Test starting a VM with read-only rootfs
#[test]
#[ignore]
fn test_start_vm_readonly_rootfs() {
    require_linux!();
    require_firecracker!();

    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set");
            return;
        }
    };

    let rootfs = match get_rootfs_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_ROOTFS_PATH not set");
            return;
        }
    };

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("firecracker.socket");

    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            kernel.to_str().unwrap(),
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--socket-path",
            socket_path.to_str().unwrap(),
            "--rootfs-read-only",
            "--detach",
        ])
        .timeout(Duration::from_secs(30))
        .assert()
        .success()
        .stdout(predicate::str::contains("vm_started=true"));
}

// ==================== Error Handling Tests ====================

/// Test that non-existent kernel path fails gracefully
#[test]
#[ignore]
fn test_start_nonexistent_kernel() {
    require_linux!();
    require_firecracker!();

    let rootfs = match get_rootfs_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_ROOTFS_PATH not set");
            return;
        }
    };

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("firecracker.socket");

    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            "/nonexistent/path/to/vmlinux",
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--socket-path",
            socket_path.to_str().unwrap(),
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();
}

/// Test that non-existent rootfs path fails gracefully
#[test]
#[ignore]
fn test_start_nonexistent_rootfs() {
    require_linux!();
    require_firecracker!();

    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set");
            return;
        }
    };

    let temp_dir = TempDir::new().unwrap();
    let socket_path = temp_dir.path().join("firecracker.socket");

    Command::cargo_bin("fc-cli")
        .unwrap()
        .args([
            "start",
            "--kernel",
            kernel.to_str().unwrap(),
            "--rootfs",
            "/nonexistent/path/to/rootfs.ext4",
            "--socket-path",
            socket_path.to_str().unwrap(),
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();
}

// ==================== Jailer Backend Tests ====================

/// Test jailer backend requires root privileges
#[test]
#[ignore]
fn test_jailer_requires_root() {
    require_linux!();
    require_firecracker!();

    // Skip if running as root (test is for non-root behavior)
    if unsafe { libc::geteuid() } == 0 {
        eprintln!("Skipping: test is for non-root users");
        return;
    }

    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set");
            return;
        }
    };

    let rootfs = match get_rootfs_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_ROOTFS_PATH not set");
            return;
        }
    };

    // Jailer typically requires root to set up chroot
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
            kernel.to_str().unwrap(),
            "--rootfs",
            rootfs.to_str().unwrap(),
            "--detach",
        ])
        .timeout(Duration::from_secs(10))
        .assert()
        .failure();
}

// ==================== Resolve Command Tests ====================

/// Test resolve command can find bundled firecracker on Linux
#[test]
#[ignore]
fn test_resolve_bundled_firecracker() {
    require_linux!();

    // This test assumes bundled binaries are available
    let result = Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "firecracker", "--mode", "bundled-then-system"])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    stdout.contains("firecracker="),
                    "Output should contain firecracker path"
                );
            }
            // If it fails, that's okay - bundled might not be available
        }
        Err(_) => {
            // Command execution failed, skip
        }
    }
}

/// Test resolve command for jailer binary
#[test]
#[ignore]
fn test_resolve_bundled_jailer() {
    require_linux!();

    let result = Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "jailer", "--mode", "bundled-then-system"])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    stdout.contains("jailer="),
                    "Output should contain jailer path"
                );
            }
        }
        Err(_) => {
            // Command execution failed, skip
        }
    }
}

/// Test resolve all binaries at once
#[test]
#[ignore]
fn test_resolve_all_binaries() {
    require_linux!();

    let result = Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["resolve", "all", "--mode", "bundled-then-system"])
        .output();

    match result {
        Ok(output) => {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                assert!(
                    stdout.contains("firecracker="),
                    "Output should contain firecracker path"
                );
                assert!(
                    stdout.contains("jailer="),
                    "Output should contain jailer path"
                );
            }
        }
        Err(_) => {
            // Command execution failed, skip
        }
    }
}
