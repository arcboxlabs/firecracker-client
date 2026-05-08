use std::env;
use std::fmt;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use fc_sdk::{FirecrackerProcessBuilder, JailerProcessBuilder, VmId};
use sha2::{Digest, Sha256};

/// Errors from bundled runtime resolution.
#[derive(Debug)]
pub enum BundledRuntimeError {
    /// I/O error while reading/checking binaries.
    Io(std::io::Error),

    /// Bundled binary cannot be found.
    BinaryNotFound {
        binary: &'static str,
        searched: Vec<PathBuf>,
    },

    /// Bundled binary exists but is not executable.
    BinaryNotExecutable(PathBuf),

    /// Bundled SHA256 string format is invalid.
    InvalidSha256 {
        binary: &'static str,
        sha256: String,
    },

    /// Bundled binary checksum mismatched.
    ChecksumMismatch {
        binary: &'static str,
        path: PathBuf,
        expected: String,
        actual: String,
    },

    /// Unsupported platform for Firecracker release-based bundled mode.
    UnsupportedPlatform { os: String, arch: String },

    /// Invalid Firecracker release version.
    InvalidReleaseVersion(String),
}

impl std::error::Error for BundledRuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl fmt::Display for BundledRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
            Self::BinaryNotFound { binary, searched } => {
                write!(
                    f,
                    "bundled binary not found: {binary}; searched: {}",
                    searched
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
            Self::BinaryNotExecutable(path) => {
                write!(f, "bundled binary is not executable: {}", path.display())
            }
            Self::InvalidSha256 { binary, sha256 } => {
                write!(f, "invalid SHA256 for {binary}: {sha256}")
            }
            Self::ChecksumMismatch {
                binary,
                path,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "checksum mismatch for {binary} ({}): expected {expected}, got {actual}",
                    path.display()
                )
            }
            Self::UnsupportedPlatform { os, arch } => {
                write!(
                    f,
                    "unsupported platform for bundled release mode: {os}-{arch}; supported: linux-x86_64, linux-aarch64"
                )
            }
            Self::InvalidReleaseVersion(version) => {
                write!(
                    f,
                    "invalid Firecracker release version: {version}; expected vX.Y.Z"
                )
            }
        }
    }
}

impl From<std::io::Error> for BundledRuntimeError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

/// Result type for bundled runtime resolution.
pub type Result<T> = std::result::Result<T, BundledRuntimeError>;

/// Binary resolution mode for bundled runtime.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BundledMode {
    /// Use only bundled binaries.
    BundledOnly,
    /// Use only system binaries from `PATH`.
    SystemOnly,
    /// Try bundled binaries first, then fall back to system `PATH`.
    #[default]
    BundledThenSystem,
    /// Try system binaries first, then fall back to bundled binaries.
    SystemThenBundled,
}

/// Options for resolving Firecracker/Jailer binaries in bundled mode.
///
/// This mode is designed around Firecracker upstream release artifacts, which
/// currently provide Linux `x86_64` and Linux `aarch64` targets.
#[derive(Debug, Clone)]
pub struct BundledRuntimeOptions {
    mode: BundledMode,
    bundle_root: Option<PathBuf>,
    release_version: Option<String>,
    firecracker_bin_name: String,
    jailer_bin_name: String,
    ensure_executable: bool,
    firecracker_sha256: Option<String>,
    jailer_sha256: Option<String>,
}

impl Default for BundledRuntimeOptions {
    fn default() -> Self {
        Self {
            mode: BundledMode::BundledThenSystem,
            bundle_root: None,
            release_version: None,
            firecracker_bin_name: "firecracker".to_owned(),
            jailer_bin_name: "jailer".to_owned(),
            ensure_executable: true,
            firecracker_sha256: None,
            jailer_sha256: None,
        }
    }
}

impl BundledRuntimeOptions {
    /// Create options with default behavior (`BundledThenSystem`).
    pub fn new() -> Self {
        Self::default()
    }

