#!/bin/sh
# Build the maker's build of SolidCraft: no optimisation, no debug info, and rust-lld as the linker.
#
# This is the loop to work in - an edit to the Ledger tab rebuilds in seconds, where the release build takes about ten
# minutes. Run Ledger against the result with:
#
#     SOLIDCRAFT_EXE=D:/sc-dev/devfast/solidcraft.exe ./ledger.exe
#
# The shipped binary still comes from `scripts/ship.sh`.
set -e
cd "$(dirname "$0")/.."
# One shared target dir for every build on this machine: a new dir per experiment is how D: gained 116 GB.
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-D:/cargo-target}"
export RUSTFLAGS="${RUSTFLAGS:--Clinker=rust-lld}"
exec cargo build -p solidcraft --profile devfast "$@"
