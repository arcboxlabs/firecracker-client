use std::num::NonZeroU64;
use std::os::unix::fs::chown;
use std::path::{Path, PathBuf};
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};
use firecracker::runtime::bundled::{BundledMode, BundledRuntimeOptions};
use firecracker::sdk::{
    FirecrackerProcess, FirecrackerProcessBuilder, JailerProcessBuilder, VmId, types,
};

#[derive(Debug, Parser)]
#[command(
    name = "fc-cli",
    version,
    about = "CLI utilities for Firecracker SDK runtime operations"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Resolve firecracker/jailer binaries using bundled runtime rules.
    Resolve(ResolveArgs),
    /// Spawn Firecracker and start a microVM.
    Start(Box<StartArgs>),
    /// Print current platform and whether release-based bundled mode supports it.
    Platform,
}

#[derive(Debug, Clone, Args)]
struct RuntimeArgs {
    /// Binary resolution mode.
    #[arg(long, value_enum, default_value_t = ResolveMode::BundledThenSystem)]
    mode: ResolveMode,

    /// Root directory containing bundled artifacts.
    #[arg(long)]
    bundle_root: Option<PathBuf>,

    /// Firecracker release version (e.g., v1.12.1).
    #[arg(long)]
    release: Option<String>,

    /// Expected SHA256 for firecracker binary.
    #[arg(long)]
    firecracker_sha256: Option<String>,

    /// Expected SHA256 for jailer binary.
    #[arg(long)]
    jailer_sha256: Option<String>,
}

#[derive(Debug, Clone, Args)]
struct ResolveArgs {
    /// Which binary path to resolve.
    #[arg(value_enum, default_value_t = ResolveTarget::All)]
    target: ResolveTarget,

    #[command(flatten)]
    runtime: RuntimeArgs,
}

#[derive(Debug, Clone, Args)]
struct StartArgs {
    #[command(flatten)]
    runtime: RuntimeArgs,

    /// Process backend.
    #[arg(long, value_enum, default_value_t = StartBackend::Firecracker)]
    backend: StartBackend,

    /// Firecracker binary path. If unset, resolved from bundled/runtime settings.
    #[arg(long)]
    firecracker_bin: Option<PathBuf>,

    /// Jailer binary path (used only when `--backend jailer`).
    #[arg(long)]
    jailer_bin: Option<PathBuf>,

    /// Firecracker API socket path (used only when `--backend firecracker`).
    #[arg(long, alias = "api-sock", default_value = "/tmp/firecracker.socket")]
    socket_path: PathBuf,

    /// Optional microVM identifier.
    ///
    /// For `--backend jailer`, this is the jailer `--id` and defaults to `fc-cli-vm`.
    #[arg(long)]
    id: Option<VmId>,

    /// Jailer UID (required when `--backend jailer`).
    #[arg(long)]
    uid: Option<u32>,

    /// Jailer GID (required when `--backend jailer`).
    #[arg(long)]
    gid: Option<u32>,

    /// Jailer chroot base dir (default from jailer is `/srv/jailer`).
    #[arg(long)]
    chroot_base_dir: Option<PathBuf>,

    /// Jailer netns path/name.
    #[arg(long)]
    netns: Option<String>,

    /// Run jailer in daemonize mode (requires `--detach`).
    #[arg(long, default_value_t = false)]
    daemonize: bool,

    /// Run jailer with a new PID namespace.
    #[arg(long, default_value_t = false)]
    new_pid_ns: bool,

    /// Jailer cgroup setting (repeatable), e.g. `cpu.shares=100`.
    #[arg(long = "cgroup")]
    cgroups: Vec<String>,

    /// Jailer resource limit (repeatable), e.g. `fsize=2048`.
    #[arg(long = "resource-limit")]
    resource_limits: Vec<String>,

    /// Jailer cgroup version (`1` or `2`).
    #[arg(long)]
    cgroup_version: Option<String>,

    /// Jailer parent cgroup.
    #[arg(long)]
    parent_cgroup: Option<String>,

    /// Linux kernel image path.
    #[arg(long)]
    kernel: PathBuf,

    /// Optional initrd path.
    #[arg(long)]
    initrd: Option<PathBuf>,

    /// Root filesystem path.
    #[arg(long)]
    rootfs: PathBuf,

    /// Root block device id.
    #[arg(long, default_value = "rootfs")]
    rootfs_id: String,

    /// Mark rootfs as read-only.
    #[arg(long, default_value_t = false)]
    rootfs_read_only: bool,

    /// Kernel boot arguments.
    #[arg(long)]
    boot_args: Option<String>,

