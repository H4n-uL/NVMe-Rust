//! NVMe Asynchronous Event management module for NVMe 2.3 specification.

use alloc::collections::VecDeque;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, Ordering};

use crate::cmd::Command;
use crate::error::Result;

/// Asynchronous event type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsyncEventType {
    /// Error status
    ErrorStatus = 0,
    /// SMART/Health status
    SmartHealth = 1,
    /// Notice
    Notice = 2,
    /// I/O command set specific
    IoCommandSet = 6,
    /// Vendor specific
    VendorSpecific = 7,
}

/// Asynchronous event info.
#[derive(Debug, Clone, Copy)]
pub enum AsyncEventInfo {
    // Error status events
    InvalidSubmissionQueue,
    InvalidCompletionQueue,
    DiagnosticFailure,
    PersistentInternalError,
    TransientInternalError,
    FirmwareImageLoadError,

    // SMART/Health status events
    DeviceReliabilityDegraded,
    TemperatureAboveThreshold,
    MediaPlacedInReadOnly,

    // Notice events
    NamespaceAttributeChanged,
    FirmwareActivationStarting,
    TelemetryLogChanged,
    AsymmetricNamespaceAccessChange,
    PredictableLatencyEventAggregateLogChange,
    LbaStatusInformationAlert,
    EnduranceGroupEventAggregateLogChange,

    // Vendor specific
    VendorSpecific(u8),
}

/// Asynchronous event request result.
#[derive(Debug, Clone, Copy)]
pub struct AsyncEvent {
    /// Event type
    pub event_type: AsyncEventType,
    /// Event information
    pub event_info: AsyncEventInfo,
    /// Associated log page ID (if any)
    pub log_page: Option<u8>,
}

impl AsyncEvent {
    /// Parse from completion entry dword 0.
    pub fn from_completion(dw0: u32) -> Self {
        let event_type_raw = ((dw0 >> 16) & 0x7) as u8;
        let event_info_raw = ((dw0 >> 8) & 0xFF) as u8;
        let log_page = (dw0 & 0xFF) as u8;

        let event_type = match event_type_raw {
            0 => AsyncEventType::ErrorStatus,
            1 => AsyncEventType::SmartHealth,
            2 => AsyncEventType::Notice,
            6 => AsyncEventType::IoCommandSet,
            7 => AsyncEventType::VendorSpecific,
            _ => AsyncEventType::ErrorStatus,
        };

        let event_info = match (event_type, event_info_raw) {
            // Error status events
            (AsyncEventType::ErrorStatus, 0) => AsyncEventInfo::InvalidSubmissionQueue,
            (AsyncEventType::ErrorStatus, 1) => AsyncEventInfo::InvalidCompletionQueue,
            (AsyncEventType::ErrorStatus, 2) => AsyncEventInfo::DiagnosticFailure,
            (AsyncEventType::ErrorStatus, 3) => AsyncEventInfo::PersistentInternalError,
            (AsyncEventType::ErrorStatus, 4) => AsyncEventInfo::TransientInternalError,
            (AsyncEventType::ErrorStatus, 5) => AsyncEventInfo::FirmwareImageLoadError,

            // SMART/Health events
            (AsyncEventType::SmartHealth, 0) => AsyncEventInfo::DeviceReliabilityDegraded,
            (AsyncEventType::SmartHealth, 1) => AsyncEventInfo::TemperatureAboveThreshold,
            (AsyncEventType::SmartHealth, 2) => AsyncEventInfo::MediaPlacedInReadOnly,

            // Notice events
            (AsyncEventType::Notice, 0) => AsyncEventInfo::NamespaceAttributeChanged,
            (AsyncEventType::Notice, 1) => AsyncEventInfo::FirmwareActivationStarting,
            (AsyncEventType::Notice, 2) => AsyncEventInfo::TelemetryLogChanged,
            (AsyncEventType::Notice, 3) => AsyncEventInfo::AsymmetricNamespaceAccessChange,
            (AsyncEventType::Notice, 4) => AsyncEventInfo::PredictableLatencyEventAggregateLogChange,
            (AsyncEventType::Notice, 5) => AsyncEventInfo::LbaStatusInformationAlert,
            (AsyncEventType::Notice, 6) => AsyncEventInfo::EnduranceGroupEventAggregateLogChange,

            // Vendor specific
            (AsyncEventType::VendorSpecific, val) => AsyncEventInfo::VendorSpecific(val),

            _ => AsyncEventInfo::VendorSpecific(event_info_raw),
        };

        let log_page = if log_page != 0 {
            Some(log_page)
        } else {
            None
        };

        Self {
            event_type,
            event_info,
            log_page,
        }
    }

