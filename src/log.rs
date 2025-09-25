//! NVMe Log Page management module for NVMe 2.3 specification.

use alloc::vec::Vec;
use core::mem::size_of;

use crate::cmd::{Command, LogPageId};
use crate::error::Result;

/// Error log entry structure.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ErrorLogEntry {
    /// Error count for this entry
    pub error_count: u64,
    /// Submission queue ID
    pub sqid: u16,
    /// Command ID
    pub cmdid: u16,
    /// Status field
    pub status: u16,
    /// Parameter error location
    pub param_error_location: u16,
    /// LBA
    pub lba: u64,
    /// Namespace ID
    pub nsid: u32,
    /// Vendor specific info available
    pub vs: u8,
    /// Transport type
    pub trtype: u8,
    /// Reserved
    _rsvd1: [u8; 2],
    /// Command specific info
    pub cs_info: u64,
    /// Transport type specific
    pub trtype_specific: u16,
    /// Reserved
    _rsvd2: [u8; 22],
}

/// SMART / Health Information log page.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SmartHealthInfo {
    /// Critical warning flags
    pub critical_warning: u8,
    /// Composite temperature in Kelvin
    pub temperature: u16,
    /// Available spare percentage
    pub available_spare: u8,
    /// Available spare threshold percentage
    pub available_spare_threshold: u8,
    /// Percentage used estimate
    pub percentage_used: u8,
    /// Endurance group critical warning summary
    pub endurance_critical_warning: u8,
    /// Reserved
    _rsvd1: [u8; 25],
    /// Data units read (128KB units)
    pub data_units_read: u128,
    /// Data units written (128KB units)
    pub data_units_written: u128,
    /// Host read commands
    pub host_read_commands: u128,
    /// Host write commands
    pub host_write_commands: u128,
    /// Controller busy time in minutes
    pub controller_busy_time: u128,
    /// Power cycles
    pub power_cycles: u128,
    /// Power on hours
    pub power_on_hours: u128,
    /// Unsafe shutdowns
    pub unsafe_shutdowns: u128,
    /// Media errors
    pub media_errors: u128,
    /// Number of error log entries
    pub num_error_log_entries: u128,
    /// Warning temperature time in minutes
    pub warning_temp_time: u32,
    /// Critical temperature time in minutes
    pub critical_temp_time: u32,
    /// Temperature sensor 1-8 in Kelvin
    pub temp_sensor: [u16; 8],
    /// Thermal management temperature 1 transition count
    pub tmt1_transition_count: u32,
    /// Thermal management temperature 2 transition count
    pub tmt2_transition_count: u32,
    /// Total time for thermal management temperature 1
    pub tmt1_total_time: u32,
    /// Total time for thermal management temperature 2
    pub tmt2_total_time: u32,
    /// Reserved
    _rsvd2: [u8; 280],
}

/// Firmware slot information.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct FirmwareSlotInfo {
    /// Active firmware info
    pub afi: u8,
    /// Reserved
    _rsvd1: [u8; 7],
    /// Firmware revision for slot 1-7 (8 bytes each)
    pub firmware_revision: [[u8; 8]; 7],
    /// Reserved
    _rsvd2: [u8; 448],
}

/// Changed namespace list entry.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct ChangedNamespaceList {
    /// List of changed namespace IDs (up to 1024)
    pub nsid_list: [u32; 1024],
}

/// Commands supported and effects log page entry.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct CommandEffects {
    /// Command supported
    pub csupp: bool,
    /// Logical block content change
    pub lbcc: bool,
    /// Namespace capability change
    pub ncc: bool,
    /// Namespace inventory change
    pub nic: bool,
    /// Controller capability change
    pub ccc: bool,
    /// UUID selection supported
    pub uuid: bool,
    /// Command submission and execution
    pub cse: u8,
}

/// Telemetry log page header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct TelemetryLogHeader {
    /// Log identifier
    pub log_id: u8,
    /// Reserved
    _rsvd1: [u8; 4],
    /// IEEE OUI identifier
    pub ieee_oui: [u8; 3],
    /// Data area 1 last block
    pub da1_last_block: u16,
    /// Data area 2 last block
    pub da2_last_block: u16,
    /// Data area 3 last block
    pub da3_last_block: u16,
    /// Reserved
    _rsvd2: [u8; 2],
    /// Data area 4 last block (NVMe 2.3)
    pub da4_last_block: u32,
    /// Reserved
    _rsvd3: [u8; 361],
    /// Host-initiated data generation number
    pub host_initiated_data_gen: u8,
    /// Controller-initiated data available
    pub controller_initiated_data_avail: u8,
    /// Controller-initiated data generation number
    pub controller_initiated_data_gen: u8,
    /// Reason identifier
    pub reason_id: [u8; 128],
}

