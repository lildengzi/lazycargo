# lazycargo

[中文说明](README.zh-CN.md)

<p align="center">
  <a href="https://crates.io/crates/lazycargo"><img src="https://img.shields.io/crates/v/lazycargo.svg" alt="Crates.io"></a>
  <a href="https://github.com/lildengzi/lazycargo/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lildengzi/lazycargo" alt="MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.81+-blue" alt="Rust">
</p>

<p align="center">
  <img src="docs/pics/ProjectsIcon.png" alt="lazycargo project icon" width="256">
</p>

A Rust project TUI for poking around your Cargo workspace without leaving the terminal.

Not a thin wrapper around `cargo build` or `cargo test`. The goal is to surface the stuff that's annoying to dig up with raw CLI commands: package scope, dependency features, dependency trees, command output, build history, disk usage, and crates.io search.

## Screenshots

![Main workspace](docs/pics/mainpage.png)

![Feature state](docs/pics/Features.png)

![Dependency tree](docs/pics/Dependencies.png)

![Crates search](docs/pics/Searchpage.png)

## Status

Early development, but the main TUI flow is already usable day-to-day.

**Implemented:**

- `lazygit` / `lazydocker` style TUI layout.
- Workspace/package scope panel.
- Build Core panel: `check`, `build`, `test`, `run`, `clippy`, `doc`, `update`, `clean`, `build --timings`.
- `cargo new <name>` from Build Core via status-line input.
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
- Clickable crates.io/docs/repository links.
- Terminal copy mode with `m` (releases mouse capture for text selection).
- Clipboard copy for search detail with `y`.
- Non-blocking Cargo actions for `check`, `build`, `test`, `run`, `tree`, and related tasks. TUI stays responsive while commands run.
- Limited mode outside Cargo projects — opens the TUI anyway so `cargo new` works.

**Not implemented yet:**

- Structured interactive dependency graph nodes.
- Click-to-expand dependency tree.
- Conflict path highlighting.
- True streaming subprocess output while Cargo is still running (commands are non-blocking, full log renders after completion).

## Install / Run

```bash
cargo install lazycargo
lazycargo
```

Or build from source:

```bash
git clone https://github.com/lildengzi/lazycargo
cd lazycargo
cargo build --release
./target/release/lazycargo
```

Either way, put the binary in your `PATH` and you can launch it from anywhere — works just like `lazygit` or `lazydocker`.

Running without arguments opens the TUI. There's also a command-preview CLI mode:

```bash
lazycargo check --workspace
lazycargo build -p lazycargo --release
lazycargo add serde_json --features preserve_order
```

## Key Bindings

**Main dashboard:**

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

**Search page:**

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

- `lazycargo_tui`: terminal UI, workspace metadata, cargo actions, dependency tree, command output, and the final `lazycargo` binary.
- `lazycargo_search`: crates.io search state, parsing, and package detail formatting.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test
cargo build --release
```

## Acknowledgments

- [lazygit](https://github.com/jesseduffield/lazygit) — TUI layout inspiration.
- [lazydocker](https://github.com/jesseduffield/lazydocker) — more TUI layout inspiration.
- [pacseek](https://github.com/moson-mo/pacseek) — search page flow inspiration.

## License

MIT

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=lildengzi/lazycargo&type=Date)](https://www.star-history.com/#lildengzi/lazycargo&Date)
