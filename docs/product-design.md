# lazycargo Product Positioning

`lazycargo` is a Rust project management TUI focused on solving Cargo ecosystem pain points that are awkward to handle with raw CLI commands alone.

It is not a generic Cargo command launcher. Rust developers already know `cargo build`, `cargo test`, and `cargo run`, and repetitive commands can be scripted. `lazycargo` should focus on workflows where the command is easy to run, but the surrounding context is hard to inspect, compare, remember, or combine.

## Product Thesis

`lazycargo` should make a single Rust project easier to understand and maintain.

The first target is one project or one workspace opened from its root. Multi-project dashboards can come later, but the initial product should deeply solve the problems inside the current project:

- Which package in this workspace am I operating on?
- Which dependencies are direct, transitive, duplicated, outdated, or conflicting?
- Which crate should I add, with which features?
- Why did dependency resolution fail?
- Why did build time suddenly increase?
- What did the last check/build/test actually report?

The core interaction should be similar to `lazygit`, `lazydocker`, and `pacseek`: left-side live lists, right-side detail tabs, bottom context hints, mouse support, and fast keyboard navigation.

## Non-Goals

- Do not make a thin wrapper around common commands.
- Do not make `test/build/run` the primary product identity.
- Do not edit `Cargo.toml` manually when `cargo add` or Cargo metadata can do the job.
- Do not turn workspace management into a file browser.
- Do not prioritize a multi-project dashboard before the single-project workflow is useful.

## Real Pain Points

### Priority Pain Points

The product should lean into three Rust-specific pain points that are hard to solve with a thin Cargo command wrapper:

- Target directory pressure: show source size, total `target/` size, and per-package cache estimates from the Workspace panel so users do not need to run `du -sh target/*` manually or use `cargo clean` blindly.
- Feature visibility: show dependency feature state with `[x]` enabled, `[ ]` available but disabled, and later `[-]` transitively enabled, so users can answer “is this feature actually on?” without leaving the TUI.
- Local dependency ancestry: show the selected dependency path in the Dependencies detail pane and provide fast inverse-tree access with `cargo tree -i <crate>` so users can answer “who pulled this package in?” quickly.

### 1. Dependency Management Guesswork

Scenario:

You want to add `serde_json`, but you do not remember the latest version or useful features. The normal workflow is:

```text
browser -> crates.io -> copy version/features -> editor -> Cargo.toml -> maybe browser again
```

Pain:

This breaks coding flow and makes feature selection easy to forget.

TUI solution:

Provide an integrated crates.io search and add flow:

```text
search serde_json
inspect versions/features/description/docs
select features
preview cargo add serde_json --features preserve_order
execute
refresh local dependency list
```

### 2. Dependency Conflict Panic

Scenario:

After adding a crate, Cargo reports:

```text
failed to select a version for `syn`
```

You do not know which dependency pulled in the incompatible version.

Pain:

`cargo tree` can produce hundreds of lines. Finding the relevant path manually is slow.

TUI solution:

Provide dependency tree and inverse-tree views:

```text
cargo tree
cargo tree -i syn
```

The TUI should highlight:

- duplicate versions
- conflict-related packages
- path from selected dependency back to workspace packages
- direct dependency responsible for the transitive pull

Later versions can suggest likely fixes, such as upgrading a direct dependency or changing features.

### 3. Build-Time Blindness

Scenario:

Build time grows from 5 seconds to 30 seconds, but it is unclear whether the cause is a new dependency, feature set, procedural macro, or local code change.

Pain:

Cargo gives enough data through build output and timings, but it is not easy to compare or scan in normal terminal output.

TUI solution:

Integrate build timing views:

```text
cargo build --timings
cargo check timings
recent build durations
slowest crate list
```

Show:

- recent command durations
- slowest crates
- target directory size
- diagnostics count
- selected package build/check status

### 4. Workspace Identity Loss

Scenario:

In a large workspace, it is easy to run a command against the wrong package or forget whether a command applies to the whole workspace.

Pain:

Cargo flags such as `--workspace`, `-p`, `--bin`, `--features`, and `--all-features` are powerful but easy to mix up.

TUI solution:

Make workspace scope explicit and persistent:

```text
scope: workspace
scope: package api
target: bin server
features: default,postgres
profile: dev/release
```

All actions should derive final Cargo commands from this context.

### 5. Documentation Lookup Friction

Scenario:

You want to inspect `tokio::spawn` or a crate feature, but `cargo doc --open` launches a browser and requires another search.

Pain:

Documentation lookup causes context switching and tab sprawl.

TUI solution:

Later versions can provide doc search:

