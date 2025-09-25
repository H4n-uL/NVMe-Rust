//! NVMe Firmware Update module for NVMe 2.3 specification.

use alloc::vec::Vec;
use core::mem::size_of;

use crate::cmd::Command;
use crate::error::{Error, Result};

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

impl FirmwareSlotInfo {
    /// Parse from log page data.
    pub fn from_log_data(data: &[u8]) -> Result<Self> {
        if data.len() < size_of::<Self>() {
            return Err(Error::InvalidBufferSize);
        }

        let info = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const Self)
        };

        Ok(info)
    }

    /// Get active firmware slot.
    pub fn active_slot(&self) -> u8 {
        self.afi & 0x07
    }

    /// Get next reset slot.
    pub fn next_reset_slot(&self) -> u8 {
        (self.afi >> 4) & 0x07
    }

    /// Check if firmware slot is read-only.
    pub fn is_slot_readonly(&self, slot: u8) -> bool {
        if slot == 1 || slot > 7 {
            return true; // Slot 1 is always read-only
        }
        false
    }

    /// Get firmware revision for slot.
    pub fn get_revision(&self, slot: u8) -> Option<[u8; 8]> {
        if slot >= 1 && slot <= 7 {
            Some(self.firmware_revision[(slot - 1) as usize])
        } else {
            None
        }
    }
}

/// Firmware commit action.
#[derive(Debug, Clone, Copy)]
pub enum FirmwareCommitAction {
    /// Downloaded image replaces slot, no activation
    ReplaceNoActivate = 0,
    /// Downloaded image replaces slot and activates on next reset
    ReplaceActivateNextReset = 1,
    /// Activate firmware in specified slot on next reset
    ActivateNextReset = 2,
    /// Downloaded image replaces slot and activates immediately
    ReplaceActivateNow = 3,
}

/// Firmware update configuration.
#[derive(Debug, Clone)]
pub struct FirmwareUpdateConfig {
    /// Target firmware slot (2-7, slot 1 is read-only)
    pub target_slot: u8,
    /// Firmware commit action
    pub commit_action: FirmwareCommitAction,
    /// Boot partition ID (for boot partition updates)
    pub boot_partition_id: Option<u8>,
    /// Firmware image data
    pub firmware_image: Vec<u8>,
}

impl FirmwareUpdateConfig {
    /// Create new firmware update configuration.
    pub fn new(target_slot: u8, firmware_image: Vec<u8>) -> Result<Self> {
        if target_slot < 2 || target_slot > 7 {
            return Err(Error::FirmwareUpdateFailed);
        }

        Ok(Self {
            target_slot,
            commit_action: FirmwareCommitAction::ReplaceActivateNextReset,
            boot_partition_id: None,
            firmware_image,
        })
    }

    /// Set commit action.
    pub fn with_commit_action(mut self, action: FirmwareCommitAction) -> Self {
        self.commit_action = action;
        self
    }

    /// Set boot partition ID.
    pub fn with_boot_partition(mut self, bpid: u8) -> Self {
        self.boot_partition_id = Some(bpid);
        self
    }

    /// Get firmware image size.
    pub fn image_size(&self) -> usize {
        self.firmware_image.len()
    }

    /// Calculate number of chunks for download.
    pub fn chunk_count(&self, chunk_size: usize) -> usize {
        (self.firmware_image.len() + chunk_size - 1) / chunk_size
    }

    /// Get firmware chunk for download.
    pub fn get_chunk(&self, offset: usize, size: usize) -> Option<&[u8]> {
        let end = (offset + size).min(self.firmware_image.len());
        if offset < self.firmware_image.len() {
            Some(&self.firmware_image[offset..end])
        } else {
            None
        }
    }
}

/// Firmware activation method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirmwareActivation {
    /// No activation required
    None,
    /// NVM subsystem reset required
    NvmSubsystemReset,
    /// Controller reset required
    ControllerReset,
    /// Maximum time violation
    MaxTimeViolation,
}

/// Firmware update status.
#[derive(Debug, Clone, Copy)]
pub enum FirmwareUpdateStatus {
    /// Update not started
    NotStarted,
    /// Downloading firmware image
    Downloading {
        /// Download progress in bytes
        progress: u32,
        /// Total size in bytes
        total: u32
    },
    /// Verifying firmware image
    Verifying,
    /// Committing firmware
    Committing,
    /// Activation pending reset
    PendingActivation,
    /// Update completed successfully
    Completed,
    /// Update failed
    Failed(FirmwareUpdateError),
}

/// Firmware update error.
#[derive(Debug, Clone, Copy)]
pub enum FirmwareUpdateError {
    /// Invalid firmware slot
    InvalidSlot,
    /// Invalid firmware image
    InvalidImage,
    /// Firmware activation prohibited
    ActivationProhibited,
    /// Firmware activation requires reset
    RequiresReset(FirmwareActivation),
    /// Overlapping firmware range
    OverlappingRange,
    /// Insufficient space
    InsufficientSpace,
    /// Verification failed
    VerificationFailed,
    /// Download failed
    DownloadFailed,
    /// Commit failed
    CommitFailed,
}

