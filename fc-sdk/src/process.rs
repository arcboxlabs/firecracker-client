//! Process lifecycle management for Firecracker and Jailer.
//!
//! This module provides builders for spawning Firecracker processes directly
//! or via the Jailer, and a handle for managing the process lifecycle.
//!
//! # Direct Process
//!
//! ```no_run
//! use fc_sdk::process::FirecrackerProcessBuilder;
//! use fc_sdk::VmId;
//!
//! # async fn example() -> fc_sdk::Result<()> {
//! let process = FirecrackerProcessBuilder::new("firecracker", "/tmp/firecracker.sock")
//!     .id(VmId::new("my-vm")?)
//!     .console_path("/tmp/firecracker-console.log")
//!     .spawn()
//!     .await?;
//!
//! let vm = process.vm_builder()
//!     .boot_source(fc_sdk::types::BootSource {
//!         kernel_image_path: "/path/to/vmlinux".into(),
//!         boot_args: Some("console=ttyS0".into()),
//!         initrd_path: None,
//!     })
//!     .machine_config(fc_sdk::types::MachineConfiguration {
//!         vcpu_count: std::num::NonZeroU64::new(2).unwrap(),
//!         mem_size_mib: 256,
//!         smt: false,
//!         track_dirty_pages: false,
//!         cpu_template: None,
//!         huge_pages: None,
//!     })
//!     .start()
//!     .await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Jailer
//!
//! ```no_run
//! use fc_sdk::process::JailerProcessBuilder;
//! use fc_sdk::VmId;
//!
//! # async fn example() -> fc_sdk::Result<()> {
//! let process = JailerProcessBuilder::new(
//!     "jailer",
//!     "/usr/bin/firecracker",
//!     VmId::new("my-vm")?,
//!     1000,
//!     1000,
//! )
//! .spawn()
//! .await?;
//!
//! let vm = process.vm_builder()
//!     .boot_source(fc_sdk::types::BootSource {
//!         kernel_image_path: "/path/to/vmlinux".into(),
//!         boot_args: Some("console=ttyS0".into()),
//!         initrd_path: None,
//!     })
//!     .machine_config(fc_sdk::types::MachineConfiguration {
//!         vcpu_count: std::num::NonZeroU64::new(2).unwrap(),
//!         mem_size_mib: 256,
//!         smt: false,
//!         track_dirty_pages: false,
//!         cpu_template: None,
//!         huge_pages: None,
//!     })
//!     .start()
//!     .await?;
//! # Ok(())
//! # }
//! ```

use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::time::{sleep, timeout as tokio_timeout};

use crate::builder::VmBuilder;
use crate::error::{Error, Result};
use crate::vm_id::VmId;

// =============================================================================
// Socket Polling
// =============================================================================

async fn wait_for_socket(
    path: &Path,
    timeout_duration: Duration,
    poll_interval: Duration,
) -> Result<()> {
    let path = path.to_owned();
    tokio_timeout(timeout_duration, async {
        loop {
            if path.exists() && tokio::net::UnixStream::connect(&path).await.is_ok() {
                return Ok(());
            }
            sleep(poll_interval).await;
        }
    })
    .await
    .map_err(|_| Error::SocketTimeout(path))?
}

// =============================================================================
// FirecrackerProcessBuilder
// =============================================================================

/// Builder for spawning a Firecracker process directly.
pub struct FirecrackerProcessBuilder {
    firecracker_bin: PathBuf,
    socket_path: PathBuf,
    id: Option<VmId>,
    seccomp_filter: Option<PathBuf>,
    no_seccomp: bool,
    boot_timer: bool,
    log_path: Option<PathBuf>,
    log_level: Option<String>,
    show_level: Option<bool>,
    show_log_origin: Option<bool>,
    metrics_path: Option<PathBuf>,
    http_api_max_payload_size: Option<usize>,
    mmds_size_limit: Option<usize>,
    enable_pci: Option<bool>,
    socket_timeout: Duration,
    socket_poll_interval: Duration,
    cleanup_socket: bool,
    console_path: Option<PathBuf>,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
}

