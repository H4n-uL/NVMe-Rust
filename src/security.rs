//! NVMe Security and Sanitize module for NVMe 2.3 specification.

use alloc::vec::Vec;
use core::mem::size_of;

use crate::cmd::Command;
use crate::error::{Error, Result};

/// Sanitize action type.
#[derive(Debug, Clone, Copy)]
pub enum SanitizeAction {
    /// Exit failure mode
    ExitFailureMode = 0x00,
    /// Block erase sanitize
    BlockErase = 0x01,
    /// Overwrite sanitize
    Overwrite = 0x02,
    /// Crypto erase sanitize
    CryptoErase = 0x03,
}

/// Sanitize configuration options.
#[derive(Debug, Clone, Copy)]
pub struct SanitizeOptions {
    /// Sanitize action to perform
    pub action: SanitizeAction,
    /// Allow unrestricted sanitize exit
    pub allow_unrestricted_exit: bool,
    /// Overwrite pass count (for overwrite action)
    pub overwrite_pass_count: u8,
    /// Overwrite pattern invert between passes
    pub overwrite_invert_pattern: bool,
    /// No-deallocate after sanitize
    pub no_dealloc_after_sanitize: bool,
}

impl SanitizeOptions {
    /// Create options for block erase sanitize.
    pub fn block_erase() -> Self {
        Self {
            action: SanitizeAction::BlockErase,
            allow_unrestricted_exit: false,
            overwrite_pass_count: 0,
            overwrite_invert_pattern: false,
            no_dealloc_after_sanitize: false,
        }
    }

    /// Create options for crypto erase sanitize.
    pub fn crypto_erase() -> Self {
        Self {
            action: SanitizeAction::CryptoErase,
            allow_unrestricted_exit: false,
            overwrite_pass_count: 0,
            overwrite_invert_pattern: false,
            no_dealloc_after_sanitize: false,
        }
    }

    /// Create options for overwrite sanitize.
    pub fn overwrite(pass_count: u8, invert: bool) -> Self {
        Self {
            action: SanitizeAction::Overwrite,
            allow_unrestricted_exit: false,
            overwrite_pass_count: pass_count,
            overwrite_invert_pattern: invert,
            no_dealloc_after_sanitize: false,
        }
    }
}

/// Sanitize Per Namespace (SPN) configuration for NVMe 2.3.
#[derive(Debug, Clone)]
pub struct SanitizePerNamespace {
    /// Target namespace ID (0xFFFFFFFF for all namespaces)
    pub namespace_id: u32,
    /// Sanitize options
    pub options: SanitizeOptions,
    /// Pattern data for overwrite (if applicable)
    pub overwrite_pattern: Option<Vec<u8>>,
}

impl SanitizePerNamespace {
    /// Create SPN configuration for a specific namespace.
    pub fn for_namespace(namespace_id: u32, options: SanitizeOptions) -> Self {
        Self {
            namespace_id,
            options,
            overwrite_pattern: None,
        }
    }

    /// Create SPN configuration for all namespaces.
    pub fn for_all_namespaces(options: SanitizeOptions) -> Self {
        Self {
            namespace_id: 0xFFFFFFFF,
            options,
            overwrite_pattern: None,
        }
    }

    /// Set overwrite pattern.
    pub fn with_pattern(mut self, pattern: Vec<u8>) -> Self {
        self.overwrite_pattern = Some(pattern);
        self
    }

    /// Build sanitize command for namespace.
    pub fn build_command(&self, cmd_id: u16) -> Command {
        Command::sanitize(
            cmd_id,
            self.namespace_id,
            self.options.action as u8,
            self.options.allow_unrestricted_exit,
            self.options.overwrite_pass_count,
            self.options.overwrite_invert_pattern,
            self.options.no_dealloc_after_sanitize,
        )
    }
}