/// Firmware update manager.
pub struct FirmwareManager {
    /// Current firmware slot info
    slot_info: Option<FirmwareSlotInfo>,
    /// Maximum firmware image size
    max_image_size: usize,
    /// Firmware download chunk size
    chunk_size: usize,
    /// Current update status
    update_status: FirmwareUpdateStatus,
    /// Update history
    update_history: Vec<(u8, u64, bool)>, // (slot, timestamp, success)
}

impl Default for FirmwareManager {
    fn default() -> Self {
        Self {
            slot_info: None,
            max_image_size: 16 * 1024 * 1024, // Default 16MB
            chunk_size: 4096,                  // Default 4KB chunks
            update_status: FirmwareUpdateStatus::NotStarted,
            update_history: Vec::new(),
        }
    }
}

impl FirmwareManager {
    /// Create new firmware manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Initialize with controller capabilities.
    pub fn init(&mut self, max_image_size: usize, chunk_size: usize) {
        self.max_image_size = max_image_size;
        self.chunk_size = chunk_size;
    }

    /// Update slot information from log page.
    pub fn update_slot_info(&mut self, log_data: &[u8]) -> Result<()> {
        self.slot_info = Some(FirmwareSlotInfo::from_log_data(log_data)?);
        Ok(())
    }

    /// Get current slot information.
    pub fn get_slot_info(&self) -> Option<&FirmwareSlotInfo> {
        self.slot_info.as_ref()
    }

    /// Validate firmware update configuration.
    pub fn validate_update(&self, config: &FirmwareUpdateConfig) -> Result<()> {
        // Check slot validity
        if config.target_slot < 2 || config.target_slot > 7 {
            return Err(Error::FirmwareUpdateFailed);
        }

        // Check image size
        if config.image_size() > self.max_image_size {
            return Err(Error::FirmwareUpdateFailed);
        }

        // Check if slot is in use
        if let Some(info) = &self.slot_info {
            if info.active_slot() == config.target_slot {
                // Cannot overwrite active slot directly
                if !matches!(
                    config.commit_action,
                    FirmwareCommitAction::ReplaceActivateNextReset
                        | FirmwareCommitAction::ReplaceActivateNow
                ) {
                    return Err(Error::FirmwareUpdateFailed);
                }
            }
        }

        Ok(())
    }

    /// Start firmware update.
    pub fn start_update(&mut self, config: &FirmwareUpdateConfig) -> Result<()> {
        self.validate_update(config)?;
        self.update_status = FirmwareUpdateStatus::Downloading {
            progress: 0,
            total: config.image_size() as u32,
        };
        Ok(())
    }

    /// Update download progress.
    pub fn update_progress(&mut self, bytes_downloaded: u32, total_bytes: u32) {
        self.update_status = FirmwareUpdateStatus::Downloading {
            progress: bytes_downloaded,
            total: total_bytes,
        };
    }

    /// Mark download complete and start verification.
    pub fn start_verification(&mut self) {
        self.update_status = FirmwareUpdateStatus::Verifying;
    }

    /// Mark verification complete and start commit.
    pub fn start_commit(&mut self) {
        self.update_status = FirmwareUpdateStatus::Committing;
    }

    /// Mark commit complete.
    pub fn complete_commit(&mut self, requires_activation: bool) {
        if requires_activation {
            self.update_status = FirmwareUpdateStatus::PendingActivation;
        } else {
            self.update_status = FirmwareUpdateStatus::Completed;
        }
    }

    /// Mark update as failed.
    pub fn fail_update(&mut self, error: FirmwareUpdateError) {
        self.update_status = FirmwareUpdateStatus::Failed(error);
    }

    /// Get current update status.
    pub fn get_status(&self) -> &FirmwareUpdateStatus {
        &self.update_status
    }

    /// Record update in history.
    pub fn record_update(&mut self, slot: u8, timestamp: u64, success: bool) {
        self.update_history.push((slot, timestamp, success));

        // Keep history limited
        if self.update_history.len() > 50 {
            self.update_history.remove(0);
        }
    }

    /// Get update history.
    pub fn get_history(&self) -> &[(u8, u64, bool)] {
        &self.update_history
    }

    /// Build firmware download command.
    pub fn build_download_command(
        &self,
        cmd_id: u16,
        address: usize,
        offset: u32,
        length: u32,
    ) -> Command {
        let num_dwords = (length + 3) / 4; // Convert bytes to dwords
        Command::firmware_image_download(cmd_id, address, num_dwords, offset / 4)
    }

    /// Build firmware commit command.
    pub fn build_commit_command(
        &self,
        cmd_id: u16,
        slot: u8,
        action: FirmwareCommitAction,
        bpid: Option<u8>,
    ) -> Command {
        Command::firmware_commit(cmd_id, slot, action as u8, bpid.unwrap_or(0))
    }

    /// Check if firmware activation is required.
    pub fn check_activation_required(&self, action: FirmwareCommitAction) -> FirmwareActivation {
        match action {
            FirmwareCommitAction::ReplaceNoActivate => FirmwareActivation::None,
            FirmwareCommitAction::ReplaceActivateNextReset
            | FirmwareCommitAction::ActivateNextReset => FirmwareActivation::ControllerReset,
            FirmwareCommitAction::ReplaceActivateNow => FirmwareActivation::NvmSubsystemReset,
        }
    }

    /// Get recommended chunk size.
    pub fn get_chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// Get maximum image size.
    pub fn get_max_image_size(&self) -> usize {
        self.max_image_size
    }
}