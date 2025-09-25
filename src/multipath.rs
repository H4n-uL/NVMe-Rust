//! NVMe Multipath and Rapid Path Failure Recovery (RPFR) module for NVMe 2.3.

use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use spin::Mutex;

use crate::error::{Error, Result};

/// Path state for multipath.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
    /// Path is active and operational
    Active,
    /// Path is optimized for I/O
    Optimized,
    /// Path is non-optimized but functional
    NonOptimized,
    /// Path is inaccessible
    Inaccessible,
    /// Path is in transition state
    Transition,
    /// Path has failed
    Failed,
}

/// Asymmetric Namespace Access (ANA) state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnaState {
    /// Optimized state
    Optimized = 0x01,
    /// Non-optimized state
    NonOptimized = 0x02,
    /// Inaccessible state
    Inaccessible = 0x03,
    /// Persistent loss state
    PersistentLoss = 0x04,
    /// Change state
    Change = 0x0F,
}

/// Controller path information.
#[derive(Debug)]
pub struct ControllerPath {
    /// Controller ID
    pub controller_id: u16,
    /// Path ID
    pub path_id: u32,
    /// Transport address (e.g., PCIe address)
    pub transport_address: u64,
    /// Path state
    pub state: PathState,
    /// ANA state for this path
    pub ana_state: AnaState,
    /// Path priority (lower is better)
    pub priority: u8,
    /// Latency in microseconds
    pub latency_us: AtomicU32,
    /// Number of I/Os through this path
    pub io_count: AtomicU64,
    /// Number of errors on this path
    pub error_count: AtomicU32,
    /// Last access timestamp
    pub last_access: AtomicU64,
}

impl ControllerPath {
    /// Create a new controller path.
    pub fn new(controller_id: u16, path_id: u32, transport_address: u64) -> Self {
        Self {
            controller_id,
            path_id,
            transport_address,
            state: PathState::Active,
            ana_state: AnaState::Optimized,
            priority: 0,
            latency_us: AtomicU32::new(0),
            io_count: AtomicU64::new(0),
            error_count: AtomicU32::new(0),
            last_access: AtomicU64::new(0),
        }
    }

    /// Check if path is usable.
    pub fn is_usable(&self) -> bool {
        matches!(
            self.state,
            PathState::Active | PathState::Optimized | PathState::NonOptimized
        ) && !matches!(
            self.ana_state,
            AnaState::Inaccessible | AnaState::PersistentLoss
        )
    }

    /// Update path metrics after I/O completion.
    pub fn update_metrics(&self, latency_us: u32, success: bool, timestamp: u64) {
        // Update latency with exponential moving average
        let old_latency = self.latency_us.load(Ordering::Relaxed);
        let new_latency = (old_latency * 7 + latency_us) / 8;
        self.latency_us.store(new_latency, Ordering::Relaxed);

        self.io_count.fetch_add(1, Ordering::Relaxed);
        if !success {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
        self.last_access.store(timestamp, Ordering::Relaxed);
    }

    /// Get path score for selection (lower is better).
    pub fn get_score(&self) -> u32 {
        if !self.is_usable() {
            return u32::MAX;
        }

        let mut score = self.priority as u32 * 1000;

        // Add latency component
        score += self.latency_us.load(Ordering::Relaxed);

        // Add error rate component
        let io_count = self.io_count.load(Ordering::Relaxed);
        if io_count > 0 {
            let error_count = self.error_count.load(Ordering::Relaxed);
            let error_rate = (error_count * 100) / io_count as u32;
            score += error_rate * 100;
        }

        // Prefer optimized paths
        match self.ana_state {
            AnaState::Optimized => {}
            AnaState::NonOptimized => score += 5000,
            _ => score = u32::MAX,
        }

        score
    }
}

/// Path failure recovery configuration.
#[derive(Debug, Clone)]
pub struct RpfrConfig {
    /// Enable Rapid Path Failure Recovery
    pub enabled: bool,
    /// Maximum retry count before marking path as failed
    pub max_retries: u32,
    /// Path failure timeout in milliseconds
    pub failure_timeout_ms: u32,
    /// Recovery timeout in milliseconds
    pub recovery_timeout_ms: u32,
    /// Enable automatic path failback
    pub auto_failback: bool,
    /// Path health check interval in seconds
    pub health_check_interval_sec: u32,
}

impl Default for RpfrConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_retries: 3,
            failure_timeout_ms: 5000,
            recovery_timeout_ms: 30000,
            auto_failback: true,
            health_check_interval_sec: 60,
        }
    }
}