impl FirecrackerProcessBuilder {
    /// Create a new builder for spawning Firecracker.
    pub fn new(firecracker_bin: impl Into<PathBuf>, socket_path: impl Into<PathBuf>) -> Self {
        Self {
            firecracker_bin: firecracker_bin.into(),
            socket_path: socket_path.into(),
            id: None,
            seccomp_filter: None,
            no_seccomp: false,
            boot_timer: false,
            log_path: None,
            log_level: None,
            show_level: None,
            show_log_origin: None,
            metrics_path: None,
            http_api_max_payload_size: None,
            mmds_size_limit: None,
            enable_pci: None,
            socket_timeout: Duration::from_secs(5),
            socket_poll_interval: Duration::from_millis(50),
            cleanup_socket: true,
            console_path: None,
            stdin: None,
            stdout: None,
            stderr: None,
        }
    }

    /// Set the VM identifier.
    ///
    /// Takes a pre-validated [`VmId`] — build one via [`VmId::new`] (strict)
    /// or [`VmId::from_sanitized`] (infallible projection).
    pub fn id(mut self, id: VmId) -> Self {
        self.id = Some(id);
        self
    }

    /// Set the path to the seccomp filter.
    pub fn seccomp_filter(mut self, path: impl Into<PathBuf>) -> Self {
        self.seccomp_filter = Some(path.into());
        self
    }

    /// Disable seccomp filtering.
    pub fn no_seccomp(mut self, no_seccomp: bool) -> Self {
        self.no_seccomp = no_seccomp;
        self
    }

    /// Enable the boot timer.
    pub fn boot_timer(mut self, boot_timer: bool) -> Self {
        self.boot_timer = boot_timer;
        self
    }

