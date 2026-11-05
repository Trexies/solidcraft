#!/bin/sh
# Build the binary the shop runs and put it where Ledger looks for it.
#
# Ten minutes on this machine, so it is the last step, not the loop: work in `scripts/dev.sh` and ship when it is right.
set -e
cd "$(dirname "$0")/.."
# One shared target dir for every build on this machine: a new dir per experiment is how D: gained 116 GB.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-D:/cargo-target}"
cargo build --release -p solidcraft "$@"
cp "$CARGO_TARGET_DIR/release/solidcraft.exe" "D:/Ledger/SolidCraft/solidcraft.exe"
ls -la "D:/Ledger/SolidCraft/solidcraft.exe"
