//! NVMe Feature management module for NVMe 2.3 specification.

use alloc::vec::Vec;

use crate::cmd::{Command, FeatureId};
use crate::error::{Error, Result};

/// Power state descriptor.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PowerStateDescriptor {
    /// Maximum power in centiwatts (100mW units)
    pub max_power: u16,
    /// Reserved
    _rsvd1: u8,
    /// Flags
    pub flags: u8,
    /// Entry latency in microseconds
    pub entry_latency: u32,
    /// Exit latency in microseconds
    pub exit_latency: u32,
    /// Relative read throughput
    pub read_throughput: u8,
    /// Relative read latency
    pub read_latency: u8,
    /// Relative write throughput
    pub write_throughput: u8,
    /// Relative write latency
    pub write_latency: u8,
    /// Idle power in centiwatts
    pub idle_power: u16,
    /// Idle power scale
    pub idle_power_scale: u8,
    /// Reserved
    _rsvd2: u8,
    /// Active power in centiwatts
    pub active_power: u16,
    /// Active power scale
    pub active_power_scale: u8,
    /// Reserved
    _rsvd3: [u8; 9],
}

/// Power management configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct PowerManagementConfig {
    /// Power state to transition to
    pub power_state: u8,
    /// Workload hint
    pub workload_hint: u8,
    /// Non-operational power state permissive mode
    pub non_op_permissive: bool,
}

/// Temperature threshold configuration.
#[derive(Debug, Clone, Copy)]
pub struct TemperatureThreshold {
    /// Temperature threshold in Kelvin
    pub threshold: u16,
    /// Temperature select
    pub select: u8,
    /// Threshold type
    pub threshold_type: u8,
}

/// Autonomous Power State Transition (APST) configuration entry.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ApstEntry {
    /// Idle time prior to transition in milliseconds
    pub idle_time_ms: u32,
    /// Idle transition power state
    pub power_state: u8,
    /// Reserved
    _rsvd: [u8; 3],
}

/// Autonomous Power State configuration.
#[derive(Debug, Clone)]
pub struct AutonomousPowerStateConfig {
    /// Enable APST
    pub enabled: bool,
    /// APST entries (up to 32)
    pub entries: Vec<ApstEntry>,
}

/// Host Memory Buffer descriptor.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct HostMemoryBufferDescriptor {
    /// Physical base address
    pub address: u64,
    /// Size in units of 4KB pages
    pub size: u32,
    /// Reserved
    _rsvd: u32,
}

/// Host Memory Buffer configuration.
#[derive(Debug, Clone)]
pub struct HostMemoryBufferConfig {
    /// Enable HMB
    pub enabled: bool,
    /// Memory return
    pub memory_return: bool,
    /// Host memory buffer size
    pub size: u64,
    /// Host memory descriptor list
    pub descriptors: Vec<HostMemoryBufferDescriptor>,
}

/// Interrupt coalescing configuration.
#[derive(Debug, Clone, Copy)]
pub struct InterruptCoalescingConfig {
    /// Aggregation threshold (number of completion entries)
    pub threshold: u8,
    /// Aggregation time in 100 microsecond increments
    pub time: u8,
}

/// Asynchronous Event configuration.
#[derive(Debug, Clone, Copy)]
pub struct AsyncEventConfig {
    /// Critical warning mask
    pub critical_warning_mask: u8,
    /// SMART/Health critical warnings
    pub smart_health_enable: bool,
    /// Namespace attribute notices
    pub namespace_attribute_enable: bool,
    /// Firmware activation notices
    pub firmware_activation_enable: bool,
    /// Telemetry log notices
    pub telemetry_enable: bool,
    /// ANA change notices (Asymmetric Namespace Access)
    pub ana_change_enable: bool,
    /// Predictable latency event aggregate log change notices
    pub predictable_latency_enable: bool,
    /// LBA status information notices
    pub lba_status_enable: bool,
    /// Endurance group event aggregate log change notices
    pub endurance_group_enable: bool,
}

/// Keep Alive Timer configuration.
#[derive(Debug, Clone, Copy)]
pub struct KeepAliveTimerConfig {
    /// Keep alive timeout in milliseconds (0 = disabled)
    pub timeout_ms: u32,
}

/// Sanitize configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct SanitizeConfig {
    /// No-deallocate after sanitize
    pub no_dealloc_after_sanitize: bool,
    /// No-deallocate modifies media
    pub no_dealloc_modifies_media: bool,
}

/// Feature configuration selector.
#[derive(Debug, Clone, Copy)]
pub enum FeatureSelector {
    /// Current operating value
    Current = 0,
    /// Default value
    Default = 1,
    /// Saved value
    Saved = 2,
    /// Supported capabilities
    Supported = 3,
}

/// Feature configuration result.
#[derive(Debug, Clone)]
pub struct FeatureResult {
    /// Feature ID
    pub feature_id: FeatureId,
    /// Feature value
    pub value: u32,
    /// Additional data if applicable
    pub data: Option<Vec<u8>>,
}

/// Power Limit Configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct PowerLimitConfig {
    /// Power limit in watts
    pub power_limit_watts: u16,
    /// Time window for averaging in milliseconds
    pub time_window_ms: u32,
    /// Enable power limit
    pub enabled: bool,
}

/// Self-reported Drive Power capability for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct SelfReportedPower {
    /// Current power consumption in watts
    pub current_power_watts: u16,
    /// Maximum power consumption recorded
    pub max_power_watts: u16,
    /// Average power consumption
    pub average_power_watts: u16,
    /// Time period for average in seconds
    pub average_time_seconds: u32,
}