    /// Check if this event requires immediate attention.
    pub fn is_critical(&self) -> bool {
        matches!(
            self.event_info,
            AsyncEventInfo::PersistentInternalError
                | AsyncEventInfo::TransientInternalError
                | AsyncEventInfo::FirmwareImageLoadError
                | AsyncEventInfo::DeviceReliabilityDegraded
                | AsyncEventInfo::MediaPlacedInReadOnly
        )
    }

    /// Get recommended log page to retrieve for this event.
    pub fn recommended_log_page(&self) -> Option<u8> {
        self.log_page.or_else(|| match self.event_info {
            AsyncEventInfo::InvalidSubmissionQueue
            | AsyncEventInfo::InvalidCompletionQueue
            | AsyncEventInfo::DiagnosticFailure
            | AsyncEventInfo::PersistentInternalError
            | AsyncEventInfo::TransientInternalError
            | AsyncEventInfo::FirmwareImageLoadError => Some(0x01), // Error Information

            AsyncEventInfo::DeviceReliabilityDegraded
            | AsyncEventInfo::TemperatureAboveThreshold
            | AsyncEventInfo::MediaPlacedInReadOnly => Some(0x02), // SMART/Health Information

            AsyncEventInfo::NamespaceAttributeChanged => Some(0x04), // Changed Namespace List
            AsyncEventInfo::FirmwareActivationStarting => Some(0x03), // Firmware Slot Information
            AsyncEventInfo::TelemetryLogChanged => Some(0x07),       // Telemetry
            AsyncEventInfo::AsymmetricNamespaceAccessChange => Some(0x0C), // ANA
            AsyncEventInfo::PredictableLatencyEventAggregateLogChange => Some(0x0B),
            AsyncEventInfo::LbaStatusInformationAlert => Some(0x0E),
            AsyncEventInfo::EnduranceGroupEventAggregateLogChange => Some(0x0F),

            _ => None,
        })
    }
}

/// Event handler callback type.
pub type EventHandler = fn(&AsyncEvent) -> Result<()>;

/// Asynchronous event manager.
pub struct AsyncEventManager {
    /// Pending events queue
    pending_events: VecDeque<AsyncEvent>,
    /// Event handlers
    handlers: Vec<EventHandler>,
    /// Maximum outstanding AERs
    max_aers: u8,
    /// Current outstanding AERs
    outstanding_aers: AtomicU32,
    /// Event history for debugging
    event_history: Vec<AsyncEvent>,
    /// Maximum history size
    max_history: usize,
}

impl Default for AsyncEventManager {
    fn default() -> Self {
        Self {
            pending_events: VecDeque::new(),
            handlers: Vec::new(),
            max_aers: 4, // Default to 4 outstanding AERs
            outstanding_aers: AtomicU32::new(0),
            event_history: Vec::new(),
            max_history: 100,
        }
    }
}

impl AsyncEventManager {
    /// Create a new async event manager.
    pub fn new(max_aers: u8) -> Self {
        Self {
            max_aers,
            ..Default::default()
        }
    }

    /// Register an event handler.
    pub fn register_handler(&mut self, handler: EventHandler) {
        self.handlers.push(handler);
    }