    /// Set binary resolution mode.
    pub fn mode(mut self, mode: BundledMode) -> Self {
        self.mode = mode;
        self
    }

    /// Set root directory for bundled binaries.
    ///
    /// Layout supports:
    /// - Firecracker release extracted layout:
    ///   - `{root}/release-vX.Y.Z-{arch}/{binary}-vX.Y.Z-{arch}`
    /// - Firecracker release flattened layout:
    ///   - `{root}/{binary}-vX.Y.Z-{arch}`
    /// - Generic fallback layout:
    ///   - `{root}/{os}-{arch}/{binary}`
    ///   - `{root}/{os}-{arch}/bin/{binary}`
    ///   - `{root}/{arch}-{os}/{binary}`
    ///   - `{root}/{arch}-{os}/bin/{binary}`
    ///   - `{root}/{binary}`
    pub fn bundle_root(mut self, root: impl Into<PathBuf>) -> Self {
        self.bundle_root = Some(root.into());
        self
    }

    /// Set Firecracker release version (e.g., `v1.10.0`).
    ///
    /// When set, bundled lookup prioritizes upstream release naming.
    pub fn release_version(mut self, version: impl Into<String>) -> Self {
        self.release_version = Some(version.into());
        self
    }

    /// Override firecracker binary filename prefix (default: `firecracker`).
    pub fn firecracker_bin_name(mut self, name: impl Into<String>) -> Self {
        self.firecracker_bin_name = name.into();
        self
    }

    /// Override jailer binary filename prefix (default: `jailer`).
    pub fn jailer_bin_name(mut self, name: impl Into<String>) -> Self {
        self.jailer_bin_name = name.into();
        self
    }

    /// Whether to set executable bits for discovered binaries when needed.
    pub fn ensure_executable(mut self, ensure: bool) -> Self {
        self.ensure_executable = ensure;
        self
    }