/// Endurance group information.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct EnduranceGroupInfo {
    /// Critical warning
    pub critical_warning: u8,
    /// Reserved
    _rsvd1: [u8; 2],
    /// Available spare percentage
    pub available_spare: u8,
    /// Available spare threshold
    pub available_spare_threshold: u8,
    /// Percentage used
    pub percentage_used: u8,
    /// Reserved
    _rsvd2: [u8; 26],
    /// Endurance estimate (in units of 100 million)
    pub endurance_estimate: u128,
    /// Data units read
    pub data_units_read: u128,
    /// Data units written
    pub data_units_written: u128,
    /// Media units written
    pub media_units_written: u128,
    /// Host read commands
    pub host_read_commands: u128,
    /// Host write commands
    pub host_write_commands: u128,
    /// Media data integrity errors
    pub media_data_integrity_errors: u128,
    /// Number of error information log entries
    pub num_error_info_log_entries: u128,
    /// Reserved
    _rsvd3: [u8; 352],
}

/// Predictable latency per NVM set.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PredictableLatencyPerNvmSet {
    /// Status
    pub status: u8,
    /// Event type
    pub event_type: u8,
    /// Reserved
    _rsvd1: u8,
    /// Reserved
    _rsvd2: [u8; 61],
    /// DTWIN reads typical
    pub dtwin_reads_typical: u64,
    /// DTWIN writes typical
    pub dtwin_writes_typical: u64,
    /// DTWIN time maximum
    pub dtwin_time_maximum: u64,
    /// NDWIN time minimum high
    pub ndwin_time_minimum_high: u64,
    /// NDWIN time minimum low
    pub ndwin_time_minimum_low: u64,
    /// Reserved
    _rsvd3: [u8; 968],
}

/// Persistent event log header.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct PersistentEventLogHeader {
    /// Log identifier
    pub log_id: u8,
    /// Reserved
    _rsvd1: [u8; 3],
    /// Total number of events
    pub total_events: u32,
    /// Total log length
    pub total_log_length: u64,
    /// Log revision
    pub log_revision: u8,
    /// Reserved
    _rsvd2: u8,
    /// Header length
    pub header_length: u16,
    /// Timestamp
    pub timestamp: u64,
    /// Power on hours
    pub power_on_hours: u128,
    /// Power cycle count
    pub power_cycle_count: u64,
    /// PCI vendor ID
    pub pci_vid: u16,
    /// PCI subsystem vendor ID
    pub pci_ssvid: u16,
    /// Serial number
    pub serial_number: [u8; 20],
    /// Model number
    pub model_number: [u8; 40],
    /// NVM subsystem NVMe qualified name
    pub subsystem_nqn: [u8; 256],
    /// Reserved
    _rsvd3: [u8; 108],
    /// Supported event bitmap
    pub supported_events: [u8; 32],
}

/// LBA status information.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct LbaStatusInfo {
    /// Number of LBA ranges
    pub num_lba_ranges: u32,
    /// Reserved
    _rsvd: [u8; 4],
    // LBA range entries follow
}

/// Media unit status.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct MediaUnitStatus {
    /// Number of media unit status descriptors
    pub num_mus_descriptors: u16,
    /// Reserved
    _rsvd1: [u8; 2],
    /// Media unit capacity adjustment factor
    pub mucaf: u16,
    /// Reserved
    _rsvd2: [u8; 10],
    // Media unit status descriptors follow
}

/// Supported log pages.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SupportedLogPages {
    /// Supported log page entries (bit per LID)
    pub supported: [u8; 256],
}

/// Log page manager for handling various log pages.
pub struct LogPageManager {
    /// Error log entries cache
    error_log: Vec<ErrorLogEntry>,
    /// SMART/Health information cache
    smart_health: Option<SmartHealthInfo>,
    /// Firmware slot info cache
    firmware_slot: Option<FirmwareSlotInfo>,
    /// Changed namespace list cache
    changed_namespaces: Vec<u32>,
    /// Telemetry data cache
    telemetry_host: Vec<u8>,
    telemetry_controller: Vec<u8>,
    /// Endurance group info cache
    endurance_group: Option<EnduranceGroupInfo>,
    /// Persistent event log cache
    persistent_events: Vec<u8>,
}