/// ANA group information.
#[derive(Debug, Clone)]
pub struct AnaGroup {
    /// ANA group ID
    pub group_id: u32,
    /// Namespaces in this group
    pub namespaces: Vec<u32>,
    /// ANA state per controller
    pub states: BTreeMap<u16, AnaState>,
}

impl AnaGroup {
    /// Create a new ANA group.
    pub fn new(group_id: u32) -> Self {
        Self {
            group_id,
            namespaces: Vec::new(),
            states: BTreeMap::new(),
        }
    }

    /// Add namespace to group.
    pub fn add_namespace(&mut self, nsid: u32) {
        if !self.namespaces.contains(&nsid) {
            self.namespaces.push(nsid);
        }
    }

    /// Set ANA state for controller.
    pub fn set_state(&mut self, controller_id: u16, state: AnaState) {
        self.states.insert(controller_id, state);
    }

    /// Get ANA state for controller.
    pub fn get_state(&self, controller_id: u16) -> Option<AnaState> {
        self.states.get(&controller_id).copied()
    }
}

/// Path selector strategy.
#[derive(Debug, Clone, Copy)]
pub enum PathSelector {
    /// Round-robin between paths
    RoundRobin,
    /// Select path with lowest latency
    LowestLatency,
    /// Select path with least I/O count
    LeastIo,
    /// Select based on path score
    BestScore,
    /// Use priority-based selection
    Priority,
}

/// Multipath I/O controller.
pub struct MultipathController {
    /// Available paths
    paths: Mutex<Vec<ControllerPath>>,
    /// Active path index
    active_path: AtomicU32,
    /// RPFR configuration
    rpfr_config: RpfrConfig,
    /// Path selection strategy
    path_selector: PathSelector,
    /// ANA groups
    ana_groups: Mutex<BTreeMap<u32, AnaGroup>>,
    /// Failed paths pending recovery
    failed_paths: Mutex<Vec<u32>>,
    /// Last path selection timestamp
    last_selection: AtomicU64,
}

impl MultipathController {
    /// Create a new multipath controller.
    pub fn new(rpfr_config: RpfrConfig, path_selector: PathSelector) -> Self {
        Self {
            paths: Mutex::new(Vec::new()),
            active_path: AtomicU32::new(0),
            rpfr_config,
            path_selector,
            ana_groups: Mutex::new(BTreeMap::new()),
            failed_paths: Mutex::new(Vec::new()),
            last_selection: AtomicU64::new(0),
        }
    }

    /// Add a controller path.
    pub fn add_path(&self, path: ControllerPath) {
        let mut paths = self.paths.lock();
        paths.push(path);
    }

    /// Remove a controller path.
    pub fn remove_path(&self, path_id: u32) -> Result<()> {
        let mut paths = self.paths.lock();
        if let Some(pos) = paths.iter().position(|p| p.path_id == path_id) {
            paths.remove(pos);
            Ok(())
        } else {
            Err(Error::PathFailure)
        }
    }

    /// Select the best path based on configured strategy.
    pub fn select_path(&self, _namespace_id: u32, timestamp: u64) -> Result<u32> {
        let paths = self.paths.lock();
        if paths.is_empty() {
            return Err(Error::PathFailure);
        }

        // Filter usable paths
        let usable_paths: Vec<_> = paths
            .iter()
            .enumerate()
            .filter(|(_, p)| p.is_usable())
            .collect();

        if usable_paths.is_empty() {
            return Err(Error::PathFailure);
        }

        let selected_idx = match self.path_selector {
            PathSelector::RoundRobin => {
                let current = self.active_path.load(Ordering::Relaxed) as usize;
                (current + 1) % usable_paths.len()
            }
            PathSelector::LowestLatency => {
                usable_paths
                    .iter()
                    .min_by_key(|(_, p)| p.latency_us.load(Ordering::Relaxed))
                    .map(|(idx, _)| *idx)
                    .unwrap_or(0)
            }
            PathSelector::LeastIo => {
                usable_paths
                    .iter()
                    .min_by_key(|(_, p)| p.io_count.load(Ordering::Relaxed))
                    .map(|(idx, _)| *idx)
                    .unwrap_or(0)
            }
            PathSelector::BestScore => {
                usable_paths
                    .iter()
                    .min_by_key(|(_, p)| p.get_score())
                    .map(|(idx, _)| *idx)
                    .unwrap_or(0)
            }
            PathSelector::Priority => {
                usable_paths
                    .iter()
                    .min_by_key(|(_, p)| p.priority)
                    .map(|(idx, _)| *idx)
                    .unwrap_or(0)
            }
        };

        let selected_path = &usable_paths[selected_idx].1;
        self.active_path.store(selected_path.path_id, Ordering::Relaxed);
        self.last_selection.store(timestamp, Ordering::Relaxed);

        Ok(selected_path.path_id)
    }

