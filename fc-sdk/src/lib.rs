//! High-level typestate SDK for managing Firecracker microVM lifecycles.
//!
//! This crate provides a type-safe, ergonomic API for configuring and managing
//! Firecracker microVMs. It wraps the low-level [`fc_api`] client with a
//! builder pattern for pre-boot configuration and a `Vm` handle for post-boot
//! operations.
//!
//! # Quick Start
//!
//! ```no_run
//! use fc_sdk::{VmBuilder, types::*};
//!
//! # async fn example() -> fc_sdk::Result<()> {
//! // Build and start a microVM
//! let vm = VmBuilder::new("/tmp/firecracker.sock")
//!     .boot_source(BootSource {
//!         kernel_image_path: "/path/to/vmlinux".into(),
//!         boot_args: Some("console=ttyS0 reboot=k panic=1".into()),
//!         initrd_path: None,
//!     })
//!     .machine_config(MachineConfiguration {
//!         vcpu_count: std::num::NonZeroU64::new(2).unwrap(),
//!         mem_size_mib: 512,
//!         smt: false,
//!         track_dirty_pages: false,
//!         cpu_template: None,
//!         huge_pages: None,
//!     })
//!     .start()
//!     .await?;
//!
//! // Post-boot operations
//! let info = vm.describe().await?;
//! println!("VM state: {:?}", info.state);
//!
//! // Pause and snapshot
//! vm.pause().await?;
//! vm.create_snapshot("/path/to/snapshot", "/path/to/mem").await?;
//! vm.resume().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # Restoring from Snapshot
//!
//! ```no_run
//! use fc_sdk::{restore, types::*};
//!
//! # async fn example() -> fc_sdk::Result<()> {
//! let vm = restore(
//!     "/tmp/firecracker.sock",
//!     SnapshotLoadParams {
//!         snapshot_path: "/path/to/snapshot".into(),
//!         mem_file_path: Some("/path/to/mem".into()),
//!         mem_backend: None,
//!         enable_diff_snapshots: None,
//!         track_dirty_pages: None,
//!         resume_vm: Some(true),
//!         network_overrides: vec![],
//!     },
//! ).await?;
//! # Ok(())
//! # }
//! ```

pub mod builder;
pub mod connection;
pub mod error;
pub mod process;
pub mod vm;
pub mod vm_id;

pub use builder::VmBuilder;
pub use error::{Error, Result};
pub use process::{
    DetachedFirecrackerProcess, FirecrackerProcess, FirecrackerProcessBuilder, JailerProcessBuilder,
};
pub use vm::{Vm, restore, restore_with_client};
pub use vm_id::{VmId, VmIdError};

/// Re-export API types for convenience.
pub use fc_api::types;

/// Re-export the low-level API client for advanced use cases.
pub use fc_api::Client;