    /// Optional expected SHA256 for firecracker binary.
    pub fn firecracker_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.firecracker_sha256 = Some(sha256.into());
        self
    }

    /// Optional expected SHA256 for jailer binary.
    pub fn jailer_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.jailer_sha256 = Some(sha256.into());
        self
    }

    /// Resolve path to firecracker binary.
    pub fn resolve_firecracker_bin(&self) -> Result<PathBuf> {
        self.resolve_binary(
            "firecracker",
            &self.firecracker_bin_name,
            "FC_SDK_FIRECRACKER_BIN",
            self.firecracker_sha256.as_deref(),
        )
    }

    /// Resolve path to jailer binary.
    pub fn resolve_jailer_bin(&self) -> Result<PathBuf> {
        self.resolve_binary(
            "jailer",
            &self.jailer_bin_name,
            "FC_SDK_JAILER_BIN",
            self.jailer_sha256.as_deref(),
        )
    }

    /// Build a [`FirecrackerProcessBuilder`] using bundled resolution.
    pub fn firecracker_builder(
        &self,
        socket_path: impl Into<PathBuf>,
    ) -> Result<FirecrackerProcessBuilder> {
        let firecracker_bin = self.resolve_firecracker_bin()?;
        Ok(FirecrackerProcessBuilder::new(firecracker_bin, socket_path))
    }

    /// Build a [`JailerProcessBuilder`] using bundled resolution.
    pub fn jailer_builder(&self, id: VmId, uid: u32, gid: u32) -> Result<JailerProcessBuilder> {
        let jailer_bin = self.resolve_jailer_bin()?;
        let firecracker_bin = self.resolve_firecracker_bin()?;
        Ok(JailerProcessBuilder::new(
            jailer_bin,
            firecracker_bin,
            id,
            uid,
            gid,
        ))
    }

    fn resolve_binary(
        &self,
        binary_label: &'static str,
        default_name: &str,
        env_override: &str,
        expected_sha256: Option<&str>,
    ) -> Result<PathBuf> {
        let mut searched = Vec::new();
        let bundled_enabled = matches!(
            self.mode,
            BundledMode::BundledOnly
                | BundledMode::BundledThenSystem
                | BundledMode::SystemThenBundled
        );

        let release_version = if bundled_enabled {
            self.resolve_release_version()?
        } else {
            None
        };
        let release_arch = if bundled_enabled {
            Some(current_release_arch()?)
        } else {
            None
        };

        if let Some(override_value) = env::var_os(env_override) {
            let override_path = PathBuf::from(override_value);
            let mut override_candidates = Vec::new();

            if looks_like_path(&override_path) {
                override_candidates.push(override_path);
            } else if let Some(name) = override_path.to_str() {
                override_candidates.extend(system_candidates(name));
                if bundled_enabled {
                    override_candidates.extend(bundled_candidates(
                        name,
                        &self.bundle_roots(),
                        release_version.as_deref(),
                        release_arch.as_deref(),
                    ));
                }
            }

            if let Some(path) = self.first_valid(
                binary_label,
                override_candidates,
                expected_sha256,
                &mut searched,
            )? {
                return Ok(path);
            }
        }

        let roots = self.bundle_roots();
        let mut mode_candidates = Vec::new();
        match self.mode {
            BundledMode::BundledOnly => {
                mode_candidates.extend(bundled_candidates(
                    default_name,
                    &roots,
                    release_version.as_deref(),
                    release_arch.as_deref(),
                ));
            }
            BundledMode::SystemOnly => {
                mode_candidates.extend(system_candidates(default_name));
            }
            BundledMode::BundledThenSystem => {
                mode_candidates.extend(bundled_candidates(
                    default_name,
                    &roots,
                    release_version.as_deref(),
                    release_arch.as_deref(),
                ));
                mode_candidates.extend(system_candidates(default_name));
            }
            BundledMode::SystemThenBundled => {
                mode_candidates.extend(system_candidates(default_name));
                mode_candidates.extend(bundled_candidates(
                    default_name,
                    &roots,
                    release_version.as_deref(),
                    release_arch.as_deref(),
                ));
            }
        }

        if let Some(path) = self.first_valid(
            binary_label,
            mode_candidates,
            expected_sha256,
            &mut searched,
        )? {
            return Ok(path);
        }

        Err(BundledRuntimeError::BinaryNotFound {
            binary: binary_label,
            searched,
        })
    }

    fn first_valid(
        &self,
        binary_label: &'static str,
        candidates: Vec<PathBuf>,
        expected_sha256: Option<&str>,
        searched: &mut Vec<PathBuf>,
    ) -> Result<Option<PathBuf>> {
        for candidate in dedupe_paths(candidates) {
            searched.push(candidate.clone());
            if !candidate.is_file() {
                continue;
            }

            if self.ensure_executable {
                ensure_executable(&candidate)?;
            }
            if !is_executable(&candidate)? {
                return Err(BundledRuntimeError::BinaryNotExecutable(candidate));
            }

            if let Some(expected) = expected_sha256 {
                verify_sha256(binary_label, &candidate, expected)?;
            }

            return Ok(Some(candidate));
        }
        Ok(None)
    }

    fn resolve_release_version(&self) -> Result<Option<String>> {
        let resolved = if let Some(version) = &self.release_version {
            Some(version.clone())
        } else {
            env::var("FC_SDK_FIRECRACKER_RELEASE").ok()
        };

        if let Some(version) = &resolved
            && !is_valid_release_version(version)
        {
            return Err(BundledRuntimeError::InvalidReleaseVersion(version.clone()));
        }

        Ok(resolved)
    }

    fn bundle_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();

        if let Some(root) = &self.bundle_root {
            roots.push(root.clone());
        }

        if let Some(root) = env::var_os("FC_SDK_BUNDLED_DIR") {
            roots.push(PathBuf::from(root));
        }

        if let Ok(current_exe) = env::current_exe()
            && let Some(exe_dir) = current_exe.parent()
        {
            roots.push(exe_dir.join("bundled"));
            roots.push(exe_dir.join("../bundled"));
        }

        roots.push(PathBuf::from("bundled"));
        dedupe_paths(roots)
    }
}