    /// Number of vCPUs (must be > 0).
    #[arg(long, default_value_t = 1)]
    vcpu_count: u64,

    /// Guest memory size (MiB).
    #[arg(long, default_value_t = 256)]
    mem_size_mib: i64,

    /// Enable SMT.
    #[arg(long, default_value_t = false)]
    smt: bool,

    /// Enable dirty page tracking.
    #[arg(long, default_value_t = false)]
    track_dirty_pages: bool,

    /// Disable seccomp for Firecracker process.
    #[arg(long, default_value_t = false)]
    no_seccomp: bool,

    /// Firecracker log output path.
    #[arg(long)]
    log_path: Option<PathBuf>,

    /// Firecracker metrics output path.
    #[arg(long)]
    metrics_path: Option<PathBuf>,

    /// Firecracker log level.
    #[arg(long)]
    log_level: Option<String>,

    /// Socket readiness timeout (seconds).
    #[arg(long, default_value_t = 5)]
    socket_timeout_secs: u64,

    /// Socket poll interval (milliseconds).
    #[arg(long, default_value_t = 50)]
    socket_poll_interval_ms: u64,

    /// Detach after startup and leave microVM running.
    #[arg(long, default_value_t = false)]
    detach: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ResolveTarget {
    Firecracker,
    Jailer,
    All,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ResolveMode {
    BundledOnly,
    SystemOnly,
    BundledThenSystem,
    SystemThenBundled,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum StartBackend {
    Firecracker,
    Jailer,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Resolve(args) => resolve(args)?,
        Commands::Start(args) => start(*args).await?,
        Commands::Platform => platform(),
    }
    Ok(())
}

fn resolve(args: ResolveArgs) -> Result<(), Box<dyn std::error::Error>> {
    let opts = build_runtime_options(&args.runtime);

    match args.target {
        ResolveTarget::Firecracker => {
            let path = opts.resolve_firecracker_bin()?;
            println!("firecracker={}", path.display());
        }
        ResolveTarget::Jailer => {
            let path = opts.resolve_jailer_bin()?;
            println!("jailer={}", path.display());
        }
        ResolveTarget::All => {
            let firecracker = opts.resolve_firecracker_bin()?;
            let jailer = opts.resolve_jailer_bin()?;
            println!("firecracker={}", firecracker.display());
            println!("jailer={}", jailer.display());
        }
    }

    Ok(())
}

async fn start(args: StartArgs) -> Result<(), Box<dyn std::error::Error>> {
    let runtime_options = build_runtime_options(&args.runtime);

    let vcpu_count = NonZeroU64::new(args.vcpu_count)
        .ok_or_else(|| invalid_input("--vcpu-count must be greater than 0"))?;
    if args.mem_size_mib <= 0 {
        return Err(invalid_input("--mem-size-mib must be greater than 0").into());
    }
    if matches!(args.backend, StartBackend::Jailer)
        && args.socket_path != Path::new("/tmp/firecracker.socket")
    {
        return Err(
            invalid_input("`--socket-path` is only supported when --backend firecracker").into(),
        );
    }
    if matches!(args.backend, StartBackend::Jailer) && args.daemonize && !args.detach {
        return Err(invalid_input("`--backend jailer --daemonize` requires `--detach`").into());
    }

    let mut process = spawn_process(&args, &runtime_options).await?;

    // For jailer backend, stage resource files into the chroot and use
    // chroot-relative paths for the Firecracker API.
    let vm_paths = match args.backend {
        StartBackend::Jailer => {
            let chroot_root = chroot_root_from_socket(process.socket_path())?;
            stage_jailer_resources(&chroot_root, &args)?
        }
        StartBackend::Firecracker => VmPaths {
            kernel: args.kernel.clone(),
            rootfs: args.rootfs.clone(),
            initrd: args.initrd.clone(),
        },
    };

    configure_vm(&process, &args, vcpu_count, &vm_paths).await?;

    println!("vm_started=true");
    println!("backend={}", backend_as_str(args.backend));

    if args.detach {
        let detached = process.detach();
        println!("detached=true");
        println!("socket={}", detached.socket_path().display());
        if let Some(pid) = detached.pid() {
            println!("pid={pid}");
        }
        return Ok(());
    }

    println!("detached=false");
    println!("socket={}", process.socket_path().display());
    if let Some(pid) = process.pid() {
        println!("pid={pid}");
    }
    println!("waiting=true");
    println!("hint=press Ctrl+C to stop microVM");

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            let status = process.shutdown().await?;
            match status {
                Some(status) => println!("exit_status={status}"),
                None => println!("exit_status=unknown"),
            }
        }
        status = process.wait() => {
            let status = status?;
            match status {
                Some(status) => println!("exit_status={status}"),
                None => println!("exit_status=unknown"),
            }
        }
    }

