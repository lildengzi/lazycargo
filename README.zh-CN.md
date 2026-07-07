# lazycargo

`lazycargo` 是一个面向 Rust 项目的终端 TUI 工作台，用来在不离开终端的情况下查看和管理 Cargo workspace。

它不是简单的 `cargo build` / `cargo test` 命令包装器。它的重点是把日常开发里难以扫清的上下文集中展示出来：当前 package 作用域、依赖 features、依赖树、命令输出、构建历史、磁盘占用和 crates.io 搜索。

## 当前状态

项目还处于早期开发阶段，但主 TUI 流程已经可以日常试用。

已实现：

- 类似 `lazygit` / `lazydocker` 的紧凑 TUI 布局。
- Workspace / package 作用域面板。
- Build Core 面板，支持 `check`、`build`、`test`、`run`、`clippy`、`doc`、`update`、`clean`、`build --timings`。
- Dependencies 面板，展示直接依赖元数据和 feature 状态：
  - `[x]` 显式或默认启用
  - `[-]` 由 Cargo resolve 结果启用
  - `[ ]` 可用但未启用
- 上下文感知右侧标签页：
  - Workspace: `Crate Info`, `Metrics`
  - Build Core: `Task Config`, `Live Output`
  - Dependencies: `Features`, `Dependency Tree`
- 右侧瀑布屏支持持久输出、ANSI 颜色解析、语义着色、滚动条、键盘滚动、鼠标滚轮和拖动滚动条。
- `cargo tree` 和 `cargo tree -i <crate>` 输出到 Dependency Tree 视图。
- 类 pacseek 的 crates.io 搜索页，基于 `cargo search` 和 `cargo info`。
- 支持打开 crates.io / docs / repository 链接。
- `m` 进入终端复制模式，释放鼠标捕获，方便直接拖选可见文本。
- `y` 复制搜索详情。
- 核心 Cargo 动作已经非阻塞，包括 `check`、`build`、`test`、`run`、`tree` 和 Build Core 里的相关任务。命令执行时 TUI 仍然可以移动焦点、切换面板，命令结束后显示完整输出。
- 非 Cargo 项目目录启动时会直接打印明确的 `FATAL: cargo metadata failed...` 错误并退出，不再打开空面板。

尚未完成：

- 结构化的交互式依赖图节点。
- 点击展开/折叠依赖树。
- 依赖冲突路径高亮。
- Cargo 子进程运行中的真正实时流式输出。当前核心命令已经非阻塞，但完整日志会在进程结束后渲染。

## 安装与运行

在仓库根目录执行：

```bash
cargo build --release
./target/release/lazycargo
```

不带参数运行 `lazycargo` 会打开 TUI。

也可以使用当前的命令预览 CLI：

```bash
./target/release/lazycargo check --workspace
./target/release/lazycargo build -p lazycargo --release
./target/release/lazycargo add serde_json --features preserve_order
```

## 快捷键

主界面：

- `1`, `2`, `3`: 聚焦 Workspace、Build Core、Dependencies。
- `0`: 聚焦右侧瀑布屏。
- `Tab`: 循环切换焦点。
- `Enter`: 执行或检查当前选中项。
- `[`, `]`: 切换当前面板对应的右侧子标签。
- `j/k` 或方向键：移动选择；右侧聚焦时滚动输出。
- `PgUp/PgDn`: 滚动右侧瀑布屏。
- `c`: 执行 `cargo check`。
- `b`: 执行 `cargo build`。
- `t`: 执行 `cargo tree`。
- `i`: 执行 `cargo tree -i <selected dependency>`。
- `s`: 打开 crates.io 搜索页。
- `/`: 过滤当前左侧面板。
- `m`: 切换终端复制模式。
- `x`: 显示快捷键帮助。
- `q`: 退出，或从搜索页返回。

搜索页：

- 输入关键词后按 `Enter` 搜索。
- `j/k` 或方向键移动结果选择。
- `Enter`: 使用 `cargo info` 检查当前 crate。
- `a`: 预览 `cargo add <crate>`。
- `o`: 打开 crates.io。
- `d`: 打开文档。
- `g`: 打开仓库。
- `y`: 复制当前详情文本。
- `q`: 返回主界面。

## 工作区结构

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

crate 分工：

- `lazycargo_tui`: 终端 UI、workspace 元数据、cargo 动作、依赖树输出、命令输出，以及最终 `lazycargo` 二进制。
- `lazycargo_search`: crates.io 搜索状态、结果解析和包详情格式化。

## 开发

推荐本地检查：

```bash
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets
cargo build --workspace --release
```

MVP 手动冒烟测试：

```bash
cargo build --workspace --release
./target/release/lazycargo
```

进入 TUI 后：

- 按 `c`，确认 `cargo check` 执行时界面不会卡死。
- `cargo check` 运行期间，用 `1`、`2`、`3`、`0` 切换焦点，用 `[` / `]` 切换右侧标签。
- 长任务运行期间按 `Ctrl+C`，确认当前 Cargo 子进程会被杀掉。
- 按 `b`，确认 build 结束后输出出现在 `Live Output`。
- 在依赖区域按 `t` 和 `i`，确认依赖树输出正常出现。
- 按 `s` 搜索 crate，选中结果后按 `Enter` 查看详情，再尝试 `o` / `d` / `g` 打开链接。
- 按 `m`，确认可以用终端直接拖选文字；再按一次 `m` 恢复鼠标交互。

在非 Cargo 项目目录：

```bash
cd /tmp
/path/to/lazycargo
```

预期结果：程序在打开 TUI 前退出，并打印 `FATAL: cargo metadata failed...`。

## 产品方向

产品定位见 [docs/product-design.md](docs/product-design.md)。

当前最重要的 Rust 开发痛点：

- 不盲目 `cargo clean` 的前提下追踪巨大的 `target/` 目录。
- 看清某个依赖的 feature 到底有没有开启，以及为什么开启。
- 通过 `cargo tree -i` 快速定位“是谁把这个依赖带进来的”。

## 许可证

MIT，见 [LICENSE](LICENSE)。
