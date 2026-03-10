# fc-cli Testing Guide

This document describes the testing strategy and how to run tests for `fc-cli`.

## Test Structure

```
fc-cli/
├── src/
│   └── main.rs              # Unit tests (bottom of file)
└── tests/
    ├── cli_integration.rs   # Integration tests
    └── e2e_linux.rs         # End-to-end tests (Linux only)
```

## Test Categories

| Category | Location | Count | Requires |
|----------|----------|-------|----------|
| Unit Tests | `src/main.rs` | 15 | Nothing |
| Integration Tests | `tests/cli_integration.rs` | 20 | Nothing |
| E2E Tests | `tests/e2e_linux.rs` | 9 | Linux + Firecracker |

## Running Tests

### Run All Tests (Recommended)

```bash
cargo test -p fc-cli
```

This runs unit tests and integration tests, skipping E2E tests.

### Run Specific Test Categories

```bash
# Unit tests only
cargo test -p fc-cli --lib

# Integration tests only
cargo test -p fc-cli --test cli_integration

# E2E tests only (requires Linux + Firecracker)
cargo test -p fc-cli --test e2e_linux -- --ignored
```

### Run a Single Test

```bash
cargo test -p fc-cli test_parse_start_minimal_args
```

### Run Tests with Output

```bash
cargo test -p fc-cli -- --nocapture
```

## Unit Tests

Unit tests verify internal functions and argument parsing without external dependencies.

### Argument Parsing Tests

| Test | Description |
|------|-------------|
| `test_parse_start_minimal_args` | Verify minimal required arguments are parsed correctly |
| `test_parse_start_with_all_vm_options` | Verify all VM configuration options |
| `test_parse_jailer_backend_options` | Verify jailer backend specific options |
| `test_parse_resolve_command` | Verify resolve command parsing |
| `test_parse_platform_command` | Verify platform command parsing |

### Input Validation Tests

| Test | Description |
|------|-------------|
| `test_missing_required_kernel_arg` | Missing --kernel should fail |
| `test_missing_required_rootfs_arg` | Missing --rootfs should fail |
| `test_invalid_backend_value` | Invalid backend value should fail |

### Helper Function Tests

| Test | Description |
|------|-------------|
| `test_chroot_root_from_socket_valid` | Parse chroot root from valid socket path |
| `test_chroot_root_from_socket_minimal_path` | Parse chroot root from minimal path |
| `test_chroot_root_from_socket_too_short` | Too short path should fail |
| `test_path_to_string` | Path to string conversion |
| `test_backend_as_str` | Backend enum to string conversion |
| `test_to_bundled_mode` | ResolveMode to BundledMode conversion |

### Default Values Tests

| Test | Description |
|------|-------------|
| `test_default_values` | Verify all default values are set correctly |

## Integration Tests

Integration tests run the actual compiled binary and verify its behavior.

### Platform Command Tests

| Test | Description |
|------|-------------|
| `test_platform_command_output` | Output contains os=, arch=, bundled_release_supported= |
| `test_platform_command_format` | Output is in key=value format |

### Start Command Validation Tests

| Test | Description |
|------|-------------|
| `test_start_missing_kernel_error` | Missing --kernel shows helpful error |
| `test_start_missing_rootfs_error` | Missing --rootfs shows helpful error |
| `test_start_jailer_missing_uid_error` | Jailer without --uid fails |
| `test_start_jailer_missing_gid_error` | Jailer without --gid fails |
| `test_start_vcpu_count_zero_error` | --vcpu-count=0 fails |
| `test_start_mem_size_zero_error` | --mem-size-mib=0 fails |
| `test_start_jailer_daemonize_without_detach_error` | --daemonize requires --detach |
| `test_start_socket_path_with_jailer_error` | --socket-path not allowed with jailer |

### Resolve Command Tests

| Test | Description |
|------|-------------|
| `test_resolve_system_only_not_found` | system-only fails when binary not in PATH |
| `test_resolve_bundled_only_without_bundle_root` | bundled-only without bundle root fails |

### Help and Version Tests

| Test | Description |
|------|-------------|
| `test_help_output` | --help shows usage information |
| `test_version_output` | --version shows version |
| `test_start_help` | start --help shows all options |
| `test_resolve_help` | resolve --help shows options |

### Invalid Input Tests

| Test | Description |
|------|-------------|
| `test_invalid_subcommand` | Invalid subcommand fails |
| `test_invalid_backend` | Invalid backend value fails |
| `test_invalid_resolve_target` | Invalid resolve target fails |
| `test_negative_mem_size` | Negative mem-size-mib fails |