    Ok(())
}

async fn spawn_process(
    args: &StartArgs,
    runtime_options: &BundledRuntimeOptions,
) -> Result<FirecrackerProcess, Box<dyn std::error::Error>> {
    match args.backend {
        StartBackend::Firecracker => {
            let firecracker_bin = match &args.firecracker_bin {
                Some(path) => path.clone(),
                None => runtime_options.resolve_firecracker_bin()?,
            };
            let mut builder = FirecrackerProcessBuilder::new(&firecracker_bin, &args.socket_path)
                .no_seccomp(args.no_seccomp)
                .socket_timeout(Duration::from_secs(args.socket_timeout_secs))
                .socket_poll_interval(Duration::from_millis(args.socket_poll_interval_ms));

            if let Some(id) = &args.id {
                builder = builder.id(id.clone());
            }
            if let Some(log_path) = &args.log_path {
                builder = builder.log_path(log_path.clone());
            }
            if let Some(metrics_path) = &args.metrics_path {
                builder = builder.metrics_path(metrics_path.clone());
            }
            if let Some(log_level) = &args.log_level {
                builder = builder.log_level(log_level.clone());
            }

            Ok(builder.spawn().await?)
        }
        StartBackend::Jailer => {
            let id = args
                .id
                .clone()
                .unwrap_or_else(|| VmId::from_sanitized("fc-cli-vm"));
            let uid = args
                .uid
                .ok_or_else(|| invalid_input("--uid is required when --backend jailer"))?;
            let gid = args
                .gid
                .ok_or_else(|| invalid_input("--gid is required when --backend jailer"))?;
            let firecracker_bin = match &args.firecracker_bin {
                Some(path) => path.clone(),
                None => runtime_options.resolve_firecracker_bin()?,
            };
            let jailer_bin = match &args.jailer_bin {
                Some(path) => path.clone(),
                None => runtime_options.resolve_jailer_bin()?,
            };

            let mut builder = JailerProcessBuilder::new(jailer_bin, firecracker_bin, id, uid, gid)
                .daemonize(args.daemonize)
                .new_pid_ns(args.new_pid_ns)
                .socket_timeout(Duration::from_secs(args.socket_timeout_secs))
                .socket_poll_interval(Duration::from_millis(args.socket_poll_interval_ms));

            if let Some(chroot_base_dir) = &args.chroot_base_dir {
                builder = builder.chroot_base_dir(chroot_base_dir.clone());
            }
            if let Some(netns) = &args.netns {
                builder = builder.netns(netns.clone());
            }
            if let Some(cgroup_version) = &args.cgroup_version {
                builder = builder.cgroup_version(cgroup_version.clone());
            }
            if let Some(parent_cgroup) = &args.parent_cgroup {
                builder = builder.parent_cgroup(parent_cgroup.clone());
            }
            for cgroup in &args.cgroups {
                builder = builder.cgroup(cgroup.clone());
            }
            for limit in &args.resource_limits {
                builder = builder.resource_limit(limit.clone());
            }

            if args.no_seccomp {
                builder = builder.firecracker_arg("--no-seccomp");
            }
            // log-path and metrics-path are resolved by Firecracker inside the
            // chroot, so use a fixed chroot-relative path and let the file be
            // created there.  The original host path is NOT accessible after
            // pivot_root.
            if args.log_path.is_some() {
                builder = builder
                    .firecracker_arg("--log-path")
                    .firecracker_arg("firecracker.log");
            }
            if args.metrics_path.is_some() {
                builder = builder
                    .firecracker_arg("--metrics-path")
                    .firecracker_arg("firecracker-metrics");
            }
            if let Some(log_level) = &args.log_level {
                builder = builder
                    .firecracker_arg("--level")
                    .firecracker_arg(log_level.clone());
            }

            Ok(builder.spawn().await?)
        }
    }
}

struct VmPaths {
    kernel: PathBuf,
    rootfs: PathBuf,
    initrd: Option<PathBuf>,
}