    /// Set the log output path.
    pub fn log_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.log_path = Some(path.into());
        self
    }

    /// Set the log level.
    pub fn log_level(mut self, level: impl Into<String>) -> Self {
        self.log_level = Some(level.into());
        self
    }

    /// Show the log level in output.
    pub fn show_level(mut self, show: bool) -> Self {
        self.show_level = Some(show);
        self
    }

    /// Show the log origin in output.
    pub fn show_log_origin(mut self, show: bool) -> Self {
        self.show_log_origin = Some(show);
        self
    }

    /// Set the metrics output path.
    pub fn metrics_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.metrics_path = Some(path.into());
        self
    }

    /// Set the maximum payload size for the HTTP API.
    pub fn http_api_max_payload_size(mut self, size: usize) -> Self {
        self.http_api_max_payload_size = Some(size);
        self
    }

    /// Set the MMDS data store size limit.
    pub fn mmds_size_limit(mut self, size: usize) -> Self {
        self.mmds_size_limit = Some(size);
        self
    }

    /// Enable PCI support.
    pub fn enable_pci(mut self, enable: bool) -> Self {
        self.enable_pci = Some(enable);
        self
    }

    /// Set the timeout for waiting for the socket to become available.
    pub fn socket_timeout(mut self, timeout: Duration) -> Self {
        self.socket_timeout = timeout;
        self
    }

    /// Set the polling interval when waiting for the socket.
    pub fn socket_poll_interval(mut self, interval: Duration) -> Self {
        self.socket_poll_interval = interval;
        self
    }

    /// Whether to clean up an existing socket file before spawning.
    pub fn cleanup_socket(mut self, cleanup: bool) -> Self {
        self.cleanup_socket = cleanup;
        self
    }

    /// Route the guest serial console (firecracker's stdout) and any stderr
    /// diagnostics to `path`.
    ///
    /// Firecracker pipes the guest ttyS0 to its own stdout, so when the
    /// kernel cmdline includes `console=ttyS0` the boot log and any later
    /// serial output land wherever the spawned firecracker process's stdout
    /// goes. This file is opened in create+append mode at [`spawn`] time.
    ///
    /// Explicit [`stdout`] / [`stderr`] overrides take precedence per-channel;
    /// [`stdin`] is left at the process default regardless.
    ///
    /// [`spawn`]: Self::spawn
    /// [`stdin`]: Self::stdin
    /// [`stdout`]: Self::stdout
    /// [`stderr`]: Self::stderr
    pub fn console_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.console_path = Some(path.into());
        self
    }

    /// Override stdin for the spawned Firecracker process.
    ///
    /// Passed through verbatim to [`tokio::process::Command::stdin`].
    pub fn stdin(mut self, stdio: Stdio) -> Self {
        self.stdin = Some(stdio);
        self
    }

    /// Override stdout for the spawned Firecracker process.
    ///
    /// Passed through verbatim to [`tokio::process::Command::stdout`]. When
    /// set, this takes precedence over [`console_path`] for the stdout
    /// channel.
    ///
    /// [`console_path`]: Self::console_path
    pub fn stdout(mut self, stdio: Stdio) -> Self {
        self.stdout = Some(stdio);
        self
    }

    /// Override stderr for the spawned Firecracker process.
    ///
    /// Passed through verbatim to [`tokio::process::Command::stderr`]. When
    /// set, this takes precedence over [`console_path`] for the stderr
    /// channel.
    ///
    /// [`console_path`]: Self::console_path
    pub fn stderr(mut self, stdio: Stdio) -> Self {
        self.stderr = Some(stdio);
        self
    }

    /// Build the command-line arguments for the Firecracker process.
    fn build_args(&self) -> Vec<String> {
        let mut args = vec![
            "--api-sock".to_owned(),
            self.socket_path.display().to_string(),
        ];

        if let Some(id) = &self.id {
            args.push("--id".to_owned());
            args.push(id.as_str().to_owned());
        }

        if let Some(filter) = &self.seccomp_filter {
            args.push("--seccomp-filter".to_owned());
            args.push(filter.display().to_string());
        }

        if self.no_seccomp {
            args.push("--no-seccomp".to_owned());
        }

        if self.boot_timer {
            args.push("--boot-timer".to_owned());
        }

        if let Some(path) = &self.log_path {
            args.push("--log-path".to_owned());
            args.push(path.display().to_string());
        }

        if let Some(level) = &self.log_level {
            args.push("--level".to_owned());
            args.push(level.clone());
        }

        if self.show_level == Some(true) {
            args.push("--show-level".to_owned());
        }

        if self.show_log_origin == Some(true) {
            args.push("--show-log-origin".to_owned());
        }

        if let Some(path) = &self.metrics_path {
            args.push("--metrics-path".to_owned());
            args.push(path.display().to_string());
        }

        if let Some(size) = self.http_api_max_payload_size {
            args.push("--http-api-max-payload-size".to_owned());
            args.push(size.to_string());
        }

        if let Some(size) = self.mmds_size_limit {
            args.push("--mmds-size-limit".to_owned());
            args.push(size.to_string());
        }

        if self.enable_pci == Some(true) {
            args.push("--enable-pci".to_owned());
        }

        args
    }

    /// Spawn the Firecracker process and wait for the socket to become available.
    pub async fn spawn(mut self) -> Result<FirecrackerProcess> {
        if self.cleanup_socket && self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path).ok();
        }

        let args = self.build_args();
        let mut command = Command::new(&self.firecracker_bin);
        command.args(args);
        apply_stdio(
            &mut command,
            self.stdin.take(),
            self.stdout.take(),
            self.stderr.take(),
            self.console_path.as_deref(),
        )?;

        let child = command.spawn().map_err(Error::SpawnFailed)?;

        let pid = child.id();
        let socket_path = self.socket_path.clone();

        let mut process = FirecrackerProcess {
            child: Some(child),
            pid,
            socket_path,
            cleanup_socket_on_drop: true,
        };

        if let Err(e) = wait_for_socket(
            &self.socket_path,
            self.socket_timeout,
            self.socket_poll_interval,
        )
        .await
        {
            // If socket wait failed, check if process exited
            if let Some(child) = &mut process.child
                && let Ok(Some(status)) = child.try_wait()
            {
                return Err(Error::ProcessExited(Some(status)));
            }
            return Err(e);
        }

        Ok(process)
    }
}

