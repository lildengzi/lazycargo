# lazycargo

[中文说明](README.zh-CN.md)

<p align="center">
  <img src="docs/pics/ProjectsIcon.png" alt="lazycargo project icon" width="128">
</p>

`lazycargo` is a Rust project management TUI for inspecting a Cargo workspace without leaving the terminal.

It is not meant to be a thin wrapper around `cargo build` or `cargo test`. The goal is to make the surrounding project context easier to see: package scope, dependency features, dependency trees, command output, build history, disk usage, and crates.io search.

## Screenshots

The main workspace keeps project scope, build actions, dependency state, and long-form output visible in one terminal workspace.

![Main workspace](docs/pics/mainpage.png)

Feature state and dependency detail views make Cargo feature resolution easier to inspect without leaving the terminal.

![Feature state](docs/pics/Features.png)

Dependency tree and inverse tree output are kept in the right-side waterfall pane for focused debugging.

![Dependency tree](docs/pics/Dependencies.png)

The crates.io search page provides a pacseek-style flow for finding packages, inspecting metadata, and opening docs or repository links.

![Crates search](docs/pics/Searchpage.png)

## Current Status

This project is in early development, but the main TUI workflow is already usable.

Implemented:

- `lazygit` / `lazydocker` style TUI layout.
- Workspace/package scope panel.
- Build Core panel for `check`, `build`, `test`, `run`, `clippy`, `doc`, `update`, `clean`, and `build --timings`.
- `cargo new <name>` from the Build Core panel through a small status-line input mode.
- Dependencies panel with direct dependency metadata and feature state markers:
  - `[x]` explicitly/default enabled
  - `[-]` enabled through Cargo resolution
  - `[ ]` available but disabled
- Context-aware right pane tabs:
  - Workspace: `Crate Info`, `Metrics`
  - Build Core: `Task Config`, `Live Output`
  - Dependencies: `Features`, `Dependency Tree`
- Persistent right-side waterfall output with ANSI color parsing, semantic coloring, scrollbar, keyboard scroll, mouse wheel, and draggable scrollbar.
- `cargo tree` and `cargo tree -i <crate>` output in the Dependency Tree view.
- Pacseek-style crates.io search page using `cargo search` and `cargo info`.
- Clickable crates.io/docs/repository links when available.
- Terminal copy mode with `m`, which releases mouse capture so visible text can be selected by the terminal.
- Clipboard copy for search detail with `y`.
- Non-blocking core Cargo actions for `check`, `build`, `test`, `run`, `tree`, and related Build Core tasks. The TUI remains responsive while the command is running, and output is shown when the command finishes.
- Limited mode outside Cargo projects. Running in a directory without `Cargo.toml` still opens the TUI so `cargo new <name>` can be used.

Not implemented yet:

- Structured interactive dependency graph nodes.
- Click-to-expand dependency tree.
- Conflict path highlighting.
- True streaming subprocess output while Cargo is still running. Core commands are non-blocking, but their full log is rendered after process completion.

## Install / Run

From the repository root:

```bash
cargo build --release
./target/release/lazycargo
```

Running `lazycargo` without arguments opens the TUI.

The binary also has a small command-preview CLI mode:

```bash
./target/release/lazycargo check --workspace
./target/release/lazycargo build -p lazycargo --release
./target/release/lazycargo add serde_json --features preserve_order
```

## Key Bindings

Main dashboard:

- `1`, `2`, `3`: focus Workspace, Build Core, Dependencies.
- `0`: focus the right waterfall pane.
- `Tab`: cycle focus.
- `Enter`: run or inspect the selected item.
- `[`, `]`: switch the current panel's right-side sub-tab.
- `j/k` or arrow keys: move selection; when right pane is focused, scroll output.
- `PgUp/PgDn`: scroll the right pane.
- `c`: run `cargo check`.
- `b`: run `cargo build`.
- Select `new project` in Build Core and press `Enter`: type a project name, then press `Enter` to run `cargo new <name>`.
- `t`: run `cargo tree`.
- `i`: run `cargo tree -i <selected dependency>`.
- `s`: open crates.io search page.
- `/`: filter current left panel.
- `m`: toggle terminal copy mode.
- `x`: show key help.
- `q`: quit, or return from search page.

Search page:

- Type a query and press `Enter` to search.
- Move through results with `j/k` or arrows.
- `Enter`: inspect the selected crate with `cargo info`.
- `a`: preview `cargo add <crate>`.
- `o`: open crates.io.
- `d`: open docs.
- `g`: open repository.
- `y`: copy selected detail text.
- `q`: return to dashboard.

## Workspace Layout

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

Crates:

- `lazycargo_tui`: terminal UI, workspace metadata, cargo actions, dependency tree output, command output, and final `lazycargo` binary.
- `lazycargo_search`: crates.io search state, parsing, and package detail formatting.

## Development

Recommended local checks:

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo build --release
```

Manual MVP smoke test:

```bash
cargo build --workspace --release
./target/release/lazycargo
```

Inside the TUI:

- Press `c` and confirm `cargo check` runs without freezing navigation.
- While `cargo check` is running, switch focus with `1`, `2`, `3`, `0`, and switch right tabs with `[` / `]`.
- Press `Ctrl+C` during a long-running Cargo task and confirm the task is killed.
- Press `b` and confirm build output appears in `Live Output` after completion.
- Press `t` and `i` from the dependency area and confirm dependency tree output appears.
- Press `s`, search for a crate, inspect it with `Enter`, and try `o` / `d` / `g` links.
- Press `m` and confirm terminal text selection works, then press `m` again to restore mouse interaction.

Outside a Cargo project:

```bash
cd /tmp
/path/to/lazycargo
```

Expected result: the TUI opens in limited mode. Use `Build Core -> new project` to create a Cargo project from that directory.

## Product Notes

The current product direction is documented in [docs/product-design.md](docs/product-design.md).

The most important Rust-specific pain points are:

- Tracking large `target/` directories without blindly running `cargo clean`.
- Understanding which dependency features are enabled and why.
- Finding who pulled a dependency into the project with `cargo tree -i`.

## Support

If `lazycargo` helps your Rust workflow, consider supporting development:

- GitHub Sponsors: https://github.com/sponsors/lildengzi
- More options: [docs/sponsor.md](docs/sponsor.md)

## License

MIT
