# lazycargo 重构设计：core/ui 分层 + CLI lazy + doc 集成

日期：2026-08-12
状态：已批准（brainstorming 确认）

## 背景与目标

当前 `lazycargo_tui` 的 UI 层是上帝对象结构：`ui.rs` 单文件 2248 行，`impl App` 占约 1475 行，渲染靠模块级自由函数传 `&App` 进进出出，命令执行用裸 `&[&str]` 再反推类型。CLI 模式只打印命令不执行，无增量价值。

本次重构目标：
1. **core/ui 两层分离**：core 为纯逻辑层（无 ratatui/crossterm 依赖，可独立单测），ui 只负责渲染与输入。
2. **ui 再分 pages 与 components**：页面（一页 = controller + view）与无状态通用组件。
3. **CLI 变 lazy**：真执行 + 智能默认 scope（少打字）。
4. **doc 集成**：四形态（docs.rs 跳转 / 本地 doc 构建+打开 / CLI doc / TUI 内嵌阅读），真正 lazy。
5. 引入更成熟的错误处理与持久化基础设施。

## 现状问题清单

- `ui.rs:202-1677` `impl App` 上帝对象，~1475 行。
- `ui.rs:2248` 单文件混入 App 方法 + 渲染函数 + 事件循环。
- `run_cargo(&mut self, args: &[&str])` 接收裸字符串，再用 `cargo_task_kind()` 反推类型（先丢类型再猜）。
- 渲染层以 `Vec<String>` + `HashMap<OutputSlot, ContextOutput>` 为接口，文本加工厂，非人类心智模型。
- `runner.rs` / `search_job.rs` 本质是核心逻辑（进程/后台任务）却物理放在 `ui/` 下。
- CLI（`main.rs:24`）只打印命令不执行；手写 `args.rs` 289 行参数解析。
- 错误处理杂：`Result<(), String>`（build_history.rs:50）、多处 `error.to_string()` 拼接。
- config / build_history 持久化路径与读写逻辑两处重复，无原子写。

## 架构总览

```
输入（按键 / CLI 参数）
   → Page::handle_key / cli.rs
   → core 方法（run_cargo / fetch_readme / clean_stale …）
   → CoreState 变更 + mpsc 流式输出
   → 渲染循环 drain → view 只读 core → 画帧
```

**核心决策（已确认）：**

| 决策点 | 结论 |
|--------|------|
| core 边界 | core 包含一切非 UI 逻辑：领域模型 + 进程执行 + 后台任务 |
| 交互控制流 | Controller 模式（每页一个 controller），非 Action/redux |
| 状态归属 | 业务状态进 core，视图状态（focus/tab/selection/filter/scroll）留 ui |
| 迁移策略 | 一次性大重构 |
| lazycargo_search | 保持独立 publishable crate，core 依赖它 |
| CLI lazy | 真执行 + 智能默认 scope（不做"记住上次选择"） |
| doc 集成 | 四形态全做 |
| TUI 阅读渲染 | tui-markdown（joshka，ratatui 0.30 兼容） |
| 新依赖 | clap + thiserror + anyhow + tempfile |

## 目录结构

