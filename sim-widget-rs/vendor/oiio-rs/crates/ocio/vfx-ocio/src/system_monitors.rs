//! System monitor enumeration for ICC profile discovery.
//!
//! Provides access to the ICC monitor profiles from the operating system.
//! Works on Windows and macOS; on Linux returns empty list (no standard API).
//!
//! # Example
//!
//! ```no_run
//! use vfx_ocio::SystemMonitors;
//!
//! let monitors = SystemMonitors::get();
//! if monitors.is_supported() {
//!     for i in 0..monitors.num_monitors() {
//!         println!("{}: {}", monitors.monitor_name(i), monitors.profile_filepath(i));
//!     }
//! }
//! ```

use std::path::PathBuf;
use std::sync::OnceLock;

/// Information about a single monitor.
#[derive(Debug, Clone)]
struct MonitorInfo {
    name: String,
    icc_path: PathBuf,
}

/// System monitor enumeration singleton.
///
/// Provides access to ICC profiles for attached monitors.
/// OCIO parity: `SystemMonitors` class.
#[derive(Debug, Clone)]
pub struct SystemMonitors {
    monitors: Vec<MonitorInfo>,
}

static INSTANCE: OnceLock<SystemMonitors> = OnceLock::new();

impl SystemMonitors {
    /// Get the global SystemMonitors instance.
    ///
    /// OCIO parity: `SystemMonitors::Get()`.
    pub fn get() -> &'static SystemMonitors {
        INSTANCE.get_or_init(|| {
            let mut sm = SystemMonitors {
                monitors: Vec::new(),
            };
            sm.enumerate_monitors();
            sm
        })
    }

    /// Returns true if the OS provides ICC profiles for monitors.
    ///
    /// True on Windows and macOS, false on Linux and other platforms.
    /// OCIO parity: `isSupported()`.
    pub fn is_supported(&self) -> bool {
        !self.monitors.is_empty()
    }

    /// Get the number of active monitors.
    ///
    /// OCIO parity: `getNumMonitors()`.
    pub fn num_monitors(&self) -> usize {
        self.monitors.len()
    }

    /// Get the monitor name at the given index.
    ///
    /// Used as argument to `Config::instantiate_display_from_monitor_name()`.
    /// OCIO parity: `getMonitorName()`.
    ///
    /// # Panics
    /// Panics if index >= num_monitors().
    pub fn monitor_name(&self, index: usize) -> &str {
        &self.monitors[index].name
    }

    /// Get the ICC profile path for the monitor at the given index.
    ///
    /// OCIO parity: `getProfileFilepath()`.
    ///
    /// # Panics
    /// Panics if index >= num_monitors().
    pub fn profile_filepath(&self, index: usize) -> &str {
        self.monitors[index].icc_path.to_str().unwrap_or("")
    }

    /// Find ICC profile path by monitor name.
    ///
    /// Returns None if monitor not found.
    pub fn icc_profile_from_name(&self, name: &str) -> Option<&str> {
        self.monitors
            .iter()
            .find(|m| m.name == name)
            .map(|m| m.icc_path.to_str().unwrap_or(""))
    }

    /// The OS's displays and their profiles (gpu-info-rs `displays`, which holds the Windows and
    /// macOS calls OCIO's `SystemMonitor_windows.cpp` / `SystemMonitor_macos.cpp` make).
    fn enumerate_monitors(&mut self) {
        self.monitors = gpu_mem::displays::icc_profiles()
            .into_iter()
            .map(|d| MonitorInfo {
                name: d.name,
                icc_path: d.icc_path,
            })
            .collect();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_system_monitors_singleton() {
        let m1 = SystemMonitors::get();
        let m2 = SystemMonitors::get();
        assert!(std::ptr::eq(m1, m2));
    }

    #[test]
    fn test_system_monitors_api() {
        let monitors = SystemMonitors::get();

        // Always-true invariant that also holds on headless CI (count==0,
        // where the per-monitor loop below never runs and the old test
        // therefore asserted NOTHING): you cannot enumerate monitors on an
        // unsupported platform, so count>0 implies is_supported().
        let count = monitors.num_monitors();
        assert!(
            count == 0 || monitors.is_supported(),
            "num_monitors()={count} but is_supported()=false"
        );

        for i in 0..count {
            let name = monitors.monitor_name(i);
            let path = monitors.profile_filepath(i);
            assert!(!name.is_empty());
            assert!(!path.is_empty());
        }
    }
}