    /// Clear all event handlers.
    pub fn clear_handlers(&mut self) {
        self.handlers.clear();
    }

    /// Process an async event from completion.
    pub fn process_event(&mut self, completion_dw0: u32) -> Result<()> {
        let event = AsyncEvent::from_completion(completion_dw0);

        // Add to history
        if self.event_history.len() >= self.max_history {
            self.event_history.remove(0);
        }
        self.event_history.push(event);

        // Queue the event
        self.pending_events.push_back(event);

        // Decrement outstanding AERs
        self.outstanding_aers.fetch_sub(1, Ordering::SeqCst);

        // Call handlers
        for handler in &self.handlers {
            handler(&event)?;
        }

        Ok(())
    }

    /// Get pending events.
    pub fn get_pending_events(&mut self) -> Vec<AsyncEvent> {
        self.pending_events.drain(..).collect()
    }

    /// Check if we need to submit more AERs.
    pub fn needs_aer_submission(&self) -> bool {
        self.outstanding_aers.load(Ordering::SeqCst) < self.max_aers as u32
    }

    /// Mark that an AER was submitted.
    pub fn aer_submitted(&self) {
        self.outstanding_aers.fetch_add(1, Ordering::SeqCst);
    }

    /// Get the number of outstanding AERs.
    pub fn outstanding_aer_count(&self) -> u32 {
        self.outstanding_aers.load(Ordering::SeqCst)
    }

    /// Build Async Event Request command.
    pub fn build_aer_command(&self, cmd_id: u16) -> Command {
        Command::async_event_request(cmd_id)
    }

    /// Get event history.
    pub fn get_history(&self) -> &[AsyncEvent] {
        &self.event_history
    }

    /// Clear event history.
    pub fn clear_history(&mut self) {
        self.event_history.clear();
    }

    /// Check if there are critical events pending.
    pub fn has_critical_events(&self) -> bool {
        self.pending_events.iter().any(|e| e.is_critical())
    }

    /// Get critical events.
    pub fn get_critical_events(&self) -> Vec<AsyncEvent> {
        self.pending_events
            .iter()
            .filter(|e| e.is_critical())
            .copied()
            .collect()
    }
}

/// Critical warning flags for SMART/Health.
#[derive(Debug, Clone, Copy)]
pub struct CriticalWarning {
    /// Available spare space below threshold
    pub spare_below_threshold: bool,
    /// Temperature above threshold or below under temperature threshold
    pub temperature_warning: bool,
    /// Device reliability degraded
    pub reliability_degraded: bool,
    /// Media in read-only mode
    pub read_only_mode: bool,
    /// Volatile memory backup failed
    pub volatile_backup_failed: bool,
    /// Persistent memory region in read-only mode
    pub pmr_read_only: bool,
}

impl CriticalWarning {
    /// Parse from critical warning byte.
    pub fn from_byte(byte: u8) -> Self {
        Self {
            spare_below_threshold: byte & 0x01 != 0,
            temperature_warning: byte & 0x02 != 0,
            reliability_degraded: byte & 0x04 != 0,
            read_only_mode: byte & 0x08 != 0,
            volatile_backup_failed: byte & 0x10 != 0,
            pmr_read_only: byte & 0x20 != 0,
        }
    }

    /// Convert to byte representation.
    pub fn to_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.spare_below_threshold { byte |= 0x01; }
        if self.temperature_warning { byte |= 0x02; }
        if self.reliability_degraded { byte |= 0x04; }
        if self.read_only_mode { byte |= 0x08; }
        if self.volatile_backup_failed { byte |= 0x10; }
        if self.pmr_read_only { byte |= 0x20; }
        byte
    }

    /// Check if any critical warning is active.
    pub fn is_critical(&self) -> bool {
        self.spare_below_threshold
            || self.temperature_warning
            || self.reliability_degraded
            || self.read_only_mode
            || self.volatile_backup_failed
            || self.pmr_read_only
    }
}