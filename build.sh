#!/bin/bash
set -e -x

cargo build --features=vtpm,pci-hotplug,vendor-devices
sudo install -v target/debug/crosvm /usr/local/bin
