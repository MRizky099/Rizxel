# craft-ui (vendored)

A copy of the `craft-ui` crate from [storytold/craft-libs](https://github.com/storytold/craft-libs)
with two Rizxel changes on top. Rizxel uses its docking layer (`rizxel-ui/src/dock.rs`).
The crate's own description is in [UPSTREAM_README.md](UPSTREAM_README.md).

| | |
|---|---|
| Upstream | `storytold/craft-libs`, `crates/ui` |
| Base rev | `ae2f796b673f29cf3161f36eb6a8933f0a3fe028` |
| License | MIT OR Apache-2.0 ([LICENSE-MIT](LICENSE-MIT), [LICENSE-APACHE](LICENSE-APACHE), [NOTICE](NOTICE)) |
| Vendored | 2026-10-24, Phase 10d |

## Changes from upstream

Two commits, kept as patches for upstreaming:
[`patches/0001-docking-opt-in-Fill-tab-widths-and-scrolling-tab-str.patch`](patches/0001-docking-opt-in-Fill-tab-widths-and-scrolling-tab-str.patch)
and [`patches/0002-docking-compact-groups-with-a-grip.patch`](patches/0002-docking-compact-groups-with-a-grip.patch).

- `DockStyle::tab_sizing: TabSizing::{Natural, Fill}`: Fill stretches a group's tabs to share its
  strip evenly; a wider label keeps its width.
- `DockStyle::tab_overflow: TabOverflow::{Menu, Scroll}`: Scroll keeps label widths and scrolls
  the strip with the wheel or the ‹ › arrows. The active tab scrolls into view when it changes,
  and a dragged tab held near an end auto-scrolls. Tabs scrolled out of view are not drop targets.
- `DockStyle::scroll_left_label` / `scroll_right_label` for the arrows' accessible names.
- Pure helpers `tabs::fill` and `tabs::scroll` (with `TabScroll::visible`) next to `tabs::fit`.
- Unit tests in `src/tabs/tests.rs` and `src/docking/tests.rs`.

The defaults stay `Natural` and `Menu`, so other users of craft-ui see no change.

- `DockArea::compact(panel, GripSide::{Left, Top})` (patch 0002): while that panel is alone
  in a docked group, a 10 px grip on that edge replaces the captioned tab strip. The grip is
  the tab (drag, drop target, panel menu, accessible name) and shows the label as a tooltip;
  floating or sharing a group, the panel gets an ordinary strip. Nothing is compact unless an
  application asks. Tests: `compact_*` in `src/docking/tests.rs`.

Packaging differences (not code changes):

- `Cargo.toml` spells out the fields upstream inherits from its workspace, and `egui_kittest`
  has no `wgpu` feature.
- Upstream's integration tests (`tests/docking_workflows.rs`, `tests/visual_regression.rs`) are
  left out: they render with wgpu, which Rizxel's CI runners don't provide. They passed against
  this change before vendoring.
- `rustfmt.toml` and `clippy.toml` are upstream's, so `cargo fmt --all` and clippy treat the code
  as upstream does.

The crate is a member of the Rizxel workspace, so CI's `cargo fmt --all -- --check`,
`cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` cover it.

## Moving back to upstream

1. Open a PR to `storytold/craft-libs` from the patch above (`git am` it onto `ae2f796` or later).
2. Once it is merged, set `craft-ui` in the root `Cargo.toml` back to
   `{ git = "https://github.com/storytold/craft-libs", rev = "<merged rev>" }`.
3. Remove `"vendor/craft-ui"` from the workspace members, delete this folder and run the gate.