/// Wire optional stdio overrides and `console_path` into a [`Command`].
///
/// Resolution per channel: an explicit override (`stdin` / `stdout` /
/// `stderr`) wins; otherwise `console_path` fills stdout + stderr from the
/// same file (opened in create+append mode, cloned for the second channel);
/// otherwise the channel is left at `Command`'s default (inherit from the
/// parent process).
fn apply_stdio(
    command: &mut Command,
    stdin: Option<Stdio>,
    stdout: Option<Stdio>,
    stderr: Option<Stdio>,
    console_path: Option<&Path>,
) -> Result<()> {
    if let Some(s) = stdin {
        command.stdin(s);
    }

    let open_console = || -> std::io::Result<std::fs::File> {
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(console_path.expect("called only when console_path is set"))
    };

    match (stdout, stderr, console_path) {
        (Some(out), Some(err), _) => {
            command.stdout(out);
            command.stderr(err);
        }
        (Some(out), None, Some(_)) => {
            command.stdout(out);
            command.stderr(Stdio::from(open_console()?));
        }
        (Some(out), None, None) => {
            command.stdout(out);
        }
        (None, Some(err), Some(_)) => {
            command.stdout(Stdio::from(open_console()?));
            command.stderr(err);
        }
        (None, Some(err), None) => {
            command.stderr(err);
        }
        (None, None, Some(_)) => {
            let file = open_console()?;
            command.stdout(Stdio::from(file.try_clone()?));
            command.stderr(Stdio::from(file));
        }
        (None, None, None) => {}
    }

    Ok(())
}

// =============================================================================
// JailerProcessBuilder
// =============================================================================

/// Builder for spawning a Firecracker process via the Jailer.
pub struct JailerProcessBuilder {
    jailer_bin: PathBuf,
    exec_file: PathBuf,
    id: VmId,
    uid: u32,
    gid: u32,
    chroot_base_dir: PathBuf,
    netns: Option<String>,
    daemonize: bool,
    new_pid_ns: bool,
    cgroups: Vec<String>,
    resource_limits: Vec<String>,
    cgroup_version: Option<String>,
    parent_cgroup: Option<String>,
    firecracker_args: Vec<String>,
    socket_timeout: Duration,
    socket_poll_interval: Duration,
}

impl JailerProcessBuilder {
    /// Create a new Jailer builder.
    ///
    /// `id` must be a pre-validated [`VmId`] — the jailer applies the same
    /// Firecracker `--id` rules and also uses the value as a chroot path
    /// component, so it has to satisfy the validator.
    pub fn new(
        jailer_bin: impl Into<PathBuf>,
        exec_file: impl Into<PathBuf>,
        id: VmId,
        uid: u32,
        gid: u32,
    ) -> Self {
        Self {
            jailer_bin: jailer_bin.into(),
            exec_file: exec_file.into(),
            id,
            uid,
            gid,
            chroot_base_dir: PathBuf::from("/srv/jailer"),
            netns: None,
            daemonize: false,
            new_pid_ns: false,
            cgroups: Vec::new(),
            resource_limits: Vec::new(),
            cgroup_version: None,
            parent_cgroup: None,
            firecracker_args: Vec::new(),
            socket_timeout: Duration::from_secs(5),
            socket_poll_interval: Duration::from_millis(50),
        }
    }

