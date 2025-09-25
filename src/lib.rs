//! A no-std compatible NVMe driver for embedded and operating system development.
//!
//! This crate provides full NVMe 2.3 specification support for interacting with NVMe
//! (Non-Volatile Memory Express) storage devices in environments without
//! the standard library, such as kernels, bootloaders, or embedded systems.
//!
//! # NVMe 2.3 Features
//! - Complete admin and I/O command set
//! - Rapid Path Failure Recovery (RPFR)
//! - Power Limit Configuration (PLC) and Self-reported Drive Power (SDP)
//! - Configurable Device Personality (CDP)
//! - Sanitize Per Namespace (SPN)
//! - Enhanced error handling and asynchronous events
//! - Multipath I/O and Asymmetric Namespace Access (ANA)
//! - Firmware update and security features
#![no_std]
#![deny(missing_docs)]

extern crate alloc;

mod cmd;
mod device;
mod error;
mod memory;
mod queues;

// NVMe 2.3 modules
mod events;
mod features;
mod firmware;
mod log;
mod multipath;
mod power;
mod security;

// Core exports
pub use device::{ControllerData, NVMeDevice, Namespace};
pub use error::{Error, StatusCode, StatusCodeType};
pub use memory::Allocator;

// NVMe 2.3 feature exports
pub use events::{AsyncEvent, AsyncEventManager, AsyncEventType, CriticalWarning};
pub use features::{
    AsyncEventConfig, AutonomousPowerStateConfig, DevicePersonality, FeatureManager,
    HostBehaviorSupport, InterruptCoalescingConfig, KeepAliveTimerConfig,
    PowerManagementConfig, PredictableLatencyConfig, SanitizeConfig, TemperatureThreshold,
};
pub use firmware::{
    FirmwareCommitAction, FirmwareManager, FirmwareSlotInfo, FirmwareUpdateConfig,
    FirmwareUpdateStatus,
};
pub use log::{LogPageManager, SmartHealthInfo};
pub use multipath::{
    AnaState, ControllerPath, MultipathController, PathSelector, PathState, RpfrConfig,
};
pub use power::{
    ApstConfig, PersonalityConfig, PowerLimitConfig, PowerManager, PowerState,
    SelfReportedPower,
};
pub use security::{
    CryptoEraseConfig, SanitizeAction, SanitizeOptions, SanitizePerNamespace,
    SanitizeStatus, SecurityManager,
};

/// NVMe 2.3 specification version
pub const NVME_SPEC_VERSION: (u16, u8, u8) = (2, 3, 0);
