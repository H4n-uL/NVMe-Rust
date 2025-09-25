//! NVMe Power Management module for NVMe 2.3 specification.

use alloc::vec;
use alloc::vec::Vec;
use core::time::Duration;

use crate::cmd::{Command, FeatureId};
use crate::error::{Error, Result};
use crate::features::{DevicePersonality, PowerStateDescriptor};

/// Power state information.
#[derive(Debug, Clone, Copy)]
pub struct PowerState {
    /// Power state ID (0-31)
    pub id: u8,
    /// Maximum power in centiwatts
    pub max_power_cw: u16,
    /// Entry latency in microseconds
    pub entry_latency_us: u32,
    /// Exit latency in microseconds
    pub exit_latency_us: u32,
    /// Relative read throughput
    pub read_throughput: u8,
    /// Relative read latency
    pub read_latency: u8,
    /// Relative write throughput
    pub write_throughput: u8,
    /// Relative write latency
    pub write_latency: u8,
    /// Idle power in centiwatts
    pub idle_power_cw: u16,
    /// Active power in centiwatts
    pub active_power_cw: u16,
    /// Non-operational state
    pub non_operational: bool,
}

impl From<&PowerStateDescriptor> for PowerState {
    fn from(desc: &PowerStateDescriptor) -> Self {
        Self {
            id: 0, // Will be set externally
            max_power_cw: desc.max_power,
            entry_latency_us: desc.entry_latency,
            exit_latency_us: desc.exit_latency,
            read_throughput: desc.read_throughput,
            read_latency: desc.read_latency,
            write_throughput: desc.write_throughput,
            write_latency: desc.write_latency,
            idle_power_cw: desc.idle_power,
            active_power_cw: desc.active_power,
            non_operational: (desc.flags & 0x02) != 0,
        }
    }
}

/// Power Limit Configuration (PLC) for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct PowerLimitConfig {
    /// Power limit in watts
    pub power_limit_watts: u16,
    /// Time window for averaging in milliseconds
    pub time_window_ms: u32,
    /// Enable power limit
    pub enabled: bool,
}

impl PowerLimitConfig {
    /// Create a new power limit configuration.
    pub fn new(power_limit_watts: u16, time_window_ms: u32) -> Self {
        Self {
            power_limit_watts,
            time_window_ms,
            enabled: true,
        }
    }

    /// Disable power limiting.
    pub fn disabled() -> Self {
        Self {
            power_limit_watts: 0,
            time_window_ms: 0,
            enabled: false,
        }
    }

    /// Convert to feature value for Set Features command.
    pub fn to_feature_value(&self) -> u32 {
        if !self.enabled {
            return 0;
        }

        let mut value = self.power_limit_watts as u32;
        value |= ((self.time_window_ms / 100) & 0xFF) << 16; // Time window in 100ms units
        value |= 0x80000000; // Enable bit
        value
    }
}

/// Self-reported Drive Power (SDP) for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct SelfReportedPower {
    /// Current power consumption in watts
    pub current_power_watts: u16,
    /// Maximum power consumption recorded
    pub max_power_watts: u16,
    /// Average power consumption
    pub average_power_watts: u16,
    /// Minimum power consumption recorded
    pub min_power_watts: u16,
    /// Time period for measurements in seconds
    pub measurement_period_sec: u32,
    /// Total energy consumed in watt-hours
    pub total_energy_wh: u64,
}

impl SelfReportedPower {
    /// Parse from log page data.
    pub fn from_log_data(data: &[u8]) -> Result<Self> {
        if data.len() < 32 {
            return Err(Error::InvalidBufferSize);
        }

        Ok(Self {
            current_power_watts: u16::from_le_bytes([data[0], data[1]]),
            max_power_watts: u16::from_le_bytes([data[2], data[3]]),
            average_power_watts: u16::from_le_bytes([data[4], data[5]]),
            min_power_watts: u16::from_le_bytes([data[6], data[7]]),
            measurement_period_sec: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
            total_energy_wh: u64::from_le_bytes([
                data[16], data[17], data[18], data[19],
                data[20], data[21], data[22], data[23],
            ]),
        })
    }

    /// Calculate average power efficiency (operations per watt).
    pub fn power_efficiency(&self, operations: u64) -> f32 {
        if self.average_power_watts == 0 {
            return 0.0;
        }
        operations as f32 / self.average_power_watts as f32
    }
}

