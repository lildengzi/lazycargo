# Testing Checklist

Use this before cutting a release or after touching TUI navigation, subprocess handling, search, target analysis, or dependency rendering.

## Automated Checks

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo build --workspace --release
```

## Startup

- Start in this repository and confirm the dashboard opens.
- Start in a non-Cargo directory and confirm limited mode opens.
- In limited mode, run `new project` from Build Core.
- Start in a single-crate project.
- Start in a multi-crate workspace.

## Navigation

- Use `1`, `2`, `3`, `0`, `Tab`, and `BackTab`.
- Switch right-pane tabs with `[` and `]`.
- Open and close the key menu with `x`.
- Filter each left panel with `/`.
- Resize the terminal to a narrow layout and confirm text does not overlap badly.

## Build Core

- Run `check`, `build`, `test`, and `clippy`.
- Confirm output streams while the process is running.
- Switch panels while a command is running, then return to Live Output.
- Kill a running command with `Ctrl+C`.
- Confirm build history updates after recordable commands.

## Dependencies

- Move through direct dependencies and inspect Features.
- Run `t` for `cargo tree`.
- Run `i` for inverse dependency tree on a selected dependency.
- Expand/collapse dependency tree nodes with `Enter`, `h`, and `l`.
- Confirm fallback raw tree output appears if parsing fails.

## Target Analysis

- Open Workspace `Target` tab.
- Refresh with `r`.
- Run dry-run stale cleanup with `d`.
- Run stale cleanup with `c` only in a disposable project.
- Confirm target size updates after refresh.

## Search

- Search for a known crate such as `serde`.
- Search with network unavailable or proxy misconfigured and confirm fallback/error messaging is understandable.
- Inspect a result with `Enter`.
- Open crates.io/docs/repository links with `o`, `d`, and `g`.
- Copy search detail with `y`.

## Release Artifacts

For GitHub Releases, confirm the tag build uploads:

- `lazycargo-linux-x86_64`
- `lazycargo-linux-x86_64.tar.gz`
- `lazycargo-macos-aarch64`
- `lazycargo-macos-aarch64.tar.gz`
- `lazycargo-windows-msvc-x86_64.exe`
- `lazycargo-windows-msvc-x86_64.zip`
- `lazycargo_*.deb`
- `lazycargo-*.rpm`
- `SHA256SUMS`

Smoke-test at least Linux locally:

```bash
./target/release/lazycargo
```
