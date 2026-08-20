#!/bin/bash
set -e -x

cargo build --features=bpmp
sudo install -v target/debug/crosvm /usr/local/bin