    /// Set the chroot base directory (default: `/srv/jailer`).
    pub fn chroot_base_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.chroot_base_dir = path.into();
        self
    }

    /// Set the network namespace.
    pub fn netns(mut self, netns: impl Into<String>) -> Self {
        self.netns = Some(netns.into());
        self
    }

    /// Enable daemonize mode.
    pub fn daemonize(mut self, daemonize: bool) -> Self {
        self.daemonize = daemonize;
        self
    }

    /// Enable new PID namespace.
    pub fn new_pid_ns(mut self, new_pid_ns: bool) -> Self {
        self.new_pid_ns = new_pid_ns;
        self
    }

    /// Add a cgroup setting (e.g., `"cpu.shares=100"`).
    pub fn cgroup(mut self, cgroup: impl Into<String>) -> Self {
        self.cgroups.push(cgroup.into());
        self
    }

    /// Add a resource limit (e.g., `"fsize=2048"`).
    pub fn resource_limit(mut self, limit: impl Into<String>) -> Self {
        self.resource_limits.push(limit.into());
        self
    }

    /// Set the cgroup version (`"1"` or `"2"`).
    pub fn cgroup_version(mut self, version: impl Into<String>) -> Self {
        self.cgroup_version = Some(version.into());
        self
    }

    /// Set the parent cgroup.
    pub fn parent_cgroup(mut self, parent: impl Into<String>) -> Self {
        self.parent_cgroup = Some(parent.into());
        self
    }

    /// Add extra arguments to pass to the Firecracker process.
    pub fn firecracker_arg(mut self, arg: impl Into<String>) -> Self {
        self.firecracker_args.push(arg.into());
        self
    }

    /// Set the timeout for waiting for the socket to become available.
    pub fn socket_timeout(mut self, timeout: Duration) -> Self {
        self.socket_timeout = timeout;
        self
    }

    /// Set the polling interval when waiting for the socket.
    pub fn socket_poll_interval(mut self, interval: Duration) -> Self {
        self.socket_poll_interval = interval;
        self
    }

    /// Compute the socket path inside the chroot.
    ///
    /// Returns `{chroot_base_dir}/{exec_name}/{id}/root/run/firecracker.socket`.
    pub fn socket_path(&self) -> PathBuf {
        let exec_name = self
            .exec_file
            .file_name()
            .expect("exec_file must have a filename")
            .to_string_lossy();
        self.chroot_base_dir
            .join(exec_name.as_ref())
            .join(self.id.as_str())
            .join("root")
            .join("run")
            .join("firecracker.socket")
    }

    /// Build the command-line arguments for the Jailer process.
    fn build_args(&self) -> Vec<String> {
        let mut args = vec![
            "--exec-file".to_owned(),
            self.exec_file.display().to_string(),
            "--id".to_owned(),
            self.id.as_str().to_owned(),
            "--uid".to_owned(),
            self.uid.to_string(),
            "--gid".to_owned(),
            self.gid.to_string(),
        ];

        if self.chroot_base_dir != Path::new("/srv/jailer") {
            args.push("--chroot-base-dir".to_owned());
            args.push(self.chroot_base_dir.display().to_string());
        }

        if let Some(netns) = &self.netns {
            args.push("--netns".to_owned());
            args.push(netns.clone());
        }

        if self.daemonize {
            args.push("--daemonize".to_owned());
        }

        if self.new_pid_ns {
            args.push("--new-pid-ns".to_owned());
        }

        for cg in &self.cgroups {
            args.push("--cgroup".to_owned());
            args.push(cg.clone());
        }

        for limit in &self.resource_limits {
            args.push("--resource-limit".to_owned());
            args.push(limit.clone());
        }

        if let Some(version) = &self.cgroup_version {
            args.push("--cgroup-version".to_owned());
            args.push(version.clone());
        }

        if let Some(parent) = &self.parent_cgroup {
            args.push("--parent-cgroup".to_owned());
            args.push(parent.clone());
        }

        // Append Firecracker args after "--"
        if !self.firecracker_args.is_empty() {
            args.push("--".to_owned());
            args.extend(self.firecracker_args.iter().cloned());
        }

        args
    }

    /// Spawn the Jailer process and wait for the Firecracker socket to become available.
    // TODO: mirror FirecrackerProcessBuilder's `console_path` / `stdin` / `stdout`
    // / `stderr` knobs once a caller actually needs to capture stdio through the
    // jailer chroot. Jailer's optional `--daemonize` detaches stdio, so the
    // semantics need a bit more care than the direct case.
    pub async fn spawn(self) -> Result<FirecrackerProcess> {
        let socket_path = self.socket_path();
        let socket_timeout = self.socket_timeout;
        let socket_poll_interval = self.socket_poll_interval;
        let daemonize = self.daemonize;

        let child = Command::new(&self.jailer_bin)
            .args(self.build_args())
            .spawn()
            .map_err(Error::SpawnFailed)?;

        let (child, pid) = if daemonize {
            // In daemonize mode, the jailer exits quickly after forking.
            // We don't hold a handle to the child Firecracker process.
            let mut child = child;
            let _ = child.wait().await;
            (None, None)
        } else {
            let pid = child.id();
            (Some(child), pid)
        };

        let process = FirecrackerProcess {
            child,
            pid,
            socket_path: socket_path.clone(),
            cleanup_socket_on_drop: !daemonize,
        };

        wait_for_socket(&socket_path, socket_timeout, socket_poll_interval).await?;

        Ok(process)
    }
}