fn looks_like_path(path: &Path) -> bool {
    path.is_absolute()
        || path.components().count() > 1
        || path.to_string_lossy().contains(std::path::MAIN_SEPARATOR)
}

fn system_candidates(binary_name: &str) -> Vec<PathBuf> {
    let name_path = PathBuf::from(binary_name);
    if looks_like_path(&name_path) {
        return vec![name_path];
    }

    let mut paths = Vec::new();
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            paths.push(dir.join(binary_name));
        }
    }
    paths
}

fn bundled_candidates(
    binary_name: &str,
    roots: &[PathBuf],
    release_version: Option<&str>,
    release_arch: Option<&str>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let keys = target_keys();

    for root in roots {
        if let (Some(version), Some(arch)) = (release_version, release_arch) {
            let versioned_bin = format!("{binary_name}-{version}-{arch}");
            let release_dir = format!("release-{version}-{arch}");

            candidates.push(root.join(&release_dir).join(&versioned_bin));
            candidates.push(root.join(&release_dir).join("bin").join(&versioned_bin));
            candidates.push(root.join(&versioned_bin));
        }

        for key in &keys {
            candidates.push(root.join(key).join(binary_name));
            candidates.push(root.join(key).join("bin").join(binary_name));
        }
        candidates.push(root.join(binary_name));
    }

    candidates
}

fn target_keys() -> [String; 2] {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    [format!("{os}-{arch}"), format!("{arch}-{os}")]
}

fn is_supported_release_target(os: &str, arch: &str) -> bool {
    os == "linux" && matches!(arch, "x86_64" | "aarch64")
}

fn current_release_arch() -> Result<String> {
    let os = env::consts::OS;
    let arch = env::consts::ARCH;

    if is_supported_release_target(os, arch) {
        Ok(arch.to_owned())
    } else {
        Err(BundledRuntimeError::UnsupportedPlatform {
            os: os.to_owned(),
            arch: arch.to_owned(),
        })
    }
}

fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut unique = Vec::new();
    for path in paths {
        if !unique.iter().any(|p: &PathBuf| p == &path) {
            unique.push(path);
        }
    }
    unique
}

fn verify_sha256(binary_label: &'static str, path: &Path, expected: &str) -> Result<()> {
    let expected =
        normalize_sha256(expected).ok_or_else(|| BundledRuntimeError::InvalidSha256 {
            binary: binary_label,
            sha256: expected.to_owned(),
        })?;

    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = [0_u8; 8192];
    loop {
        let read = file.read(&mut buf)?;
        if read == 0 {
            break;
        }
        hasher.update(&buf[..read]);
    }
    let actual = format!("{:x}", hasher.finalize());

    if actual == expected {
        Ok(())
    } else {
        Err(BundledRuntimeError::ChecksumMismatch {
            binary: binary_label,
            path: path.to_path_buf(),
            expected,
            actual,
        })
    }
}

fn normalize_sha256(raw: &str) -> Option<String> {
    let value = raw.strip_prefix("sha256:").unwrap_or(raw);
    if value.len() != 64 || !value.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(value.to_ascii_lowercase())
}

fn is_valid_release_version(value: &str) -> bool {
    if !value.starts_with('v') {
        return false;
    }
    let mut parts = value[1..].split('.');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(major), Some(minor), Some(patch), None) => {
            !major.is_empty()
                && !minor.is_empty()
                && !patch.is_empty()
                && major.chars().all(|c| c.is_ascii_digit())
                && minor.chars().all(|c| c.is_ascii_digit())
                && patch.chars().all(|c| c.is_ascii_digit())
        }
        _ => false,
    }
}

fn is_executable(path: &Path) -> std::io::Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = path.metadata()?.permissions().mode();
        Ok(mode & 0o111 != 0)
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(true)
    }
}

