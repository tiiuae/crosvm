#!/bin/bash
set -e -x

cargo build --features=bpmp,vtpm,pci-hotplug
sudo install -v target/debug/crosvm /usr/local/bin
