# lazycargo

<p align="center">
  <a href="https://crates.io/crates/lazycargo-tui"><img src="https://img.shields.io/crates/v/lazycargo-tui.svg" alt="Crates.io"></a>
  <a href="https://github.com/lildengzi/lazycargo/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lildengzi/lazycargo" alt="MIT"></a>
  <img src="https://img.shields.io/badge/rust-1.81+-blue" alt="Rust">
</p>

<p align="center">
  <img src="docs/pics/ProjectsIcon.png" alt="lazycargo 项目图标" width="256">
</p>

一个 lazygit 风格的 Rust 项目 TUI。

- **Workspace 面板** — 当前操作的是 workspace 还是哪个 package，一目了然
- **依赖面板** — feature 状态、交互式树、冲突检测
- **构建面板** — 带作用域控制的 check/build/test
- **搜索** — 在 TUI 里搜 crates.io 并查看详情
- **target/ 分析** — 按 crate 看磁盘占用，清理过期缓存
- **构建历史** — 自动记录每次耗时，展示最慢 crate

## 安装

```bash
cargo install lazycargo-tui
# 或从 GitHub Releases 下载二进制
```

```bash
cd your-project && lazycargo
```

也支持命令行模式：

```bash
lazycargo check --workspace
lazycargo build -p lazycargo-tui --release
lazycargo add serde_json --features preserve_order
lazycargo config
```

## 界面预览

![主工作台](docs/pics/mainpage.png)
![Feature 状态](docs/pics/Features.png)
![依赖树](docs/pics/Dependencies.png)
![包搜索](docs/pics/Searchpage.png)

## 快捷键

`1` `2` `3` — 切换面板 | `Tab` — 循环焦点 | `[` `]` — 切换标签 | `s` — 搜索 | `/` — 过滤 | `c` — check | `b` — build | `t` — 依赖树 | `i` — 反向树 | `m` — 复制模式 | `q` — 退出

## 致谢

- [lazygit](https://github.com/jesseduffield/lazygit)
- [lazydocker](https://github.com/jesseduffield/lazydocker)
- [pacseek](https://github.com/moson-mo/pacseek)

## 许可证

MIT

[![Star History Chart](https://api.star-history.com/svg?repos=lildengzi/lazycargo&type=Date)](https://www.star-history.com/#lildengzi/lazycargo&Date)
