#!/bin/sh
# The edit loop: build the maker's build, then run Ledger against it.
#
#     ./scripts/dev-run.sh
#
# Measured on this machine: a cold devfast build is ~6m30s; after that, editing a file in
# crates/ui-egui rebuilds and relinks in ~17s (no-op: ~2s). The shipped binary still comes from
# `scripts/ship.sh`; nothing here changes it.
#
# Ledger finds the editor through SOLIDCRAFT_EXE (see src/sc/host.rs::locate_exe), so this script
# sets it for the run only — your installed editor at D:/Ledger/SolidCraft/solidcraft.exe is left
# untouched, and pressing Calc Camlocks in this session uses the fresh devfast build.
set -e
cd "$(dirname "$0")/.."

./scripts/dev.sh "$@"

TARGET="${CARGO_TARGET_DIR:-D:/cargo-target}"
EXE="$TARGET/devfast/solidcraft.exe"
LEDGER="${LEDGER_EXE:-D:/Ledger/ledger.exe}"

if [ ! -f "$EXE" ]; then
    echo "dev-run: no devfast binary at $EXE — the build did not link it" >&2
    exit 2
fi
if [ ! -f "$LEDGER" ]; then
    echo "dev-run: no Ledger at $LEDGER (set LEDGER_EXE=...)" >&2
    exit 2
fi

echo "dev-run: editor $EXE"
echo "dev-run: ledger $LEDGER"
SOLIDCRAFT_EXE="$EXE" exec "$LEDGER"