impl Default for LogPageManager {
    fn default() -> Self {
        Self {
            error_log: Vec::new(),
            smart_health: None,
            firmware_slot: None,
            changed_namespaces: Vec::new(),
            telemetry_host: Vec::new(),
            telemetry_controller: Vec::new(),
            endurance_group: None,
            persistent_events: Vec::new(),
        }
    }
}

impl LogPageManager {
    /// Create a new log page manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse error log page data.
    pub fn parse_error_log(&mut self, data: &[u8]) -> Result<Vec<ErrorLogEntry>> {
        let entry_size = size_of::<ErrorLogEntry>();
        let num_entries = data.len() / entry_size;
        let mut entries = Vec::with_capacity(num_entries);

        for i in 0..num_entries {
            let start = i * entry_size;
            let entry_data = &data[start..start + entry_size];
            let entry = unsafe {
                core::ptr::read_unaligned(entry_data.as_ptr() as *const ErrorLogEntry)
            };
            entries.push(entry);
        }

        self.error_log = entries.clone();
        Ok(entries)
    }

    /// Parse SMART/Health information.
    pub fn parse_smart_health(&mut self, data: &[u8]) -> Result<SmartHealthInfo> {
        let info = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const SmartHealthInfo)
        };
        self.smart_health = Some(info);
        Ok(info)
    }

    /// Parse firmware slot information.
    pub fn parse_firmware_slot(&mut self, data: &[u8]) -> Result<FirmwareSlotInfo> {
        let info = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const FirmwareSlotInfo)
        };
        self.firmware_slot = Some(info);
        Ok(info)
    }

    /// Parse changed namespace list.
    pub fn parse_changed_namespaces(&mut self, data: &[u8]) -> Result<Vec<u32>> {
        let list = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const ChangedNamespaceList)
        };

        let mut namespaces = Vec::new();
        // Use a local copy to avoid unaligned access
        let nsid_list = list.nsid_list;
        for nsid in nsid_list {
            if nsid == 0 { break; }
            namespaces.push(nsid);
        }

        self.changed_namespaces = namespaces.clone();
        Ok(namespaces)
    }

    /// Parse telemetry log header.
    pub fn parse_telemetry_header(&self, data: &[u8]) -> Result<TelemetryLogHeader> {
        let header = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const TelemetryLogHeader)
        };
        Ok(header)
    }

    /// Parse endurance group information.
    pub fn parse_endurance_group(&mut self, data: &[u8]) -> Result<EnduranceGroupInfo> {
        let info = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const EnduranceGroupInfo)
        };
        self.endurance_group = Some(info);
        Ok(info)
    }

    /// Parse persistent event log header.
    pub fn parse_persistent_event_header(&self, data: &[u8]) -> Result<PersistentEventLogHeader> {
        let header = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const PersistentEventLogHeader)
        };
        Ok(header)
    }

    /// Parse supported log pages.
    pub fn parse_supported_log_pages(&self, data: &[u8]) -> Result<Vec<u8>> {
        let pages = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const SupportedLogPages)
        };

        let mut supported = Vec::new();
        for lid in 0..=255u8 {
            let byte_idx = lid / 8;
            let bit_idx = lid % 8;
            if pages.supported[byte_idx as usize] & (1 << bit_idx) != 0 {
                supported.push(lid);
            }
        }

        Ok(supported)
    }

    /// Build Get Log Page command.
    pub fn build_get_log_command(
        &self,
        cmd_id: u16,
        log_id: LogPageId,
        address: usize,
        num_dwords: u32,
        offset: u64,
    ) -> Command {
        Command::get_log_page(cmd_id, address, log_id, num_dwords, offset)
    }

    /// Get cached SMART/Health info.
    pub fn get_smart_health(&self) -> Option<&SmartHealthInfo> {
        self.smart_health.as_ref()
    }

    /// Get cached error log entries.
    pub fn get_error_log(&self) -> &[ErrorLogEntry] {
        &self.error_log
    }

    /// Get cached firmware slot info.
    pub fn get_firmware_slot(&self) -> Option<&FirmwareSlotInfo> {
        self.firmware_slot.as_ref()
    }

    /// Get cached changed namespaces.
    pub fn get_changed_namespaces(&self) -> &[u32] {
        &self.changed_namespaces
    }

    /// Get cached endurance group info.
    pub fn get_endurance_group(&self) -> Option<&EnduranceGroupInfo> {
        self.endurance_group.as_ref()
    }
}