async fn configure_vm(
    process: &FirecrackerProcess,
    args: &StartArgs,
    vcpu_count: NonZeroU64,
    paths: &VmPaths,
) -> Result<(), Box<dyn std::error::Error>> {
    process
        .vm_builder()
        .boot_source(types::BootSource {
            kernel_image_path: path_to_string(&paths.kernel),
            boot_args: args.boot_args.clone(),
            initrd_path: paths.initrd.as_ref().map(|p| path_to_string(p)),
        })
        .machine_config(types::MachineConfiguration {
            vcpu_count,
            mem_size_mib: args.mem_size_mib,
            smt: args.smt,
            track_dirty_pages: args.track_dirty_pages,
            cpu_template: None,
            huge_pages: None,
        })
        .drive(types::Drive {
            drive_id: args.rootfs_id.clone(),
            path_on_host: Some(path_to_string(&paths.rootfs)),
            is_root_device: true,
            is_read_only: Some(args.rootfs_read_only),
            partuuid: None,
            cache_type: types::DriveCacheType::Unsafe,
            rate_limiter: None,
            io_engine: types::DriveIoEngine::Sync,
            socket: None,
        })
        .start()
        .await?;

    Ok(())
}

/// Derive the chroot root directory from the jailer socket path.
///
/// Socket path format: `{chroot_base}/{exec_name}/{id}/root/run/firecracker.socket`
/// We need:            `{chroot_base}/{exec_name}/{id}/root/`
fn chroot_root_from_socket(socket_path: &Path) -> Result<PathBuf, Box<dyn std::error::Error>> {
    // Strip run/firecracker.socket → .../root
    let root = socket_path
        .parent() // .../root/run
        .and_then(|p| p.parent()) // .../root
        .ok_or_else(|| invalid_input("cannot derive chroot root from socket path"))?;
    Ok(root.to_path_buf())
}

/// Copy a file into the chroot root directory and set ownership.
/// Returns the chroot-relative path (e.g. `/vmlinux`).
fn copy_to_chroot(
    chroot_root: &Path,
    source: &Path,
    uid: Option<u32>,
    gid: Option<u32>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let file_name = source
        .file_name()
        .ok_or_else(|| invalid_input(&format!("path has no filename: {}", source.display())))?;
    let dest = chroot_root.join(file_name);
    std::fs::copy(source, &dest).map_err(|e| {
        invalid_input(&format!(
            "failed to copy {} → {}: {e}",
            source.display(),
            dest.display()
        ))
    })?;
    chown(&dest, uid, gid)
        .map_err(|e| invalid_input(&format!("failed to chown {}: {e}", dest.display())))?;
    // Return the chroot-relative path (Firecracker sees / as chroot root).
    Ok(PathBuf::from("/").join(file_name))
}

/// Stage kernel, rootfs, and optionally initrd into the jailer chroot directory.
fn stage_jailer_resources(
    chroot_root: &Path,
    args: &StartArgs,
) -> Result<VmPaths, Box<dyn std::error::Error>> {
    let kernel = copy_to_chroot(chroot_root, &args.kernel, args.uid, args.gid)?;
    let rootfs = copy_to_chroot(chroot_root, &args.rootfs, args.uid, args.gid)?;
    let initrd = match &args.initrd {
        Some(initrd_path) => Some(copy_to_chroot(
            chroot_root,
            initrd_path,
            args.uid,
            args.gid,
        )?),
        None => None,
    };
    Ok(VmPaths {
        kernel,
        rootfs,
        initrd,
    })
}

fn platform() {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let supported = os == "linux" && (arch == "x86_64" || arch == "aarch64");

    println!("os={os}");
    println!("arch={arch}");
    println!("bundled_release_supported={supported}");
}

fn build_runtime_options(args: &RuntimeArgs) -> BundledRuntimeOptions {
    let mut opts = BundledRuntimeOptions::new().mode(to_bundled_mode(args.mode));

    if let Some(bundle_root) = &args.bundle_root {
        opts = opts.bundle_root(bundle_root.clone());
    }
    if let Some(release) = &args.release {
        opts = opts.release_version(release.clone());
    }
    if let Some(sha) = &args.firecracker_sha256 {
        opts = opts.firecracker_sha256(sha.clone());
    }
    if let Some(sha) = &args.jailer_sha256 {
        opts = opts.jailer_sha256(sha.clone());
    }

    opts
}

fn path_to_string(path: &Path) -> String {
    path.display().to_string()
}

fn backend_as_str(backend: StartBackend) -> &'static str {
    match backend {
        StartBackend::Firecracker => "firecracker",
        StartBackend::Jailer => "jailer",
    }
}

fn invalid_input(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.to_owned())
}

fn to_bundled_mode(mode: ResolveMode) -> BundledMode {
    match mode {
        ResolveMode::BundledOnly => BundledMode::BundledOnly,
        ResolveMode::SystemOnly => BundledMode::SystemOnly,
        ResolveMode::BundledThenSystem => BundledMode::BundledThenSystem,
        ResolveMode::SystemThenBundled => BundledMode::SystemThenBundled,
    }
}
