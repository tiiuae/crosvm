// Copyright 2026 The ChromiumOS Authors
// Use of this source code is governed by a BSD-style license that can be
// found in the LICENSE file.

mod bpmp_proxy;

use anyhow::Result;
use argh::FromArgValue;
use devices::virtio::VirtioDevice;
use devices::VirtioDeviceArgs;
use devices::VirtioDeviceModule;
#[cfg(any(target_os = "android", target_os = "linux"))]
use jail::JailConfig;
#[cfg(any(target_os = "android", target_os = "linux"))]
use minijail::Minijail;
use serde::Deserialize;
use serde::Serialize;

pub use crate::bpmp_proxy::VirtioBpmpModule;

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum VendorDeviceModule {
    Bpmp(VirtioBpmpModule),
}

impl VirtioDeviceModule for VendorDeviceModule {
    fn sort_name(&self) -> &'static str {
        match self {
            VendorDeviceModule::Bpmp(m) => m.sort_name(),
        }
    }

    fn create(&self, cx: &mut VirtioDeviceArgs<'_>) -> Result<Box<dyn VirtioDevice>> {
        match self {
            VendorDeviceModule::Bpmp(m) => m.create(cx),
        }
    }

    #[cfg(any(target_os = "android", target_os = "linux"))]
    fn create_jail(&self, jail_config: &JailConfig) -> Result<Option<Minijail>> {
        match self {
            VendorDeviceModule::Bpmp(m) => m.create_jail(jail_config),
        }
    }
}

/// Receives a string identifier from the command line and returns the corresponding module
/// if it exists.
impl FromArgValue for VendorDeviceModule {
    fn from_arg_value(value: &str) -> std::result::Result<Self, String> {
        match value {
            "bpmp" => Ok(Self::Bpmp(VirtioBpmpModule)),
            _ => Err(format!("unknown vendor device: {value}")),
        }
    }
}