/// Sanitize status information.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SanitizeStatus {
    /// Sanitize progress (0-65535, where 65535 = 100%)
    pub progress: u16,
    /// Sanitize status flags
    pub flags: u16,
    /// Sanitize completion dword info
    pub cdw10_info: u32,
    /// Estimated time to complete in seconds
    pub time_for_overwrite: u32,
    /// Estimated time for block erase in seconds
    pub time_for_block_erase: u32,
    /// Estimated time for crypto erase in seconds
    pub time_for_crypto_erase: u32,
    /// Estimated time for overwrite with no-deallocate
    pub time_for_overwrite_nd: u32,
    /// Estimated time for block erase with no-deallocate
    pub time_for_block_erase_nd: u32,
    /// Estimated time for crypto erase with no-deallocate
    pub time_for_crypto_erase_nd: u32,
}

impl SanitizeStatus {
    /// Parse from log page data.
    pub fn from_log_data(data: &[u8]) -> Result<Self> {
        if data.len() < size_of::<Self>() {
            return Err(Error::InvalidBufferSize);
        }

        let status = unsafe {
            core::ptr::read_unaligned(data.as_ptr() as *const Self)
        };

        Ok(status)
    }

    /// Check if sanitize is in progress.
    pub fn is_in_progress(&self) -> bool {
        (self.flags & 0x07) == 0x02
    }

    /// Check if sanitize completed successfully.
    pub fn is_completed(&self) -> bool {
        (self.flags & 0x07) == 0x01
    }

    /// Check if sanitize failed.
    pub fn is_failed(&self) -> bool {
        (self.flags & 0x07) == 0x03
    }

    /// Get progress percentage.
    pub fn progress_percent(&self) -> f32 {
        (self.progress as f32 / 65535.0) * 100.0
    }
}

/// Security protocol identifiers.
#[derive(Debug, Clone, Copy)]
pub enum SecurityProtocol {
    /// Information protocol
    Information,
    /// TCG protocol (Trusted Computing Group)
    Tcg,
    /// NVMe protocol
    Nvme,
    /// Vendor specific
    VendorSpecific(u8),
}

impl SecurityProtocol {
    /// Convert to u8 value.
    pub fn to_u8(&self) -> u8 {
        match self {
            Self::Information => 0x00,
            Self::Tcg => 0x01,
            Self::Nvme => 0xEA,
            Self::VendorSpecific(val) => *val,
        }
    }
}

/// TCG (Trusted Computing Group) operations.
#[derive(Debug, Clone)]
pub struct TcgOperations {
    /// Security protocol
    protocol: SecurityProtocol,
    /// Comid for the operation
    comid: u16,
}

impl TcgOperations {
    /// Create new TCG operations handler.
    pub fn new() -> Self {
        Self {
            protocol: SecurityProtocol::Tcg,
            comid: 0,
        }
    }

    /// Build TCG discovery command.
    pub fn build_discovery_command(&self, cmd_id: u16, address: usize) -> Command {
        Command::security_receive(
            cmd_id,
            0, // namespace ID
            address,
            self.protocol.to_u8(),
            0x0001, // Discovery ComID
            512,    // Allocation length
        )
    }

    /// Build TCG properties command.
    pub fn build_properties_command(&self, cmd_id: u16, address: usize) -> Command {
        Command::security_receive(
            cmd_id,
            0,
            address,
            self.protocol.to_u8(),
            0x0002, // Properties ComID
            512,
        )
    }
}

/// Crypto erase configuration.
#[derive(Debug, Clone)]
pub struct CryptoEraseConfig {
    /// Target namespace
    pub namespace_id: u32,
    /// Crypto erase user data
    pub erase_user_data: bool,
    /// Crypto erase key
    pub crypto_key_identifier: Option<u32>,
}

impl CryptoEraseConfig {
    /// Create crypto erase config for namespace.
    pub fn for_namespace(namespace_id: u32) -> Self {
        Self {
            namespace_id,
            erase_user_data: true,
            crypto_key_identifier: None,
        }
    }

    /// Set crypto key identifier.
    pub fn with_key(mut self, key_id: u32) -> Self {
        self.crypto_key_identifier = Some(key_id);
        self
    }
}

/// Security manager for handling security operations.
pub struct SecurityManager {
    /// Current sanitize status
    sanitize_status: Option<SanitizeStatus>,
    /// Sanitize operations history
    sanitize_history: Vec<(u32, SanitizeAction, u64)>, // (namespace, action, timestamp)
    /// TCG operations handler
    tcg_ops: TcgOperations,
    /// Crypto erase configurations
    crypto_configs: Vec<CryptoEraseConfig>,
}