## E2E Tests

E2E tests require Linux and actual Firecracker binaries. They are marked with `#[ignore]` and skipped by default.

### Prerequisites

1. Linux operating system (x86_64 or aarch64)
2. Firecracker binary available (auto-installed if missing)
3. Valid kernel image (vmlinux)
4. Valid rootfs image (rootfs.ext4)

### Automatic Firecracker Installation

E2E tests will **automatically install Firecracker** using [arcbox.sh](https://arcbox.sh) if not found:

```bash
# This happens automatically when running E2E tests
curl -fsSL https://arcbox.sh/firecracker.sh | bash
```

To disable automatic installation:

```bash
export SKIP_FIRECRACKER_INSTALL=1
```

### Environment Variables

| Variable | Description |
|----------|-------------|
| `TEST_KERNEL_PATH` | Path to vmlinux kernel image |
| `TEST_ROOTFS_PATH` | Path to rootfs.ext4 image |
| `SKIP_FIRECRACKER_INSTALL` | Set to `1` to disable auto-install |

### Running E2E Tests

```bash
# Firecracker will be auto-installed if not found
export TEST_KERNEL_PATH=/path/to/vmlinux
export TEST_ROOTFS_PATH=/path/to/rootfs.ext4
cargo test -p fc-cli --test e2e_linux -- --ignored
```

### Getting Test Assets

You can download test kernel and rootfs from Firecracker's quickstart:

```bash
# Download kernel
ARCH=$(uname -m)
curl -fsSL -o /tmp/vmlinux \
  https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/${ARCH}/kernels/vmlinux.bin

# Download rootfs
curl -fsSL -o /tmp/rootfs.ext4 \
  https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/${ARCH}/rootfs/bionic.rootfs.ext4

# Run tests
export TEST_KERNEL_PATH=/tmp/vmlinux
export TEST_ROOTFS_PATH=/tmp/rootfs.ext4
cargo test -p fc-cli --test e2e_linux -- --ignored
```

### E2E Test Cases

| Test | Description |
|------|-------------|
| `test_start_vm_detached_firecracker` | Start VM in detached mode with firecracker backend |
| `test_start_vm_with_boot_args` | Start VM with custom boot arguments |
| `test_start_vm_readonly_rootfs` | Start VM with read-only rootfs |
| `test_start_nonexistent_kernel` | Non-existent kernel path fails gracefully |
| `test_start_nonexistent_rootfs` | Non-existent rootfs path fails gracefully |
| `test_jailer_requires_root` | Jailer backend requires root privileges |
| `test_resolve_bundled_firecracker` | Resolve bundled firecracker binary |
| `test_resolve_bundled_jailer` | Resolve bundled jailer binary |
| `test_resolve_all_binaries` | Resolve all binaries at once |

## CI Integration

Tests are automatically run in CI via GitHub Actions:

```yaml
# .github/workflows/ci.yml
- name: Test
  run: cargo test --workspace --all-features
```

### CI Test Matrix

| Stage | Tests Run | Environment |
|-------|-----------|-------------|
| Every push | Unit + Integration | ubuntu-latest |
| Every PR | Unit + Integration | ubuntu-latest |
| Release | Unit + Integration + E2E | ubuntu-latest with Firecracker |

## Adding New Tests

### Adding a Unit Test

Add to `src/main.rs` inside the `#[cfg(test)] mod tests` block:

```rust
#[test]
fn test_your_new_test() {
    // Your test code here
    assert!(true);
}
```

### Adding an Integration Test

Add to `tests/cli_integration.rs`:

```rust
#[test]
fn test_your_integration_test() {
    Command::cargo_bin("fc-cli")
        .unwrap()
        .args(["your", "args"])
        .assert()
        .success();
}
```

### Adding an E2E Test

Add to `tests/e2e_linux.rs`:

```rust
#[test]
#[ignore]  // Important: mark as ignored
fn test_your_e2e_test() {
    require_linux!();

    // Check for required resources
    let kernel = match get_kernel_path() {
        Some(p) => p,
        None => {
            eprintln!("Skipping: TEST_KERNEL_PATH not set");
            return;
        }
    };

    // Your test code here
}
```

## Test Dependencies

```toml
[dev-dependencies]
assert_cmd = "2"    # CLI testing utilities
predicates = "3"    # Assertion matchers
tempfile = "3"      # Temporary file/directory creation
libc = "0.2"        # Low-level system calls (E2E tests)
```