    /// Handle path failure with RPFR.
    pub fn handle_path_failure(&self, path_id: u32, timestamp: u64) -> Result<u32> {
        if !self.rpfr_config.enabled {
            return Err(Error::PathFailure);
        }

        // Mark path as failed
        {
            let mut paths = self.paths.lock();
            if let Some(path) = paths.iter_mut().find(|p| p.path_id == path_id) {
                path.state = PathState::Failed;
                path.error_count.fetch_add(1, Ordering::Relaxed);
            }
        }

        // Add to failed paths for recovery
        {
            let mut failed = self.failed_paths.lock();
            if !failed.contains(&path_id) {
                failed.push(path_id);
            }
        }

        // Select alternate path
        self.select_path(0, timestamp)
    }

    /// Attempt to recover failed paths.
    pub fn recover_failed_paths(&self, timestamp: u64) -> Vec<u32> {
        let mut recovered = Vec::new();
        let mut failed_paths = self.failed_paths.lock();
        let mut paths = self.paths.lock();

        failed_paths.retain(|&path_id| {
            if let Some(path) = paths.iter_mut().find(|p| p.path_id == path_id) {
                // Check if enough time has passed for recovery
                let last_access = path.last_access.load(Ordering::Relaxed);
                let elapsed_ms = (timestamp - last_access) / 1000;

                if elapsed_ms >= self.rpfr_config.recovery_timeout_ms as u64 {
                    // Attempt recovery
                    path.state = PathState::Active;
                    path.error_count.store(0, Ordering::Relaxed);
                    recovered.push(path_id);
                    false // Remove from failed list
                } else {
                    true // Keep in failed list
                }
            } else {
                false // Path no longer exists
            }
        });

        recovered
    }

    /// Update ANA group information.
    pub fn update_ana_group(&self, group: AnaGroup) {
        let mut groups = self.ana_groups.lock();
        groups.insert(group.group_id, group);
    }

    /// Get ANA state for namespace and controller.
    pub fn get_ana_state(&self, namespace_id: u32, controller_id: u16) -> Option<AnaState> {
        let groups = self.ana_groups.lock();

        for group in groups.values() {
            if group.namespaces.contains(&namespace_id) {
                return group.get_state(controller_id);
            }
        }

        None
    }

    /// Get path statistics.
    pub fn get_path_stats(&self, path_id: u32) -> Option<PathStats> {
        let paths = self.paths.lock();
        paths.iter().find(|p| p.path_id == path_id).map(|p| PathStats {
            path_id: p.path_id,
            controller_id: p.controller_id,
            state: p.state,
            ana_state: p.ana_state,
            io_count: p.io_count.load(Ordering::Relaxed),
            error_count: p.error_count.load(Ordering::Relaxed),
            average_latency_us: p.latency_us.load(Ordering::Relaxed),
        })
    }

    /// Get all path statistics.
    pub fn get_all_path_stats(&self) -> Vec<PathStats> {
        let paths = self.paths.lock();
        paths
            .iter()
            .map(|p| PathStats {
                path_id: p.path_id,
                controller_id: p.controller_id,
                state: p.state,
                ana_state: p.ana_state,
                io_count: p.io_count.load(Ordering::Relaxed),
                error_count: p.error_count.load(Ordering::Relaxed),
                average_latency_us: p.latency_us.load(Ordering::Relaxed),
            })
            .collect()
    }

    /// Get RPFR configuration.
    pub fn get_rpfr_config(&self) -> &RpfrConfig {
        &self.rpfr_config
    }

    /// Update RPFR configuration.
    pub fn update_rpfr_config(&mut self, config: RpfrConfig) {
        self.rpfr_config = config;
    }
}

/// Path statistics.
#[derive(Debug, Clone, Copy)]
pub struct PathStats {
    /// Path ID
    pub path_id: u32,
    /// Controller ID
    pub controller_id: u16,
    /// Current path state
    pub state: PathState,
    /// Current ANA state
    pub ana_state: AnaState,
    /// Total I/O count
    pub io_count: u64,
    /// Total error count
    pub error_count: u32,
    /// Average latency in microseconds
    pub average_latency_us: u32,
}