// =============================================================================
// FirecrackerProcess
// =============================================================================

/// Handle to a running Firecracker process.
///
/// Returned by [`FirecrackerProcessBuilder::spawn()`] or [`JailerProcessBuilder::spawn()`].
/// Provides access to the socket path for building a [`VmBuilder`] and methods for
/// managing the process lifecycle.
pub struct FirecrackerProcess {
    child: Option<Child>,
    pid: Option<u32>,
    socket_path: PathBuf,
    cleanup_socket_on_drop: bool,
}

/// Metadata for a detached Firecracker process.
///
/// Returned by [`FirecrackerProcess::detach()`] when the process lifecycle is
/// intentionally transferred to the caller (e.g., CLI detached mode).
pub struct DetachedFirecrackerProcess {
    pid: Option<u32>,
    socket_path: PathBuf,
}

impl DetachedFirecrackerProcess {
    /// Best-effort PID if available.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Path to the Firecracker API socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl FirecrackerProcess {
    /// Best-effort PID if available.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Get the path to the Firecracker API socket.
    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    /// Create a [`VmBuilder`] connected to this process's socket.
    pub fn vm_builder(&self) -> VmBuilder {
        VmBuilder::new(&self.socket_path)
    }

    /// Create a low-level API client connected to this process's socket.
    pub fn client(&self) -> fc_api::Client {
        crate::connection::connect(&self.socket_path)
    }