```
lazycargo_tui/src/
├── main.rs                 # 入口：无参数→TUI；有参数→CLI（anyhow::Result）
├── cli.rs                  # CLI 入口：clap 解析 + 真执行 + 智能 scope
│
├── core/                   # ═ 纯逻辑层，不依赖 ratatui/crossterm ═
│   ├── mod.rs
│   ├── project.rs          # 原 metadata.rs（ProjectInfo/PackageInfo/…）
│   ├── command.rs          # 原 cargo_task.rs（CargoTask/CommandSpec/…）
│   ├── process.rs          # 原 ui/runner.rs（spawn/streaming/timeout）
│   ├── task.rs             # 原 ui/search_job.rs（Job 配置 + 任务执行，进度用 mpsc）
│   ├── docs.rs             # 新：本地 doc 路径 + README 抓取 + docs.rs URL
│   ├── build_history.rs    # 原样搬移 + 错误类型化
│   ├── target_analyzer.rs  # 原样搬移
│   ├── dep_tree.rs         # 原样搬移
│   ├── config.rs           # 原样搬移 + 错误类型化
│   ├── search.rs           # 原 crates.rs（CrateSearchQuery/DependencyAddPlan）
│   ├── action.rs           # CoreCommand 语义化命令 enum
│   ├── store.rs            # 新：统一持久化（config + history，原子写）
│   └── model.rs            # 业务状态（CoreState：project/disk/history/processes/search/docs）
│
└── ui/                     # ═ 界面层，只依赖 ratatui + core ═
    ├── mod.rs              # 根 App：装配 + 事件循环 + 页面路由
    ├── controller.rs       # Page trait
    ├── pages/
    │   ├── mod.rs
    │   ├── workspace/      # WorkspacePage：左 3 面板 + 右输出
    │   │   ├── controller.rs
    │   │   └── view.rs     # 原 dashboard.rs 文本生成归位
    │   ├── search/         # SearchPage
    │   │   ├── controller.rs
    │   │   └── view.rs
    │   ├── docs/           # 新：文档阅读页
    │   │   ├── controller.rs
    │   │   └── view.rs
    │   └── init.rs         # cargo init 确认页
    └── components/         # 无状态通用渲染组件
        ├── panel.rs / menu.rs / scrollbar.rs / search_input.rs
        ├── command_log.rs / tab_bar.rs / status_bar.rs / dialog.rs
        └── reader.rs       # 新：tui-markdown 文档阅读器
```

## core 接口设计

### action.rs —— 语义化命令（替换 `run_cargo(args: &[&str])`）

```rust
pub enum CoreCommand {
    Check(CommandOptions),
    Build(CommandOptions),
    Test(TestOptions),
    Doc(DocOptions),
    Run(RunOptions),
    Clippy(CommandOptions),
    Update(UpdateOptions),
    AddDependency(DependencyAddPlan),
    InitProject,
}
```

core 提供 `fn run_cargo(core: &mut CoreState, cmd: CoreCommand) -> Result<CommandSpec, ProcessError>`，由 `command.rs` 保证类型（CommandOptions/TestOptions 强类型携带 scope/profile/features/target）。

### model.rs —— 业务状态

```rust
pub struct CoreState {
    pub project: ProjectInfo,
    pub disk: DiskSnapshot,
    pub history: BuildHistory,
    pub processes: ProcessManager,   // 原 ProcessState + 输出缓冲（OutputSlot→ContextOutput）
    pub search: SearchModel,
    pub docs: DocsModel,             // 文档阅读状态
}
```

进程流式输出、磁盘快照接收器（`mpsc::Receiver`）、搜索任务接收器均属 `ProcessManager`/`SearchModel`，随 core 一起移动。

### docs.rs —— doc 能力

```rust
pub fn docs_url(name: &str, version: &str) -> String;
pub fn local_doc_path(project: &ProjectInfo, name: &str) -> Option<PathBuf>;
pub fn fetch_readme(name: &str, version: &str) -> Result<String, DocsError>;   // docs.rs 原始 README markdown
pub fn fetch_fallback_description(name: &str) -> Result<String, DocsError>;     // crates.io description
```

`fetch_readme` 抓 `https://docs.rs/crate/{name}/{version}/source/README.md`（原始 markdown，而非 crates.io 的渲染 HTML）。`local_doc_path` 解析 `target/doc/{name}/index.html`。

## ui 接口设计

### controller.rs —— Page trait

```rust
pub trait Page {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool; // false=退出
    fn handle_tick(&mut self, core: &mut CoreState);
    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState;
    fn title(&self) -> &'static str;
}
```

Controller **不持有 core 引用**（避免借用冲突），由根 App 持有 `CoreState` 并传入方法。这是让 controller 既能 `&mut core` 又能 `&core` 渲染的关键。

### pages/workspace —— 示例

```rust
pub struct WorkspacePage {
    view: WorkspaceView,      // 视图状态：focus/tab/selection/filter/scroll
}
impl Page for WorkspacePage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        // 解析按键 → 更新视图状态 或 调用 core（run_build/toggle_tree/open_docs…）
    }
}
// view.rs 纯渲染：读 core+view，装配 components
pub fn render(core: &CoreState, view: &WorkspaceView, frame: &mut Frame<'_>, area: Rect) -> MouseState;
```

### components/ —— 无状态纯函数组件

`render_panel` / `render_menu` / `render_scrollbar` / `render_search_input` / `render_command_log` / tab_bar / status_bar / dialog。`reader.rs` 封装 tui-markdown → `Text` → 现有 scroll 机制。