/// Configurable Device Personality for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub enum DevicePersonality {
    /// Default balanced mode
    Balanced,
    /// High performance mode
    HighPerformance,
    /// Low power mode
    LowPower,
    /// Low latency mode
    LowLatency,
    /// High capacity mode
    HighCapacity,
    /// Custom profile
    Custom(u8),
}

/// Predictable Latency Mode configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct PredictableLatencyConfig {
    /// Enable predictable latency mode
    pub enabled: bool,
    /// Window select
    pub window: u8,
    /// Target read latency in microseconds
    pub target_read_latency_us: u32,
    /// Target write latency in microseconds
    pub target_write_latency_us: u32,
}

/// Host Behavior Support for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct HostBehaviorSupport {
    /// Host supports 128-bit CAS
    pub cas_128bit: bool,
    /// Advanced Command Retry Enable
    pub acre: bool,
    /// Extended Telemetry Data Area 4
    pub etdas: bool,
    /// LBA Format Extension Enable
    pub lbafee: bool,
}

/// Endurance Group Event configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct EnduranceGroupEventConfig {
    /// Enable endurance group events
    pub enabled: bool,
    /// Critical warning threshold percentage
    pub critical_warning_threshold: u8,
}

/// Feature management interface.
pub struct FeatureManager {
    /// Cached feature configurations
    power_management: Option<PowerManagementConfig>,
    temperature_threshold: Option<TemperatureThreshold>,
    interrupt_coalescing: Option<InterruptCoalescingConfig>,
    async_event_config: Option<AsyncEventConfig>,
    keep_alive_timer: Option<KeepAliveTimerConfig>,
    sanitize_config: Option<SanitizeConfig>,
    power_limit_config: Option<PowerLimitConfig>,
    device_personality: Option<DevicePersonality>,
    predictable_latency: Option<PredictableLatencyConfig>,
    host_behavior: Option<HostBehaviorSupport>,
    endurance_group_event: Option<EnduranceGroupEventConfig>,
}

impl Default for FeatureManager {
    fn default() -> Self {
        Self {
            power_management: None,
            temperature_threshold: None,
            interrupt_coalescing: None,
            async_event_config: None,
            keep_alive_timer: None,
            sanitize_config: None,
            power_limit_config: None,
            device_personality: None,
            predictable_latency: None,
            host_behavior: None,
            endurance_group_event: None,
        }
    }
}

impl FeatureManager {
    /// Create a new feature manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Configure power management settings.
    pub fn set_power_management(&mut self, config: PowerManagementConfig) {
        self.power_management = Some(config);
    }

    /// Get power management configuration.
    pub fn get_power_management(&self) -> Option<&PowerManagementConfig> {
        self.power_management.as_ref()
    }

    /// Configure temperature threshold.
    pub fn set_temperature_threshold(&mut self, config: TemperatureThreshold) {
        self.temperature_threshold = Some(config);
    }

    /// Configure interrupt coalescing.
    pub fn set_interrupt_coalescing(&mut self, config: InterruptCoalescingConfig) {
        self.interrupt_coalescing = Some(config);
    }

    /// Configure async event settings.
    pub fn set_async_event_config(&mut self, config: AsyncEventConfig) {
        self.async_event_config = Some(config);
    }

    /// Configure keep alive timer.
    pub fn set_keep_alive_timer(&mut self, config: KeepAliveTimerConfig) {
        self.keep_alive_timer = Some(config);
    }

    /// Configure sanitize settings.
    pub fn set_sanitize_config(&mut self, config: SanitizeConfig) {
        self.sanitize_config = Some(config);
    }

    /// Configure power limit (NVMe 2.3).
    pub fn set_power_limit(&mut self, config: PowerLimitConfig) {
        self.power_limit_config = Some(config);
    }

    /// Set device personality (NVMe 2.3).
    pub fn set_device_personality(&mut self, personality: DevicePersonality) {
        self.device_personality = Some(personality);
    }

    /// Configure predictable latency mode (NVMe 2.3).
    pub fn set_predictable_latency(&mut self, config: PredictableLatencyConfig) {
        self.predictable_latency = Some(config);
    }

    /// Configure host behavior support (NVMe 2.3).
    pub fn set_host_behavior(&mut self, config: HostBehaviorSupport) {
        self.host_behavior = Some(config);
    }

    /// Configure endurance group events (NVMe 2.3).
    pub fn set_endurance_group_event(&mut self, config: EnduranceGroupEventConfig) {
        self.endurance_group_event = Some(config);
    }

    /// Build Set Features command for power management.
    pub fn build_power_management_command(&self, cmd_id: u16) -> Result<Command> {
        let config = self.power_management
            .ok_or(Error::InvalidFeatureConfig)?;

        let value = (config.workload_hint as u32) << 5 | config.power_state as u32;
        Ok(Command::set_features(cmd_id, FeatureId::PowerManagement, value, false))
    }

    /// Build Set Features command for async events.
    pub fn build_async_event_command(&self, cmd_id: u16) -> Result<Command> {
        let config = self.async_event_config
            .ok_or(Error::InvalidFeatureConfig)?;

        let mut value = config.critical_warning_mask as u32;
        if config.smart_health_enable { value |= 1 << 8; }
        if config.namespace_attribute_enable { value |= 1 << 9; }
        if config.firmware_activation_enable { value |= 1 << 10; }
        if config.telemetry_enable { value |= 1 << 11; }
        if config.ana_change_enable { value |= 1 << 12; }
        if config.predictable_latency_enable { value |= 1 << 13; }
        if config.lba_status_enable { value |= 1 << 14; }
        if config.endurance_group_enable { value |= 1 << 15; }

        Ok(Command::set_features(cmd_id, FeatureId::AsyncEventConfig, value, false))
    }
}