    /// Gracefully shut down the Firecracker process (SIGTERM + wait).
    pub async fn shutdown(&mut self) -> Result<Option<std::process::ExitStatus>> {
        if let Some(ref mut child) = self.child {
            if let Some(pid) = self.pid {
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
            let status = child.wait().await?;
            self.child = None;
            self.pid = None;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Forcefully kill the Firecracker process (SIGKILL).
    pub async fn kill(&mut self) -> Result<Option<std::process::ExitStatus>> {
        if let Some(ref mut child) = self.child {
            child.kill().await?;
            let status = child.wait().await?;
            self.child = None;
            self.pid = None;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Wait for the Firecracker process to exit.
    pub async fn wait(&mut self) -> Result<Option<std::process::ExitStatus>> {
        if let Some(ref mut child) = self.child {
            let status = child.wait().await?;
            self.child = None;
            self.pid = None;
            Ok(Some(status))
        } else {
            Ok(None)
        }
    }

    /// Detach this handle without terminating the underlying process.
    ///
    /// After detaching, dropping this handle will not kill the process or
    /// remove the API socket path.
    pub fn detach(mut self) -> DetachedFirecrackerProcess {
        let detached = DetachedFirecrackerProcess {
            pid: self.pid,
            socket_path: self.socket_path.clone(),
        };
        self.child = None;
        self.pid = None;
        self.cleanup_socket_on_drop = false;
        detached
    }
}

impl Drop for FirecrackerProcess {
    fn drop(&mut self) {
        // Best-effort SIGKILL if the process is still running.
        if let Some(pid) = self.pid {
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
        }
        if self.cleanup_socket_on_drop {
            // Best-effort socket cleanup.
            std::fs::remove_file(&self.socket_path).ok();
        }
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn apply_stdio_routes_both_channels_to_console_path() {
        let temp = tempfile::TempDir::new().unwrap();
        let console = temp.path().join("console.log");

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf OUT; printf ERR >&2"]);
        apply_stdio(&mut cmd, None, None, None, Some(&console)).unwrap();

        let status = cmd.spawn().unwrap().wait().await.unwrap();
        assert!(status.success());

        let contents = std::fs::read_to_string(&console).unwrap();
        // stdout + stderr interleave is not ordering-stable, but both channels
        // must have reached the single file.
        assert!(
            contents.contains("OUT"),
            "missing stdout marker in {contents:?}"
        );
        assert!(
            contents.contains("ERR"),
            "missing stderr marker in {contents:?}"
        );
    }

    #[tokio::test]
    async fn apply_stdio_explicit_channel_overrides_console_path() {
        let temp = tempfile::TempDir::new().unwrap();
        let console = temp.path().join("console.log");

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf OUT; printf ERR >&2"]);
        // Send stdout to /dev/null; stderr still falls through to console_path.
        apply_stdio(&mut cmd, None, Some(Stdio::null()), None, Some(&console)).unwrap();

        let status = cmd.spawn().unwrap().wait().await.unwrap();
        assert!(status.success());

        let contents = std::fs::read_to_string(&console).unwrap();
        assert!(
            !contents.contains("OUT"),
            "stdout override leaked into console file: {contents:?}"
        );
        assert!(
            contents.contains("ERR"),
            "missing stderr marker in {contents:?}"
        );
    }

    #[tokio::test]
    async fn apply_stdio_appends_to_existing_console_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let console = temp.path().join("console.log");
        std::fs::write(&console, "prior\n").unwrap();

        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", "printf after"]);
        apply_stdio(&mut cmd, None, None, None, Some(&console)).unwrap();

        let status = cmd.spawn().unwrap().wait().await.unwrap();
        assert!(status.success());

        let contents = std::fs::read_to_string(&console).unwrap();
        assert!(
            contents.starts_with("prior\n"),
            "append mode clobbered file: {contents:?}"
        );
        assert!(
            contents.ends_with("after"),
            "append mode dropped new bytes: {contents:?}"
        );
    }

    #[test]
    fn apply_stdio_returns_io_error_when_console_parent_does_not_exist() {
        let mut cmd = Command::new("/bin/true");
        let missing = PathBuf::from("/tmp/fc-sdk-apply-stdio-nonexistent-parent/console.log");
        let err = apply_stdio(&mut cmd, None, None, None, Some(&missing))
            .expect_err("missing parent dir must surface as Io error");
        assert!(
            matches!(err, Error::Io(_)),
            "unexpected error variant: {err:?}"
        );
    }

    #[test]
    fn apply_stdio_noop_when_no_overrides_or_console_path() {
        // Smoke test: the no-config path must not error, must not touch
        // filesystem, and leaves the command free to inherit the parent's
        // stdio on spawn.
        let mut cmd = Command::new("/bin/true");
        apply_stdio(&mut cmd, None, None, None, None).unwrap();
    }

    #[test]
    fn test_firecracker_builder_args() {
        let builder = FirecrackerProcessBuilder::new("/usr/bin/firecracker", "/tmp/fc.sock")
            .id(VmId::new("test-vm").unwrap())
            .no_seccomp(true)
            .boot_timer(true)
            .log_path("/var/log/fc.log")
            .log_level("Debug")
            .show_level(true)
            .show_log_origin(true)
            .metrics_path("/var/metrics/fc.json")
            .http_api_max_payload_size(65536)
            .mmds_size_limit(204800)
            .enable_pci(true);

        let args = builder.build_args();
        assert_eq!(args[0], "--api-sock");
        assert_eq!(args[1], "/tmp/fc.sock");
        assert!(args.contains(&"--id".to_owned()));
        assert!(args.contains(&"test-vm".to_owned()));
        assert!(args.contains(&"--no-seccomp".to_owned()));
        assert!(args.contains(&"--boot-timer".to_owned()));
        assert!(args.contains(&"--log-path".to_owned()));
        assert!(args.contains(&"/var/log/fc.log".to_owned()));
        assert!(args.contains(&"--level".to_owned()));
        assert!(args.contains(&"Debug".to_owned()));
        assert!(args.contains(&"--show-level".to_owned()));
        assert!(args.contains(&"--show-log-origin".to_owned()));
        assert!(args.contains(&"--metrics-path".to_owned()));
        assert!(args.contains(&"/var/metrics/fc.json".to_owned()));
        assert!(args.contains(&"--http-api-max-payload-size".to_owned()));
        assert!(args.contains(&"65536".to_owned()));
        assert!(args.contains(&"--mmds-size-limit".to_owned()));
        assert!(args.contains(&"204800".to_owned()));
        assert!(args.contains(&"--enable-pci".to_owned()));
    }

    #[test]
    fn test_jailer_socket_path() {
        let builder = JailerProcessBuilder::new(
            "/usr/bin/jailer",
            "/usr/bin/firecracker",
            VmId::new("my-vm").unwrap(),
            1000,
            1000,
        );
        assert_eq!(
            builder.socket_path(),
            PathBuf::from("/srv/jailer/firecracker/my-vm/root/run/firecracker.socket")
        );
    }

    #[test]
    fn test_jailer_custom_chroot_base() {
        let builder = JailerProcessBuilder::new(
            "/usr/bin/jailer",
            "/usr/bin/firecracker",
            VmId::new("my-vm").unwrap(),
            1000,
            1000,
        )
        .chroot_base_dir("/tmp/jailer");
        assert_eq!(
            builder.socket_path(),
            PathBuf::from("/tmp/jailer/firecracker/my-vm/root/run/firecracker.socket")
        );
    }

    #[test]
    fn test_jailer_builder_args() {
        let builder = JailerProcessBuilder::new(
            "/usr/bin/jailer",
            "/usr/bin/firecracker",
            VmId::new("my-vm").unwrap(),
            1000,
            1000,
        )
        .netns("my-netns")
        .daemonize(true)
        .new_pid_ns(true)
        .cgroup("cpu.shares=100")
        .resource_limit("fsize=2048")
        .cgroup_version("2")
        .parent_cgroup("fc-parent")
        .firecracker_arg("--no-seccomp");

        let args = builder.build_args();
        assert!(args.contains(&"--exec-file".to_owned()));
        assert!(args.contains(&"/usr/bin/firecracker".to_owned()));
        assert!(args.contains(&"--id".to_owned()));
        assert!(args.contains(&"my-vm".to_owned()));
        assert!(args.contains(&"--uid".to_owned()));
        assert!(args.contains(&"1000".to_owned()));
        assert!(args.contains(&"--gid".to_owned()));
        assert!(args.contains(&"--netns".to_owned()));
        assert!(args.contains(&"my-netns".to_owned()));
        assert!(args.contains(&"--daemonize".to_owned()));
        assert!(args.contains(&"--new-pid-ns".to_owned()));
        assert!(args.contains(&"--cgroup".to_owned()));
        assert!(args.contains(&"cpu.shares=100".to_owned()));
        assert!(args.contains(&"--resource-limit".to_owned()));
        assert!(args.contains(&"fsize=2048".to_owned()));
        assert!(args.contains(&"--cgroup-version".to_owned()));
        assert!(args.contains(&"2".to_owned()));
        assert!(args.contains(&"--parent-cgroup".to_owned()));
        assert!(args.contains(&"fc-parent".to_owned()));
        // Firecracker args come after "--"
        let separator_pos = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[separator_pos + 1], "--no-seccomp");
    }
}