## CLI 设计（cli.rs）

- **clap derive** 替换手写 `parse_args`（`args.rs` 删除），help/错误信息由 clap 提供。
- 子命令：`check/build/test/clippy/doc/run/update/add/search/config`（保留 `c`/`b`/`t`/`l`/`d`/`u` 缩写）。`doc` 为 doc 集成形态，构建成功自动 open 本地 index.html。
- **智能默认 scope**（clap 解析后用 cargo_metadata 判断当前目录）：
  - 位于 workspace 某 member 目录内 → 自动追加 `-p <member>`
  - 位于 workspace 根 → 自动追加 `--workspace`
  - 单 package → 不加
  - 显式 `-p` / `--workspace` 永远覆盖智能默认
  - CLI 明示推断结果：`lazycargo check → running: cargo check -p lazycargo-tui (auto-scope)`
- 执行复用 `core/process.rs`（阻塞式 + stdout），打印耗时。

## doc 集成（四形态）

| 形态 | 落点 | 触发 |
|------|------|------|
| 依赖→docs.rs 跳转 | workspace 页依赖面板 | 选中依赖按 `D` → `docs_url` → `open_url` |
| 本地 doc 构建+打开 | build 面板 Doc 任务 | 构建完成 → `local_doc_path` → open；状态/耗时进 history |
| CLI `lazycargo doc` | `cli.rs` | 真执行 cargo doc，成功 open 本地 index |
| TUI 内嵌阅读 | `ui/pages/docs/` | search 选中 crate 按 `d` 或依赖选中按 `d` |

内嵌阅读数据流：`fetch_readme` → markdown → tui-markdown 转 `Text` → `reader.rs` 渲染（复用 scroll）。抓取失败回退 `fetch_fallback_description`。DocsModel 存 `{ name, version, source, text, scroll }`。

## 错误处理与持久化

- **thiserror**：`ConfigError` / `ProcessError` / `DocsError` / `StoreError`，各有清晰 Display。消灭 `Result<(), String>`。
- **anyhow**：`cli.rs` / `main.rs` 应用层 `.context(...)` 加上下文，顶层统一 `eprintln!("error: {err:#}")`。
- **store.rs** 统一持久化：config + build_history 的路径与读写集中一处，写入走 tempfile+rename 原子写。

## 依赖变更

新增（lazycargo_tui）：
- `clap = { version = "4", features = ["derive"] }`
- `thiserror = "2"`
- `anyhow = "1"`
- `tempfile = "3"`
- `tui-markdown = "0.3.9"`

保留：reqwest / cargo_metadata / chrono / crossterm / ratatui / walkdir / open / arboard / ansi-to-tui / lazycargo_search（独立 crate 不动）。

## 测试策略

| 层 | 测试内容 | 方式 |
|----|---------|------|
| core | 命令构造/scope 智能默认/依赖树解析/磁盘分析/doc URL 与本地路径 | 单元测试（保留 dep_tree 测试，补 command/scope/doc 测试） |
| cli | 参数解析（clap 自带）、智能 scope 选择逻辑 | 单元测试 |
| ui | 视图状态转换（focus/tab/selection 纯逻辑） | 抽离后单测 |
| 集成 | CLI 真执行冒烟 | 改造 `tests/cli_args.rs` → `tests/cli.rs` |

**不测试**：ratatui 帧渲染（手动验证）。验证命令：`cargo build`、`cargo test`、`cargo clippy`、`cargo fmt --check`。

## 迁移范围

一次性大重构，完成后统一编译验证。文件映射：

- `metadata.rs` → `core/project.rs`
- `cargo_task.rs` → `core/command.rs`
- `ui/runner.rs` → `core/process.rs`
- `ui/search_job.rs` → `core/task.rs`（任务配置 + 后台执行 + 进度接收）
- `ui/dashboard.rs` → `ui/pages/workspace/view.rs` + `ui/pages/search/view.rs`
- `ui/layout.rs`/`ui/style.rs` → `ui/components/`
- `state/mod.rs` → 拆分：业务部分进 `core/model.rs`，视图部分进各页 controller
- `args.rs` → 删除，由 `cli.rs`（clap）替代
- `config.rs`/`build_history.rs` 持久化统一进 `core/store.rs`
