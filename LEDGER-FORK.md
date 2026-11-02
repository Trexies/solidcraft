# This fork: the Ledger tab

Skutchi Designs' fork of SolidCraft. Everything here is upstream `tangency-labs/solidcraft` at tag **v0.27.1** (the version
installed on the shop's machines) plus one feature on branch `ledger-boards-tab`.

## What it adds

A **LEDGER** ribbon tab, after Weldments. It lists the boards the shop stocks — read from
`D:\Ledger\data\boards.json`, which Ledger writes beside its own database — and puts the chosen board into the design:
a body at the sheet's size, in the shop's colour, with its physical material assigned.

Boards are stock, not sketches:

- the thickness is one of the shop's four classes (1/2, 3/4, 1, 1-1/8 in) and cannot be changed
- a board cannot be stretched past its sheet (4x8 or 5x9); cutting it smaller is allowed
- each board is tagged in the design's own metadata under `shop.ledger`, so the rule travels with the design and survives a
  reopen
- after any command that changes the design, the tag is checked by measurement (`inspect.measure`) and an illegal change is
  undone with a message in the status bar

## The diff

Four files, deliberately:

| File | What |
|---|---|
| `crates/ui-egui/src/ledger_boards.rs` | the file reader, the cards, the insertion, the guard (new file) |
| `crates/ui-egui/src/workspace.rs` | `SpecialtyTab { tab: "LEDGER", icon: "box" }` in `SPECIALTY_TABS` |
| `crates/ui-egui/src/toolbar.rs` | the tab draws its own panel and returns, after `ribbon_tabs(...)` |
| `crates/ui-egui/src/lib.rs` | `pub mod ledger_boards;`, and the guard called when a command changed the design |

Keeping it to those files is what makes an upstream merge a normal `git merge`.

## Building

```sh
export CARGO_TARGET_DIR="D:/sc-build"
cargo build -p solidcraft                 # debug: minutes, for iterating
cargo build --release -p solidcraft       # the deployable one; the link alone takes 7-20 minutes
```

Deploy by copying `D:/sc-build/release/solidcraft.exe` over `D:\SolidCraft\solidcraft.exe` with the editor closed. Keep the
untouched upstream binary beside it (`solidcraft-0.27.1-upstream.exe`); designs are the same format, so falling back is a file
copy.

## Updating from upstream

```sh
git fetch upstream
git merge v0.3x.0            # on ledger-boards-tab
```

The likely conflicts are the `SPECIALTY_TABS` list and the early return in `toolbar.rs`; both are a few lines.

## Licence

Upstream is MIT OR Apache-2.0 (see `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`). This fork keeps those terms.