/// Configurable Device Personality (CDP) configuration for NVMe 2.3.
#[derive(Debug, Clone, Copy)]
pub struct PersonalityConfig {
    /// Device personality mode
    pub personality: DevicePersonality,
    /// Custom parameters for custom personality
    pub custom_params: Option<CustomPersonalityParams>,
}

/// Custom personality parameters.
#[derive(Debug, Clone, Copy)]
pub struct CustomPersonalityParams {
    /// Target IOPS
    pub target_iops: u32,
    /// Target bandwidth in MB/s
    pub target_bandwidth_mbps: u32,
    /// Target latency in microseconds
    pub target_latency_us: u32,
    /// Power budget in watts
    pub power_budget_watts: u16,
}

impl PersonalityConfig {
    /// Create a balanced personality configuration.
    pub fn balanced() -> Self {
        Self {
            personality: DevicePersonality::Balanced,
            custom_params: None,
        }
    }

    /// Create a high performance personality configuration.
    pub fn high_performance() -> Self {
        Self {
            personality: DevicePersonality::HighPerformance,
            custom_params: None,
        }
    }

    /// Create a low power personality configuration.
    pub fn low_power() -> Self {
        Self {
            personality: DevicePersonality::LowPower,
            custom_params: None,
        }
    }

    /// Create a low latency personality configuration.
    pub fn low_latency() -> Self {
        Self {
            personality: DevicePersonality::LowLatency,
            custom_params: None,
        }
    }

    /// Create a custom personality configuration.
    pub fn custom(params: CustomPersonalityParams) -> Self {
        Self {
            personality: DevicePersonality::Custom(0xFF),
            custom_params: Some(params),
        }
    }

    /// Convert to feature value for Set Features command.
    pub fn to_feature_value(&self) -> u32 {
        match self.personality {
            DevicePersonality::Balanced => 0,
            DevicePersonality::HighPerformance => 1,
            DevicePersonality::LowPower => 2,
            DevicePersonality::LowLatency => 3,
            DevicePersonality::HighCapacity => 4,
            DevicePersonality::Custom(val) => val as u32,
        }
    }
}

/// Autonomous Power State Transition (APST) configuration.
#[derive(Debug, Clone)]
pub struct ApstConfig {
    /// Enable APST
    pub enabled: bool,
    /// Transition entries (power state, idle time)
    pub transitions: Vec<(u8, Duration)>,
}

impl ApstConfig {
    /// Create a new APST configuration.
    pub fn new() -> Self {
        Self {
            enabled: false,
            transitions: Vec::new(),
        }
    }

    /// Enable APST with specified transitions.
    pub fn with_transitions(transitions: Vec<(u8, Duration)>) -> Self {
        Self {
            enabled: true,
            transitions,
        }
    }

    /// Add a transition entry.
    pub fn add_transition(&mut self, power_state: u8, idle_time: Duration) {
        self.transitions.push((power_state, idle_time));
    }

    /// Build the APST table for submission.
    pub fn build_table(&self) -> Vec<u8> {
        let mut table = vec![0u8; 256]; // 32 entries * 8 bytes

        for (i, &(ps, idle_time)) in self.transitions.iter().enumerate() {
            if i >= 32 { break; }

            let idle_ms = idle_time.as_millis() as u32;
            let offset = i * 8;

            // Idle time in milliseconds (4 bytes)
            table[offset..offset + 4].copy_from_slice(&idle_ms.to_le_bytes());
            // Power state (1 byte)
            table[offset + 4] = ps;
            // Reserved (3 bytes)
        }

        table
    }
}

/// Power management controller.
pub struct PowerManager {
    /// Available power states
    power_states: Vec<PowerState>,
    /// Current power state
    current_power_state: u8,
    /// Power limit configuration
    power_limit: Option<PowerLimitConfig>,
    /// Self-reported power data
    self_reported_power: Option<SelfReportedPower>,
    /// Device personality configuration
    personality: PersonalityConfig,
    /// APST configuration
    apst_config: ApstConfig,
    /// Power state transition history
    transition_history: Vec<(u8, u8, u64)>, // (from, to, timestamp)
}

impl Default for PowerManager {
    fn default() -> Self {
        Self {
            power_states: Vec::new(),
            current_power_state: 0,
            power_limit: None,
            self_reported_power: None,
            personality: PersonalityConfig::balanced(),
            apst_config: ApstConfig::new(),
            transition_history: Vec::new(),
        }
    }
}

