#!/bin/sh
# Build the maker's build of SolidCraft: no optimisation, no debug info, and rust-lld as the linker.
#
# This is the loop to work in - an edit to the Ledger tab rebuilds in seconds, where the release build takes about ten
# minutes. Measured on this machine: a cold devfast build is ~6m30s, then a no-op rebuild ~2s and an edit to a file in
# crates/ui-egui ~17s (compile + relink). Run Ledger against the result with:
#
#     ./scripts/dev-run.sh                      # build, then run Ledger on the devfast editor
#     SOLIDCRAFT_EXE=D:/cargo-target/devfast/solidcraft.exe ./ledger.exe     # by hand
#
# The binary lands in $CARGO_TARGET_DIR/devfast/solidcraft.exe (D:/cargo-target/devfast by default); the shipped
# binary still comes from `scripts/ship.sh`.
set -e
cd "$(dirname "$0")/.."
# One shared target dir for every build on this machine: a new dir per experiment is how D: gained 116 GB.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-D:/cargo-target}"
export RUSTFLAGS="${RUSTFLAGS:--Clinker=rust-lld}"
exec cargo build -p solidcraft --profile devfast "$@"