fn ensure_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;

        let metadata = path.metadata()?;
        let mut permissions = metadata.permissions();
        let mode = permissions.mode();
        if mode & 0o111 == 0 {
            permissions.set_mode(mode | 0o500);
            fs::set_permissions(path, permissions)?;
        }
        Ok(())
    }

    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn test_resolve_bundled_only() {
        let temp = temp_dir("bundled-only");
        let binary_path = temp
            .join(format!("{}-{}", env::consts::OS, env::consts::ARCH))
            .join("firecracker");
        write_executable(&binary_path);

        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp);
        let resolved = opts.resolve_firecracker_bin().unwrap();
        assert_eq!(resolved, binary_path);
    }

    #[test]
    fn test_release_layout_resolution() {
        let temp = temp_dir("release-layout");
        let version = "v1.12.0";
        let arch = env::consts::ARCH;
        let binary_path = temp
            .join(format!("release-{version}-{arch}"))
            .join(format!("firecracker-{version}-{arch}"));
        write_executable(&binary_path);

        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp)
            .release_version(version);

        let resolved = opts.resolve_firecracker_bin().unwrap();
        assert_eq!(resolved, binary_path);
    }

    #[test]
    fn test_builder_wrappers() {
        let temp = temp_dir("builder-wrapper");
        let version = "v1.12.0";
        let arch = env::consts::ARCH;

        let fc_path = temp
            .join(format!("release-{version}-{arch}"))
            .join(format!("firecracker-{version}-{arch}"));
        let jailer_path = temp
            .join(format!("release-{version}-{arch}"))
            .join(format!("jailer-{version}-{arch}"));
        write_executable(&fc_path);
        write_executable(&jailer_path);

        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp)
            .release_version(version);

        let _fc_builder = opts.firecracker_builder("/tmp/fc.sock").unwrap();
        let _jailer_builder = opts
            .jailer_builder(VmId::new("vm-1").unwrap(), 1000, 1000)
            .unwrap();
    }

    #[test]
    fn test_checksum_mismatch() {
        let temp = temp_dir("checksum-mismatch");
        let binary_path = temp
            .join(format!("{}-{}", env::consts::OS, env::consts::ARCH))
            .join("firecracker");
        write_executable(&binary_path);

        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp)
            .firecracker_sha256("0000000000000000000000000000000000000000000000000000000000000000");

        let err = opts.resolve_firecracker_bin().unwrap_err();
        match err {
            BundledRuntimeError::ChecksumMismatch { .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_invalid_release_version_rejected() {
        let temp = temp_dir("invalid-release-version");
        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp)
            .release_version("1.2.3");

        let err = opts.resolve_firecracker_bin().unwrap_err();
        match err {
            BundledRuntimeError::InvalidReleaseVersion(version) => assert_eq!(version, "1.2.3"),
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn test_supported_release_target_matrix() {
        assert!(is_supported_release_target("linux", "x86_64"));
        assert!(is_supported_release_target("linux", "aarch64"));
        assert!(!is_supported_release_target("linux", "riscv64"));
        assert!(!is_supported_release_target("darwin", "x86_64"));
    }

    #[test]
    fn test_missing_binary_reports_searched_candidates() {
        let temp = temp_dir("missing-binary");
        let opts = BundledRuntimeOptions::new()
            .mode(BundledMode::BundledOnly)
            .bundle_root(&temp);

        let err = opts.resolve_jailer_bin().unwrap_err();
        match err {
            BundledRuntimeError::BinaryNotFound { binary, searched } => {
                assert_eq!(binary, "jailer");
                assert!(!searched.is_empty());
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    fn temp_dir(prefix: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!(
            "firecracker-runtime-{prefix}-{}-{}",
            std::process::id(),
            unique_suffix()
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn unique_suffix() -> u64 {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64
    }

    fn write_executable(path: &Path) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, b"test-binary").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perm = path.metadata().unwrap().permissions();
            perm.set_mode(0o755);
            fs::set_permissions(path, perm).unwrap();
        }
    }
}