impl PowerManager {
    /// Create a new power manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize with power state descriptors from Identify Controller.
    pub fn init_power_states(&mut self, descriptors: &[PowerStateDescriptor]) {
        self.power_states.clear();
        for (i, desc) in descriptors.iter().enumerate() {
            let mut state = PowerState::from(desc);
            state.id = i as u8;
            self.power_states.push(state);
        }
    }

    /// Set power limit configuration.
    pub fn set_power_limit(&mut self, config: PowerLimitConfig) {
        self.power_limit = Some(config);
    }

    /// Get current power limit.
    pub fn get_power_limit(&self) -> Option<&PowerLimitConfig> {
        self.power_limit.as_ref()
    }

    /// Update self-reported power data.
    pub fn update_self_reported_power(&mut self, data: &[u8]) -> Result<()> {
        self.self_reported_power = Some(SelfReportedPower::from_log_data(data)?);
        Ok(())
    }

    /// Get self-reported power data.
    pub fn get_self_reported_power(&self) -> Option<&SelfReportedPower> {
        self.self_reported_power.as_ref()
    }

    /// Set device personality.
    pub fn set_personality(&mut self, config: PersonalityConfig) {
        self.personality = config;
    }

    /// Get current device personality.
    pub fn get_personality(&self) -> &PersonalityConfig {
        &self.personality
    }

    /// Configure APST.
    pub fn configure_apst(&mut self, config: ApstConfig) {
        self.apst_config = config;
    }

    /// Get APST configuration.
    pub fn get_apst_config(&self) -> &ApstConfig {
        &self.apst_config
    }

    /// Find optimal power state for given constraints.
    pub fn find_optimal_power_state(
        &self,
        max_power_watts: u16,
        max_entry_latency_us: u32,
        max_exit_latency_us: u32,
    ) -> Option<u8> {
        let max_power_cw = max_power_watts * 100; // Convert to centiwatts

        self.power_states
            .iter()
            .filter(|ps| {
                ps.max_power_cw <= max_power_cw
                    && ps.entry_latency_us <= max_entry_latency_us
                    && ps.exit_latency_us <= max_exit_latency_us
                    && !ps.non_operational
            })
            .min_by_key(|ps| ps.idle_power_cw) // Choose lowest idle power
            .map(|ps| ps.id)
    }

    /// Transition to a new power state.
    pub fn transition_to(&mut self, power_state: u8, timestamp: u64) -> Result<()> {
        if power_state as usize >= self.power_states.len() {
            return Err(Error::InvalidFeatureConfig);
        }

        // Record transition
        self.transition_history.push((
            self.current_power_state,
            power_state,
            timestamp,
        ));

        // Keep history limited
        if self.transition_history.len() > 1000 {
            self.transition_history.remove(0);
        }

        self.current_power_state = power_state;
        Ok(())
    }

    /// Get current power state.
    pub fn get_current_power_state(&self) -> u8 {
        self.current_power_state
    }

    /// Get power state information.
    pub fn get_power_state_info(&self, state_id: u8) -> Option<&PowerState> {
        self.power_states.get(state_id as usize)
    }

    /// Get all available power states.
    pub fn get_power_states(&self) -> &[PowerState] {
        &self.power_states
    }

    /// Calculate current power consumption estimate.
    pub fn estimate_current_power(&self) -> u16 {
        self.power_states
            .get(self.current_power_state as usize)
            .map(|ps| ps.active_power_cw)
            .unwrap_or(0)
    }

    /// Build Set Features command for power management.
    pub fn build_power_management_command(&self, cmd_id: u16, power_state: u8) -> Command {
        Command::set_features(
            cmd_id,
            FeatureId::PowerManagement,
            power_state as u32,
            false,
        )
    }

    /// Build Set Features command for power limit.
    pub fn build_power_limit_command(&self, cmd_id: u16) -> Result<Command> {
        let config = self.power_limit.ok_or(Error::InvalidFeatureConfig)?;
        Ok(Command::set_features(
            cmd_id,
            FeatureId::PowerManagement,
            config.to_feature_value(),
            false,
        ))
    }

    /// Get transition history.
    pub fn get_transition_history(&self) -> &[(u8, u8, u64)] {
        &self.transition_history
    }

    /// Clear transition history.
    pub fn clear_transition_history(&mut self) {
        self.transition_history.clear();
    }
}