```text
cargo doc
symbol search: spawn
open docs.rs or local rustdoc at matching item
```

This is valuable, but it should not be the first implementation target.

## 0.1 Product Definition

`lazycargo 0.1` should be:

```text
single-project Rust dependency management and Cargo insight TUI
```

The first version should prioritize:

1. Workspace/package context
2. Local dependency list
3. Dependency tree and inverse tree
4. crates.io search
5. cargo add preview/execute
6. cargo check/build diagnostics
7. basic build timing/history metrics

`test`, `build`, and `run` still matter, but they are supporting actions. They should use the current workspace/package/feature context instead of being the center of the product.

## Workspace Structure

The project is organized as a Cargo workspace so the package search experience can evolve independently from the project-management TUI:

```text
lazycargo/
├── Cargo.toml
├── lazycargo_tui/
│   ├── Cargo.toml
│   └── src/
└── lazycargo_search/
    ├── Cargo.toml
    └── src/
```

Responsibilities:

- `lazycargo_tui`: terminal UI, workspace/package context, build/test/check actions, dependency tree, command output, mouse/key handling, and the final `lazycargo` binary.
- `lazycargo_search`: crates.io search result parsing, search state, selected result detail, link extraction, and package detail formatting.

The release binary remains:

```bash
./target/release/lazycargo
```

## Primary Layout

The TUI should use a `lazydocker`/`lazygit` style layout:

```text
+-[1]-Workspace----------------------+ +-[Detail]-Output-Tree-Metrics------------+
| workspace                          | | selected package/dependency/build item   |
| pkg app                            | |                                          |
| pkg core                           | | command preview                          |
| pkg db                             | | cargo check -p api                       |
+------------------------------------+ |                                          |
+-[2]-Build Core---------------------+ | tree/details/diagnostics/timings          |
| check                              | |                                          |
| build                              | |                                          |
| test                               | |                                          |
| run                                | |                                          |
| clippy                             | |                                          |
+------------------------------------+
+-[3]-Dependencies-------------------+
| direct                             |
| > serde                            |
|   tokio                            |
| transitive                         |
|   syn 2.x                          |
+------------------------------------+
s: search page | enter: inspect/run | t: tree | i: inverse | /: filter | x: menu | q: quit
```

Search is an independent page, not a small dashboard panel:

```text
+-[Search]--------------------------+ +-[Search Detail]--------------------------+
| serde                             | | crate: serde_json                         |
+-----------------------------------+ | version: ...                              |
| > serde_json 1.x                  | | description: ...                          |
|   serde                           | | crates.io/docs.rs/repository links        |
|   serde_yaml                      | |                                          |
+-----------------------------------+ +------------------------------------------+
enter: cargo info | a: add preview | s: search again | q: back
```

### Left Panels

#### [1] Workspace

Purpose:

Persistent context selector.

Shows:

- workspace
- packages
- targets for selected package
- current feature/profile summary later

Actions:

- select workspace or package scope
- select target later
- scope all generated commands

#### [2] Build Core

Purpose:

Frequent project actions and diagnostics entry point.

Shows:

- check/build/test/run actions
- release/clippy/doc/update/clean actions
- diagnostics summary
- build history
- timings summary later

Actions:

- run `cargo check`
- run `cargo build`
- run `cargo test`
- run `cargo run`
- run `cargo clippy --all-targets`
- run `cargo doc --no-deps`
- run `cargo update`
- run `cargo clean`
- run `cargo build --timings`
- inspect errors/warnings

#### [3] Dependencies

Purpose:

Local dependency management and dependency graph entry point.

Shows:

- direct dependencies
- dependency kind: normal/dev/build
- features
- transitive dependencies later
- duplicate/conflict markers later

Actions:

- inspect selected dependency
- `t`: show dependency tree
- `i`: show inverse tree
- `u`: update selected dependency
- `r`: remove selected dependency later

#### Search Page

Purpose:

Terminal-native crate discovery, inspired by `pacseek`.

Shows:

- query input
- crates.io results
- selected crate detail
- crates.io/docs.rs links
- repository/homepage/documentation/license/features when `cargo info` returns them

Actions:

- search crates.io
- inspect crate with `cargo info <crate>`
- select features
- `a`: add as normal dependency
- `d`: add as dev dependency
- `b`: add as build dependency
- `q`: return to dashboard

### Right Panel Tabs

The right panel should have visible tabs with one active tab:

```text
[Detail] - Output - Tree - Metrics
Detail - [Output] - Tree - Metrics
Detail - Output - [Tree] - Metrics
Detail - Output - Tree - [Metrics]
```

Tabs:

- `Detail`: selected item explanation and command preview
- `Output`: raw command output
- `Tree`: dependency tree / inverse tree
- `Metrics`: build timings, command duration history, target directory size

The current active tab must be visually obvious.

## Waterfall Detail Contract

The right panel is not a generic text box. It is the single waterfall screen for the current selection, so every left-side row must have a predictable right-side representation.

### Dashboard Detail Tab

When focus is in `[1]-Workspace`, the selected row defines the command scope.

Right panel should show:

- Package identity: package name, version, license when available, Rust version/MSRV when available.
- Target summary: `lib`, `bin`, examples, tests, benches from Cargo metadata.
- Scope effect: which Cargo flags will be generated, such as `--workspace` or `-p <package>`.
- Disk/build hints: source size, target cache size when measurable, last successful build timestamp when tracked.
- Workspace members: full member list for orientation.

When focus is in `[2]-Build Core`, the selected row defines the build action.

Right panel has two modes:

- Idle/diagnostic mode: show the exact command preview, current scope, and last warnings/errors captured from that action.
- Running/finished mode: show the persistent command output stream, duration, and final exit status.

Build output requirements:

- Do not discard the last output when the user moves focus away and back.
- Preserve ANSI-colored Cargo/rustc output when feasible.
- Surface warning/error snapshots with `path:line` lines where Cargo provides them.
- `check`, `build`, `test`, `run`, `clippy`, `doc`, and `build --timings` are first-class actions.

When focus is in `[3]-Dependencies`, the selected row defines the dependency entity.

Right panel should show:

- Crate identity: name, local requirement, dependency kind, optional flag, enabled direct features.
- Feature state tree:
  - `[x] feature`: explicitly enabled by the current project.
  - `[ ] feature`: available but not enabled.
  - `[-] feature`: implied or transitively enabled.
- Local dependency graph context:
  - direct dependency path from workspace package to selected crate when available.
  - inverse tree output for "who pulled this in?".
  - duplicate/conflict marker when parsed from `cargo tree`.

### Search Page Contract

Search is a separate page, not a small dashboard panel.

When the cursor moves in the search result list, the right panel should show a lightweight network preview:

- name
- latest/max version
- description
- total/recent downloads when the source provides it
- crates.io URL
- docs/documentation URL
- repository/homepage URL when known

When the user presses `enter` on a search result, the right panel should append a deeper detail block from `cargo info <crate>`:

- license
- repository/homepage/documentation
- features returned by Cargo
- dependency summary
- later: recent version history and release dates

Search detail must remain readable as a waterfall pane:

- details scroll vertically.
- URL lines can be opened with keyboard shortcuts or mouse when mouse mode is enabled.
- `m` toggles terminal copy mode so visible text can be selected with the terminal.
- `y` copies the selected search detail through clipboard providers or OSC52.

### State Model Direction

The implementation should move toward explicit state objects instead of ad hoc strings:

```rust
pub struct CrateDetailInfo {
    pub name: String,
    pub desc: String,
    pub all_features: HashMap<String, FeatureStatus>,
    pub local_version: Option<String>,
    pub dependency_path: Vec<String>,
}

pub enum TerminalOutput {
    Idle { command: String, last_diagnostics: Vec<DiagnosticItem> },
    Running { stdout_buffer: Vec<String>, start_time: Instant },
    Finished { stdout_buffer: Vec<String>, exit_code: i32, duration: Duration },
}
```

The current code can still render from strings, but new functionality should first decide which structured field it owns, then render text from that field.

## Code Organization Contract

The TUI crate should avoid turning `ui.rs` into a single all-purpose file.

Current split:

- `lazycargo_tui/src/ui.rs`: terminal lifecycle, event loop, keyboard/mouse handling, layout composition, command execution bridge.
- `lazycargo_tui/src/ui/dashboard.rs`: dashboard/search waterfall text model, left-panel item models, filtering/scroll offset helpers, right-panel title state.
- `lazycargo_tui/src/metadata.rs`: Cargo metadata loading and project model.
- `lazycargo_tui/src/cargo_task.rs`: structured Cargo command construction.
- `lazycargo_search/src/lib.rs`: search state, result parsing, package detail formatting.

Future split targets:

- `ui/search_page.rs`: search-page rendering and interactions once the page grows beyond basic result/detail flow.
- `ui/waterfall.rs`: structured `TerminalOutput`, diagnostics snapshots, ANSI-aware log rendering.
- `ui/dependencies.rs`: feature tree, dependency path extraction, duplicate/conflict markers.
- `ui/workspace.rs`: package target details, source size, target cache attribution.

## Command Philosophy

Every operation should be derived from structured context:

```text
workspace/package scope
target
features
profile
selected dependency
selected search result
selected build action
```

Examples:

```text
cargo check --workspace
cargo check -p api --features postgres
cargo tree -p api
cargo tree -i syn
cargo add serde_json -p api --features preserve_order
cargo build -p api --timings
```

The TUI should expose the final command before executing it.

## MVP Implementation Plan

### Phase 1: Project Context

- Parse `cargo metadata`.
- Show workspace and packages.
- Maintain current scope.
- Generate Cargo command previews from current scope.

### Phase 2: Dependency Insight

- Show direct dependencies grouped by normal/dev/build.
- Run and display `cargo tree`.
- Run and display `cargo tree -i <crate>`.
- Highlight duplicate crate names/versions later.

### Phase 3: Crate Search and Add

- Search crates through `cargo search` or crates.io API.
- Show result list.
- Add dependency through `cargo add`.
- Support package scope with `-p`.
- Support features input first, feature picker later.

### Phase 4: Build Diagnostics

- Run `cargo check`.
- Parse/collect error and warning lines.
- Preserve command output per action.
- Show diagnostics summary.

### Phase 5: Build Metrics

- Run `cargo build --timings`.
- Track command durations.
- Show slowest crates when parsable.
- Show target directory size.

## Current Runnable Slice

The current implementation is the first usable vertical slice of the 0.1 architecture.

Implemented:

- Three dashboard panels: `[1]-Workspace`, `[2]-Build Core`, `[3]-Dependencies`.
- Search is a separate page opened with `s` or the menu.
- Right tabs: `Detail`, `Output`, `Tree`, `Metrics`.
- Mouse click focus for panels and rows.
- Persistent raw command output in the `Output` tab.
- Persistent dependency tree output in the `Tree` tab.
- Workspace/package scope selection:
  - `workspace` runs scoped build commands with `--workspace`.
  - `pkg <name>` runs scoped build/tree commands with `-p <name>`.
- Dependency actions:
  - `enter`: inspect selected dependency.
  - `t`: run `cargo tree` for current scope.
  - `i`: run `cargo tree -i <selected dependency>` for current scope.
- Search actions:
  - `s`: enter crates.io search input and open the large Search page.
  - `enter`: run `cargo search <query> --limit 100`.
  - results are filtered after search: only package names or descriptions containing the query are shown.
  - the result list shows package name and version; description is shown in the detail pane.
  - long result lists scroll with the current selection instead of being capped by the visible panel height.
  - arrow keys, `j`/`k`, or mouse click select a search result.
  - the Search detail pane shows selected crate name, version, description, crates.io link, and docs.rs link.
  - `enter` on a selected search result runs `cargo info <crate>` and adds repository/homepage/documentation/license/features when Cargo returns them.
  - `a`: preview `cargo add <selected crate>` using current package scope when selected.
  - `o`: open selected crate on crates.io through the system URL opener.
  - `d`: open docs/documentation through the system URL opener.
  - `g`: open repository after `cargo info` has returned one.
  - clicking a URL line in the detail pane opens it through the system URL opener.
  - `y`: copy the current detail pane text through system clipboard tools or terminal OSC52.
  - `m`: toggle terminal copy mode. Copy mode disables TUI mouse capture so visible waterfall text can be selected by the terminal; pressing `m` again restores mouse interaction.
  - clicking the detail pane focuses it; mouse wheel, `j`/`k`, arrow keys, PageUp, and PageDown scroll long detail/output content.
  - `q`: leave the Search page and return to the dashboard.
- Build actions:
  - `c`: run `cargo check`.
  - `b`: run `cargo build`.
  - Build panel `enter`: run selected action, including `test`, `run`, `build --release`, `clippy --all-targets`, `doc --no-deps`, `update`, `clean`, `build --timings`, and diagnostics.
- Metrics:
  - last status
  - target count
  - dependency count
  - diagnostic count
  - command duration history
  - target directory size

Manual test command:

```bash
cargo build --release
./target/release/lazycargo
```

Suggested smoke test:

1. Click or press `1`, select `workspace` or a package.
2. Press `2`, select a dependency, then press `enter`.
3. Press `t`, then switch to the `Tree` tab with `]`.
4. Press `i` on a dependency and confirm inverse tree output persists.
5. Press `s`, search a crate such as `serde`, select a result in the Search panel, then press `a`.
6. Press `4`, select `check`, press `enter`, then inspect `Output` and `Metrics`.

## Later

- Feature picker from `cargo metadata`.
- `cargo remove` integration.
- Outdated/vulnerability checks.
- rustdoc/doc search.
- Multi-project dashboard.
- Suggested dependency conflict fixes.
