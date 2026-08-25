// Copyright 2026 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

//! Stub implementation of vendor virtio devices.
//! Downstream may replace this crate by pointing the `vendor_devices` workspace dependency to a
//! platform specific crate.

use anyhow::bail;
use anyhow::Result;
use devices::virtio::VirtioDevice;
use devices::VirtioDeviceArgs;
use devices::VirtioDeviceModule;
#[cfg(any(target_os = "android", target_os = "linux"))]
use jail::JailConfig;
#[cfg(any(target_os = "android", target_os = "linux"))]
use minijail::Minijail;
use serde::Deserialize;
use serde::Serialize;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VendorDeviceModule {
    Unsupported,
}

impl VirtioDeviceModule for VendorDeviceModule {
    fn sort_name(&self) -> &'static str {
        "vendor_device"
    }

    fn create(&self, _cx: &mut VirtioDeviceArgs<'_>) -> Result<Box<dyn VirtioDevice>> {
        bail!("no vendor devices are supported in this build")
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn create_jail(&self, _jail_config: &JailConfig) -> Result<Option<Minijail>> {
        bail!("no vendor devices are supported in this build")
    }
}

/// Receives the user provided argument from the command line and returns the corresponding
/// module.
/// Example implementation:
//      match arg {
//          "mydev" => Ok(VendorDeviceModule::MyDevice(MyDeviceModule)),
//          _ => bail!("unknown vendor device: {arg}"),
//      }
pub fn parse_vendor_device(arg: &str) -> Result<VendorDeviceModule> {
    bail!("no vendor devices are supported in this build: {arg}")
}