impl Default for SecurityManager {
    fn default() -> Self {
        Self {
            sanitize_status: None,
            sanitize_history: Vec::new(),
            tcg_ops: TcgOperations::new(),
            crypto_configs: Vec::new(),
        }
    }
}

impl SecurityManager {
    /// Create a new security manager.
    pub fn new() -> Self {
        Self::default()
    }

    /// Update sanitize status from log page.
    pub fn update_sanitize_status(&mut self, log_data: &[u8]) -> Result<()> {
        self.sanitize_status = Some(SanitizeStatus::from_log_data(log_data)?);
        Ok(())
    }

    /// Get current sanitize status.
    pub fn get_sanitize_status(&self) -> Option<&SanitizeStatus> {
        self.sanitize_status.as_ref()
    }

    /// Check if sanitize is allowed.
    pub fn can_sanitize(&self) -> bool {
        self.sanitize_status
            .map(|s| !s.is_in_progress())
            .unwrap_or(true)
    }

    /// Record sanitize operation.
    pub fn record_sanitize(&mut self, namespace_id: u32, action: SanitizeAction, timestamp: u64) {
        self.sanitize_history.push((namespace_id, action, timestamp));

        // Keep history limited
        if self.sanitize_history.len() > 100 {
            self.sanitize_history.remove(0);
        }
    }

    /// Get sanitize history.
    pub fn get_sanitize_history(&self) -> &[(u32, SanitizeAction, u64)] {
        &self.sanitize_history
    }

    /// Add crypto erase configuration.
    pub fn add_crypto_config(&mut self, config: CryptoEraseConfig) {
        self.crypto_configs.push(config);
    }

    /// Get crypto configurations.
    pub fn get_crypto_configs(&self) -> &[CryptoEraseConfig] {
        &self.crypto_configs
    }

    /// Build sanitize command.
    pub fn build_sanitize_command(
        &self,
        cmd_id: u16,
        namespace_id: u32,
        options: SanitizeOptions,
    ) -> Command {
        Command::sanitize(
            cmd_id,
            namespace_id,
            options.action as u8,
            options.allow_unrestricted_exit,
            options.overwrite_pass_count,
            options.overwrite_invert_pattern,
            options.no_dealloc_after_sanitize,
        )
    }

    /// Build security send command.
    pub fn build_security_send_command(
        &self,
        cmd_id: u16,
        namespace_id: u32,
        address: usize,
        protocol: SecurityProtocol,
        sp_specific: u16,
        transfer_length: u32,
    ) -> Command {
        Command::security_send(
            cmd_id,
            namespace_id,
            address,
            protocol.to_u8(),
            sp_specific,
            transfer_length,
        )
    }

    /// Build security receive command.
    pub fn build_security_receive_command(
        &self,
        cmd_id: u16,
        namespace_id: u32,
        address: usize,
        protocol: SecurityProtocol,
        sp_specific: u16,
        allocation_length: u32,
    ) -> Command {
        Command::security_receive(
            cmd_id,
            namespace_id,
            address,
            protocol.to_u8(),
            sp_specific,
            allocation_length,
        )
    }

    /// Estimate sanitize time for given action.
    pub fn estimate_sanitize_time(&self, action: SanitizeAction, no_dealloc: bool) -> Option<u32> {
        self.sanitize_status.map(|s| {
            match (action, no_dealloc) {
                (SanitizeAction::BlockErase, false) => s.time_for_block_erase,
                (SanitizeAction::BlockErase, true) => s.time_for_block_erase_nd,
                (SanitizeAction::Overwrite, false) => s.time_for_overwrite,
                (SanitizeAction::Overwrite, true) => s.time_for_overwrite_nd,
                (SanitizeAction::CryptoErase, false) => s.time_for_crypto_erase,
                (SanitizeAction::CryptoErase, true) => s.time_for_crypto_erase_nd,
                _ => 0,
            }
        })
    }
}