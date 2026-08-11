# lazycargo

[中文说明](README.zh-CN.md)

<p align="center">
  <a href="https://crates.io/crates/lazycargo-tui"><img src="https://img.shields.io/crates/v/lazycargo-tui.svg" alt="Crates.io"></a>
  <a href="https://github.com/lildengzi/lazycargo/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lildengzi/lazycargo" alt="MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.81+-blue" alt="Rust">
</p>

<p align="center">
  <img src="docs/pics/ProjectsIcon.png" alt="lazycargo project icon" width="256">
</p>

A lazygit-style TUI for Rust projects.

- **Workspace panel** — see which package you're operating on
- **Dependencies panel** — feature states, interactive tree, conflict detection
- **Build panel** — check/build/test with scope control
- **Search** — crates.io search + inspect in the TUI
- **target/ analysis** — per-crate disk usage, stale cleanup
- **Build telemetry** — auto-recorded duration history, slowest crates

## Install

```bash
cargo install lazycargo-tui
# or grab a binary from GitHub Releases
```

```bash
cd your-project && lazycargo
```

Also works as a command runner:

```bash
lazycargo check --workspace
lazycargo build -p lazycargo-tui --release
lazycargo add serde_json --features preserve_order
lazycargo config
```

## Screenshots

![Main workspace](docs/pics/mainpage.png)
![Feature state](docs/pics/Features.png)
![Dependency tree](docs/pics/Dependencies.png)
![Crates search](docs/pics/Searchpage.png)

## Key Bindings

`1` `2` `3` — panels | `Tab` — cycle | `[` `]` — tabs | `s` — search | `/` — filter | `c` — check | `b` — build | `t` — tree | `i` — inverse tree | `m` — copy mode | `q` — quit

## Acknowledgments

- [lazygit](https://github.com/jesseduffield/lazygit)
- [lazydocker](https://github.com/jesseduffield/lazydocker)
- [pacseek](https://github.com/moson-mo/pacseek)

## License

MIT

[![Star History Chart](https://api.star-history.com/svg?repos=lildengzi/lazycargo&type=Date)](https://www.star-history.com/#lildengzi/lazycargo&Date)
