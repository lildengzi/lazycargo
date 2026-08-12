# lazycargo core/ui 重构实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lazycargo_tui 重构为 core/ui 两层（ui 再分 pages/components），CLI 真执行+智能 scope，doc 集成四形态，引入 clap/thiserror/anyhow/tempfile/tui-markdown。

**Architecture:** core 为纯逻辑层（无 ratatui/crossterm 依赖），含领域模型+进程+后台任务+业务状态；ui 层只做渲染与输入，按 Page trait（controller）组织页面，components 为无状态渲染函数。根 App 持有 CoreState + 全局导航状态 + 页面路由。

**Tech Stack:** Rust, ratatui 0.30, crossterm 0.29, clap 4, thiserror 2, anyhow 1, tui-markdown 0.3.9, tempfile 3, reqwest(blocking), cargo_metadata 0.19

## Global Constraints

- 版本要求：rust 1.88+（clap 4.6 / tui-markdown 0.3.9 的 MSRV；Task 19 需同步 README 徽章，把 1.81 改为 1.88）
- core/ 模块**禁止**依赖 ratatui / crossterm / 任何 `Frame`/`KeyEvent` 类型（crossterm 的 KeyEvent 只允许出现在 ui/）
- `lazycargo_search` 独立 crate 不动（它是 publishable，PKGBUILD 依赖它）
- 最终删除的源文件：`src/ui.rs`、`src/ui/dashboard.rs`、`src/ui/layout.rs`、`src/ui/style.rs`、`src/ui/search_job.rs`、`src/ui/runner.rs`、`src/ui/terminal_support.rs`、`src/state/mod.rs`、`src/metadata.rs`、`src/cargo_task.rs`、`src/crates.rs`、`src/build_history.rs`、`src/config.rs`、`src/args.rs`、`src/target_analyzer.rs`、`src/dep_tree.rs`、`src/util.rs`
- 每任务结束必须 `cargo build` 通过；涉及测试的任务 `cargo test` 通过
- 保留 `tests/cli_args.rs` 现有行为直到 Task 19 改造它

---

### Task 1: 依赖更新与 core 模块骨架

**Files:**
- Modify: `lazycargo_tui/Cargo.toml`
- Modify: `lazycargo_tui/src/lib.rs`
- Create: `lazycargo_tui/src/core/mod.rs`

**Interfaces:**
- Produces: `crate::core` 模块树（初始为空壳），后续任务逐个填充 `core::project`、`core::command` 等
- Produces: `core::StoreError` / `core::ProcessError` / `core::DocsError` / `core::ProjectError`（Task 3/4/7 定义，此处先定义占位错误类型供引用）

- [ ] **Step 1: 添加依赖到 `lazycargo_tui/Cargo.toml`**

在 `[dependencies]` 中添加：

```toml
anyhow = "1"
clap = { version = "4", features = ["derive"] }
tempfile = "3"
thiserror = "2"
tui-markdown = "0.3.9"
```

- [ ] **Step 2: 创建 core 模块空壳**

创建 `lazycargo_tui/src/core/mod.rs`：

```rust
pub mod config;
pub mod store;
```

- [ ] **Step 3: 更新 `src/lib.rs` 声明**

在现有模块声明末尾追加：

```rust
pub mod core;
```

- [ ] **Step 4: 建立两个基础错误类型**

创建 `lazycargo_tui/src/core/store.rs`：

```rust
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("failed to read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("failed to write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
    #[error("invalid json in {path}: {source}")]
    Json { path: PathBuf, source: serde_json::Error },
}
```

创建 `lazycargo_tui/src/core/config.rs`（占位，Task 4 填充完整实现）：

```rust
pub use crate::store::StoreError;
```

- [ ] **Step 5: 验证编译**

Run: `cargo build`
Expected: PASS（此时 config.rs 是占位，Task 4 才会真正搬 config 实现）

- [ ] **Step 6: Commit**

```bash
git add lazycargo_tui/Cargo.toml lazycargo_tui/src/lib.rs lazycargo_tui/src/core/
git commit -m "build: add clap/thiserror/anyhow/tempfile/tui-markdown, core skeleton"
```

---

### Task 2: 搬移核心领域模型进 core

把 `metadata.rs`、`cargo_task.rs`、`crates.rs`、`util.rs` 移入 core，保留旧文件为薄转发层（`pub use crate::core::...`），保证 ui.rs 引用不碎。

**Files:**
- Create: `lazycargo_tui/src/core/project.rs`
- Create: `lazycargo_tui/src/core/command.rs`
- Create: `lazycargo_tui/src/core/search.rs`
- Create: `lazycargo_tui/src/core/util.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`
- Modify: `lazycargo_tui/src/lib.rs`
- Modify（转发）: `src/metadata.rs` / `src/cargo_task.rs` / `src/crates.rs` / `src/util.rs`

**Interfaces:**
- Produces: `core::project::{ProjectInfo, PackageInfo, TargetInfo, DependencyInfo, PackageFeatureInfo, DependencyKind, MetadataLoadError}` — 除路径外与现状一致
- Produces: `core::command::{CargoTask, CargoTaskKind, TaskScope, FeatureSelection, Profile, CommandSpec}` — 与现状一致
- Produces: `core::search::{CrateSearchQuery, DependencyAddPlan, DependencyKind}`
- Produces: `core::util::{format_bytes, progress_bar, animated_progress_bar}`

- [ ] **Step 1: 移动四个文件**

用 `git mv` 并把内容写入新位置（内容保持不变）：

```bash
git mv lazycargo_tui/src/metadata.rs lazycargo_tui/src/core/project.rs
git mv lazycargo_tui/src/cargo_task.rs lazycargo_tui/src/core/command.rs
git mv lazycargo_tui/src/crates.rs lazycargo_tui/src/core/search.rs
git mv lazycargo_tui/src/util.rs lazycargo_tui/src/core/util.rs
```

- [ ] **Step 2: 更新 `core/mod.rs`**

```rust
pub mod command;
pub mod config;
pub mod project;
pub mod search;
pub mod store;
pub mod util;
```

- [ ] **Step 3: 把四个旧文件替换为转发层**

`src/metadata.rs`：

```rust
pub use crate::core::project::*;
```

`src/cargo_task.rs`：

```rust
pub use crate::core::command::*;
```

`src/crates.rs`：

```rust
pub use crate::core::search::*;
```

`src/util.rs`：

```rust
pub use crate::core::util::*;
```

- [ ] **Step 4: 修正 `src/lib.rs`**

删除旧的 `pub mod build_history; pub mod cargo_task; pub mod crates; pub mod metadata; pub mod util;`（保留 `build_history`/`config` 仍为旧文件），并更新 `pub use`：

```rust
pub use cargo_task::{CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile, TaskScope};
pub use crates::{CrateSearchQuery, DependencyAddPlan, DependencyKind};
pub use metadata::{DependencyInfo, PackageFeatureInfo, PackageInfo, ProjectInfo, TargetInfo};
```

保持现状（转发层会让这些继续工作）。注意 `core/search.rs` 内部 `use crate::cargo_task::CommandSpec;` 要改为 `use crate::core::command::CommandSpec;`。

- [ ] **Step 5: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib`
Expected: PASS（config/dep_tree 的既有测试仍在）

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "refactor: move domain models into core module with forwarding shims"
```

---

### Task 3: core/process.rs（原 runner）+ 错误类型化

把 `ui/runner.rs` 移入 `core/process.rs`，错误类型化，UI 依赖它的部分改为 `crate::core::process`。

**Files:**
- Create: `lazycargo_tui/src/core/process.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`
- Modify（转发）: `src/ui/runner.rs`
- Modify: `src/ui.rs`、`src/ui/search_job.rs`（改引用）

**Interfaces:**
- Produces: `core::process::{OutputLine, spawn_streaming, split_output, extract_diagnostics}`
- Produces: `core::process::run_captured(program: &str, args: &[&str], timeout: Duration) -> Result<Option<Output>, ProcessError>`
- Produces: `core::process::ProcessError`（thiserror，IO 变体）
- Consumes: 现状 `ui::runner::command_output_with_timeout` 语义不变

- [ ] **Step 1: 移动并改写 `ui/runner.rs`**

`git mv lazycargo_tui/src/ui/runner.rs lazycargo_tui/src/core/process.rs`，然后改三处：
1. 顶部加 `use thiserror::Error;`
2. `command_output_with_timeout` 改名为 `run_captured`，返回类型改 `Result<Option<Output>, ProcessError>`，内部 `io::Error` 用 `.map_err(|source| ProcessError::Io { source })`
3. 定义：

```rust
#[derive(Debug, Error)]
pub enum ProcessError {
    #[error("failed to run command: {source}")]
    Io { source: std::io::Error },
}
```

- [ ] **Step 2: 更新引用**

`src/ui.rs` 中 `use runner::{extract_diagnostics, spawn_streaming};` → `use crate::core::process::{extract_diagnostics, spawn_streaming};`，删除 `mod runner;`。`src/ui/search_job.rs` 中 `use super::runner::{command_output_with_timeout, split_output};` → `use crate::core::process::{run_captured, split_output};`，调用处 `command_output_with_timeout(` → `run_captured(`。

`src/ui/runner.rs` 替换为转发层：

```rust
pub use crate::core::process::*;
```

但转发层无法转发 `pub(super)` 的 `spawn_streaming`。检查 `ui.rs` 对 `spawn_streaming` 的调用：它在 `ui.rs:57` 引入 `use runner::{extract_diagnostics, spawn_streaming};`，随后通过 `load_disk_snapshot_async`（ui.rs:2142）使用。由于 `core::process` 是 `pub` 模块，把函数改为 `pub` 即可。

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: move process runner into core with typed ProcessError"
```

---

### Task 4: core/store.rs 原子写 + config/build_history 类型化

把 `config.rs` 和 `build_history.rs` 的错误改成类型化，持久化写入统一走 `core/store.rs` 的原子写。

**Files:**
- Modify: `lazycargo_tui/src/core/store.rs`（补原子写函数）
- Modify: `lazycargo_tui/src/core/config.rs`（填充真实实现）
- Modify: `lazycargo_tui/src/build_history.rs`
- Modify: `lazycargo_tui/src/config.rs`（转发层）
- Modify: `lazycargo_tui/src/core/mod.rs`

**Interfaces:**
- Produces: `core::store::atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError>`
- Produces: `core::store::config_path() -> PathBuf`、`core::store::history_path() -> PathBuf`
- Produces: `core::config::AppConfig`（`load_or_create`/`config_path`/`network_timeout`/`cargo_info_timeout` 与现状同签名，`ConfigError` 移除，改返回 `StoreError`）
- Produces: `core::build_history::{BuildHistory, BuildEntry, CrateTiming, CommandStats}`（`add_entry -> Result<(), StoreError>`，`save -> Result<(), StoreError>`）

- [ ] **Step 1: 在 `core/store.rs` 追加原子写与路径**

```rust
use std::fs;
use std::path::{Path, PathBuf};
use serde::Serialize;

pub fn config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazycargo")
        .join("config.json")
}

pub fn history_path() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("lazycargo")
        .join("build_history.json")
}

pub fn atomic_write_json<T: Serialize>(path: &Path, value: &T) -> Result<(), StoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| StoreError::Write { path: path.to_owned(), source })?;
    }
    let text = serde_json::to_string_pretty(value)
        .map_err(|source| StoreError::Json { path: path.to_owned(), source })?;
    let mut tmp = tempfile::NamedTempFile::new_in(
        path.parent().unwrap_or_else(|| Path::new(".")),
    )
    .map_err(|source| StoreError::Write { path: path.to_owned(), source })?;
    tmp.write_all(text.as_bytes())
        .map_err(|source| StoreError::Write { path: path.to_owned(), source })?;
    tmp.persist(path)
        .map_err(|error| StoreError::Write { path: path.to_owned(), source: error.error })?;
    Ok(())
}
```

需要 `use std::io::Write;`。若 `NamedTempFile` 的 `persist` 与窗口语义冲突则忽略（linux 目标）。

- [ ] **Step 2: 填充 `core/config.rs`**

把现 `src/config.rs` 的 `AppConfig`/`ConfigError` 实现移入，改动：
- 删除 `ConfigError`，`load_or_create`/`save_config_to_path` 返回 `Result<_, StoreError>`
- `config_path()` 改为调用 `crate::core::store::config_path()`
- 写文件用 `crate::core::store::atomic_write_json(&path, &config)`
- 读取的 `fs::read_to_string(&path).map_err(...)` 改为 `StoreError::Read { path, source }`

保留 `#[cfg(test)] mod tests`（`default_config_is_stable` 等），`normalize` 保留。

- [ ] **Step 3: 改 `src/build_history.rs`**

- `pub fn load(&self)` 保持
- `save` 改为 `pub fn save(&self) -> Result<(), StoreError>`，用 `atomic_write_json(&self.db_path, &self.entries)`
- `add_entry` 返回 `Result<(), StoreError>`
- `history_path()` 改为调用 `crate::core::store::history_path()`

`src/config.rs` 替换为转发层：

```rust
pub use crate::core::config::*;
```

`src/build_history.rs` 保留为真实文件（Task 16 才移入 core，因为 build_history.rs 目前被 ui.rs 和 tests 引用；为避免转发层双路径，此处直接改返回类型，文件仍在原路径）。

`core/mod.rs` 追加：

```rust
pub mod process;
```

- [ ] **Step 4: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib`
Expected: PASS（config 测试验证原子写路径）

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: atomic json writes and typed errors for config/history"
```

---

### Task 5: core/task.rs（原 search_job）

把 `ui/search_job.rs` 移入 `core/task.rs`，改引用。

**Files:**
- Create: `lazycargo_tui/src/core/task.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`
- Modify（转发）: `src/ui/search_job.rs`

**Interfaces:**
- Produces: `core::task::{SearchJobConfig, SearchJobResult, SearchJobKind, run_search_job, run_info_job, search_progress_detail, info_progress_detail}` — 与现状一致
- Consumes: `core::process::run_captured`、`core::util::progress_bar`、`lazycargo_search::*`

- [ ] **Step 1: 移动并改引用**

`git mv lazycargo_tui/src/ui/search_job.rs lazycargo_tui/src/core/task.rs`。

改动：
- `use crate::util::{animated_progress_bar, progress_bar};` → `use crate::core::util::{animated_progress_bar, progress_bar};`
- `use super::runner::{command_output_with_timeout, split_output};` → `use crate::core::process::{run_captured, split_output};`，调用处同步改 `run_captured`
- 所有 `pub(super)` 改为 `pub(crate)` 或 `pub`（`core::task` 需被 ui 访问）：`SearchJobConfig`/`SearchJobResult`/`SearchJobKind`/`run_search_job`/`run_info_job`/`search_progress_detail`/`info_progress_detail` → `pub`

`src/ui/search_job.rs` 替换为转发层：

```rust
pub use crate::core::task::*;
```

`core/mod.rs` 追加 `pub mod task;`。`src/ui.rs` 中 `use search_job::{...};` → `use crate::core::task::{...};`，删除 `mod search_job;`。

- [ ] **Step 2: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: move search jobs into core/task"
```

---

### Task 6: core 剩余纯搬移（target_analyzer / dep_tree / build_history 收尾）

把 `target_analyzer.rs`、`dep_tree.rs` 移入 core，并把 `build_history.rs` 也移入 core（此前为真实文件），旧文件全部转转发层。

**Files:**
- Create: `lazycargo_tui/src/core/target_analyzer.rs`
- Create: `lazycargo_tui/src/core/dep_tree.rs`
- Create: `lazycargo_tui/src/core/build_history.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`
- Modify（转发）: `src/target_analyzer.rs` / `src/dep_tree.rs` / `src/build_history.rs`
- Modify: `src/lib.rs`

**Interfaces:**
- Produces: `core::target_analyzer::{DiskSnapshot, clean_stale, ...}`（与现状一致，函数/字段名不变）
- Produces: `core::dep_tree::{DepNode, parse_tree_output, flatten_visible, toggle_node, ...}`（与现状一致，`mod tests` 保留）
- Produces: `core::build_history::{BuildHistory, BuildEntry, CrateTiming, CommandStats, command_kind, is_recordable_command, format_duration_ms}`

- [ ] **Step 1: 移动三个文件并改内部引用**

```bash
git mv lazycargo_tui/src/target_analyzer.rs lazycargo_tui/src/core/target_analyzer.rs
git mv lazycargo_tui/src/dep_tree.rs lazycargo_tui/src/core/dep_tree.rs
git mv lazycargo_tui/src/build_history.rs lazycargo_tui/src/core/build_history.rs
```

- `core/target_analyzer.rs`、`core/dep_tree.rs` 内容不变（它们只依赖 std/chrono/serde）。
- `core/build_history.rs` 中 `use crate::config::...` 若无则不变；`history_path()` 若引用改 `crate::core::store::history_path()`。
- `core/mod.rs` 追加：

```rust
pub mod build_history;
pub mod dep_tree;
pub mod target_analyzer;
```

- [ ] **Step 2: 旧文件转转发层**

`src/target_analyzer.rs`、`src/dep_tree.rs`、`src/build_history.rs` 各自替换为：

```rust
pub use crate::core::target_analyzer::*;
```

（对应各自名字）。更新 `src/lib.rs`：删除 `pub mod build_history; pub mod dep_tree; pub mod metadata; ...` 中已转发的项，保留转发声明即可（lib.rs 的 `pub mod build_history;` 现指向转发层文件，无需删）。`pub use` 保持。

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib`
Expected: PASS（dep_tree 测试随迁，config 测试仍在）

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: move analyzer/dep-tree/history into core, forwarding shims"
```

---

### Task 7: core/docs.rs（新模块）+ 单元测试

**Files:**
- Create: `lazycargo_tui/src/core/docs.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`

**Interfaces:**
- Produces:
  - `core::docs::docs_url(name: &str, version: &str) -> String` → `format!("https://docs.rs/{name}/{version}")`
  - `core::docs::docs_root_url(name: &str) -> String` → `format!("https://docs.rs/{name}")`
  - `core::docs::local_doc_path(project: &ProjectInfo, name: &str) -> Option<PathBuf>` → `<workspace_root>/target/doc/<name>/index.html` 若存在则 `Some`，否则 `None`
  - `core::docs::fetch_readme(name: &str, version: &str, timeout: Duration) -> Result<String, DocsError>` → GET `https://docs.rs/crate/{name}/{version}/source/README.md`，返回 body 文本
  - `core::docs::fetch_description(name: &str, timeout: Duration) -> Result<String, DocsError>` → GET `https://crates.io/api/v1/crates/{name}`，从 JSON `version.description` 提取
  - `core::docs::DocsError`（thiserror：Http { status } / Request { source } / Parse { source } / NotFound）
- Consumes: `core::project::ProjectInfo`、reqwest blocking（同 lazycargo_search 的用法）

- [ ] **Step 1: 写失败测试**

创建 `lazycargo_tui/src/core/docs.rs` 底部测试：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_url_combines_name_and_version() {
        assert_eq!(docs_url("serde", "1.0.228"), "https://docs.rs/serde/1.0.228");
    }

    #[test]
    fn local_doc_path_resolves_under_target_doc() {
        let mut project = ProjectInfo::default();
        project.workspace_root = std::env::temp_dir().to_string_lossy().into_owned();
        // target/doc 不存在 → None
        assert_eq!(local_doc_path(&project, "serde"), None);
    }
}
```

注意：`local_doc_path` 的 `Some` 分支需要真实文件，测试只验证 `None` 分支和纯路径拼接。用私有辅助函数 `local_doc_path_unchecked(workspace_root, name)` 返回拼好的路径，`local_doc_path` 检查 exists。测试断言 unchecked 拼接：

```rust
fn local_doc_path_unchecked(workspace_root: &str, name: &str) -> PathBuf {
    Path::new(workspace_root).join("target").join("doc").join(name).join("index.html")
}

#[test]
fn local_doc_path_layout_is_under_target_doc() {
    assert_eq!(
        local_doc_path_unchecked("/workspace", "serde"),
        PathBuf::from("/workspace/target/doc/serde/index.html")
    );
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --lib docs::tests`
Expected: FAIL（`docs` 模块/函数未定义）

- [ ] **Step 3: 实现 `core/docs.rs`**

```rust
use std::path::{Path, PathBuf};
use std::time::Duration;

use thiserror::Error;

use crate::core::project::ProjectInfo;

#[derive(Debug, Error)]
pub enum DocsError {
    #[error("docs.rs request failed: {0}")]
    Request(String),
    #[error("docs.rs returned HTTP {0}")]
    Http(String),
    #[error("invalid crates.io response: {0}")]
    Parse(String),
    #[error("no documentation found for {0}")]
    NotFound(String),
}

pub fn docs_url(name: &str, version: &str) -> String {
    format!("https://docs.rs/{name}/{version}")
}

pub fn docs_root_url(name: &str) -> String {
    format!("https://docs.rs/{name}")
}

fn local_doc_path_unchecked(workspace_root: &str, name: &str) -> PathBuf {
    Path::new(workspace_root)
        .join("target")
        .join("doc")
        .join(name)
        .join("index.html")
}

pub fn local_doc_path(project: &ProjectInfo, name: &str) -> Option<PathBuf> {
    let path = local_doc_path_unchecked(&project.workspace_root, name);
    path.is_file().then_some(path)
}

fn get(url: &str, timeout: Duration) -> Result<String, DocsError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(timeout)
        .user_agent("lazycargo (https://github.com/lildengzi/lazycargo)")
        .build()
        .map_err(|error| DocsError::Request(error.to_string()))?;
    let response = client
        .get(url)
        .send()
        .map_err(|error| DocsError::Request(error.to_string()))?;
    if !response.status().is_success() {
        return Err(DocsError::Http(response.status().to_string()));
    }
    response
        .text()
        .map_err(|error| DocsError::Request(error.to_string()))
}

pub fn fetch_readme(name: &str, version: &str, timeout: Duration) -> Result<String, DocsError> {
    get(
        &format!("https://docs.rs/crate/{name}/{version}/source/README.md"),
        timeout,
    )
}

pub fn fetch_description(name: &str, timeout: Duration) -> Result<String, DocsError> {
    let body = get(&format!("https://crates.io/api/v1/crates/{name}"), timeout)?;
    #[derive(serde::Deserialize)]
    struct CrateResponse { version: Version }
    #[derive(serde::Deserialize)]
    struct Version { description: Option<String> }
    let payload: CrateResponse = serde_json::from_str(&body)
        .map_err(|error| DocsError::Parse(error.to_string()))?;
    payload
        .version
        .description
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| DocsError::NotFound(name.to_owned()))
}
```

`core/mod.rs` 追加 `pub mod docs;`。

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test --lib docs::tests`
Expected: PASS

Run: `cargo build`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: core/docs module with url resolution and readme fetch"
```

---

### Task 8: core/model.rs 业务状态（CoreState）

把 `state/mod.rs` 中业务相关状态收拢进 `core/model.rs`，视图相关状态留在 ui 侧（Task 10 建）。

**Files:**
- Create: `lazycargo_tui/src/core/model.rs`
- Modify: `lazycargo_tui/src/core/mod.rs`
- Modify: `lazycargo_tui/src/state/mod.rs`

**Interfaces:**
- Produces:
  - `core::model::CoreState { pub config: AppConfig, pub project: ProjectInfo, pub disk: DiskSnapshot, pub disk_receiver: Option<Receiver<DiskSnapshot>>, pub history: BuildHistory, pub processes: ProcessState, pub output: HashMap<OutputSlot, ContextOutput>, pub search: SearchModel, pub search_receiver: Option<Receiver<SearchJobResult>>, pub search_started: Option<Instant> }`
  - `core::model::{OutputSlot, ContextOutput, ProcessState, SearchModel}`（从 `state/mod.rs` 迁入，`ProcessState` 含 `child: Option<Child>`/`command: String`/`start: Instant`/`slot: OutputSlot`）
  - `core::model::CoreState::new(project: ProjectInfo, config: AppConfig, disk: DiskSnapshot) -> Self`
  - `core::model::CoreState::context(&mut self, slot: OutputSlot) -> &mut ContextOutput`、`fn slot_lines(&self, slot: OutputSlot) -> Vec<String>`、`fn set_slot_lines(&mut self, slot: OutputSlot, lines: Vec<String>)`、`fn drain_all_streams(&mut self, max_lines: usize)`、`fn poll_disk_snapshot(&mut self)`、`fn poll_search_job(&mut self)`
- Consumes: `core::config::AppConfig`、`core::project::ProjectInfo`、`core::target_analyzer::DiskSnapshot`、`core::build_history::BuildHistory`、`core::task::{SearchJobResult, SearchJobKind, search_progress_detail, info_progress_detail}`、`lazycargo_search::SearchState`

- [ ] **Step 1: 把 `state/mod.rs` 内容拆分**

`WorkspaceModel` 的字段（project/disk/disk_receiver/build_history/diagnostics）直接并入 `CoreState`。`SearchModel`（包装 `SearchState`）保留。`NavigationState`/`SelectionState` 留在 `state/mod.rs` 供 ui 使用（暂不动）。`OutputSlot`/`ContextOutput`/`ProcessState` 迁入 `core/model.rs`。

`OutputSlot` 需加两个新变体（doc 页用，Task 15 消费）：

```rust
pub enum OutputSlot {
    WorkspaceCrateInfo,
    WorkspaceMetrics,
    WorkspaceTarget,
    BuildConfig,
    BuildLive,
    DepsFeatures,
    DepsTree,
    SearchDetail,
    DocsReadme,     // 新增
    DocsFallback,   // 新增
}
```

`poll_search_job`/`poll_disk_snapshot` 逻辑从 `App`（ui.rs:302-378）移入 `CoreState`，把 `self.navigation.*`/`self.history` 的副作用改为通过返回元组或回调让 ui 处理。**本任务只搬状态结构，方法体在 Task 16 从 App 迁移时落位**；此处 `CoreState::new` 只需完成字段组装。

`core/model.rs`：

```rust
use std::collections::HashMap;
use std::process::Child;
use std::sync::mpsc::Receiver;
use std::time::Instant;

use lazycargo_search::SearchState;

use crate::core::build_history::BuildHistory;
use crate::core::config::AppConfig;
use crate::core::project::ProjectInfo;
use crate::core::target_analyzer::DiskSnapshot;
use crate::core::task::SearchJobResult;

// OutputSlot / ContextOutput 完整迁入（含 drain_stream / trim_lines / with_lines）
// ProcessState 完整迁入
// SearchModel { pub state: SearchState }

pub struct CoreState {
    pub config: AppConfig,
    pub project: ProjectInfo,
    pub disk: DiskSnapshot,
    pub disk_receiver: Option<Receiver<DiskSnapshot>>,
    pub history: BuildHistory,
    pub processes: ProcessState,
    pub output: HashMap<OutputSlot, ContextOutput>,
    pub search: SearchModel,
    pub search_receiver: Option<Receiver<SearchJobResult>>,
    pub search_started: Option<Instant>,
}

impl CoreState {
    pub fn new(project: ProjectInfo, config: AppConfig, disk: DiskSnapshot) -> Self {
        let mut output = HashMap::new();
        output.insert(OutputSlot::BuildLive, ContextOutput::with_lines(Vec::new()));
        Self {
            config,
            project,
            disk,
            disk_receiver: None,
            history: BuildHistory::load(),
            processes: ProcessState {
                child: None,
                command: String::new(),
                start: Instant::now(),
                slot: OutputSlot::BuildLive,
            },
            output,
            search: SearchModel { state: SearchState::default() },
            search_receiver: None,
            search_started: None,
        }
    }

    pub fn context(&mut self, slot: OutputSlot) -> &mut ContextOutput {
        self.output.entry(slot).or_insert_with(ContextOutput::new)
    }

    pub fn slot_lines(&self, slot: OutputSlot) -> Vec<String> {
        self.output.get(&slot).map(|c| c.lines.clone()).unwrap_or_default()
    }

    pub fn set_slot_lines(&mut self, slot: OutputSlot, lines: Vec<String>) {
        let ctx = self.context(slot);
        ctx.lines = lines;
        ctx.stream_rx = None;
        ctx.follow_tail = true;
    }
}
```

- [ ] **Step 2: 改 `state/mod.rs`**

删除迁走的 `WorkspaceModel`/`ProcessState`/`ContextOutput`/`OutputSlot`/`SearchModel` 定义，改为 `pub use crate::core::model::{...};` 或直接删（ui.rs 引用改为 `crate::core::model`）。`NavigationState`/`SelectionState` 保留原样。

- [ ] **Step 3: 更新 ui.rs 引用**

`src/ui.rs` 顶部 `use crate::state::{ContextOutput, NavigationState, OutputSlot, ProcessState, SearchModel, SelectionState, WorkspaceModel};` 改为：

```rust
use crate::core::model::{ContextOutput, OutputSlot, ProcessState, SearchModel, CoreState};
use crate::state::{NavigationState, SelectionState};
```

`App` struct（ui.rs:189-200）改为持有 `core: CoreState` + `navigation` + `selection`。`App::new` 组装 `CoreState::new(...)`。凡原 `self.workspace.project` → `self.core.project`，`self.workspace.disk` → `self.core.disk`，`self.config` → `self.core.config`，`self.process` → `self.core.processes`，`self.output_store` → `self.core.output`，`self.search.state` → `self.core.search.state`。

- [ ] **Step 4: 编译验证**

Run: `cargo build`
Expected: PASS（如有个别方法名冲突，按编译器提示调整 `CoreState` 辅助方法，保持 App 逻辑等价）

Run: `cargo test --lib`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: consolidate business state into core::model::CoreState"
```

---

### Task 9: 清理转发层，删除旧文件

Task 2-6 的转发层（`src/metadata.rs`、`src/cargo_task.rs`、`src/crates.rs`、`src/util.rs`、`src/target_analyzer.rs`、`src/dep_tree.rs`、`src/build_history.rs`、`src/config.rs`、`src/ui/runner.rs`、`src/ui/search_job.rs`）全部删除，所有引用改为直接 `crate::core::*`。

**Files:**
- Delete: 上述转发层文件
- Modify: `src/lib.rs`（`pub mod` 删除转发项；`pub use crate::core::...` 直接引用）
- Modify: `src/state/mod.rs`、`src/ui.rs`、`src/ui/dashboard.rs`、`src/ui/layout.rs`、`src/ui/style.rs`、`src/ui/terminal_support.rs`（改 `use`）

**Interfaces:**
- Consumes: 此前所有 `core::*` 模块
- Produces: 无新接口；lib.rs 对外 API 与现状保持一致（`pub use` 不改名）

- [ ] **Step 1: 删除转发文件并全局改引用**

```bash
git rm lazycargo_tui/src/metadata.rs lazycargo_tui/src/cargo_task.rs lazycargo_tui/src/crates.rs \
       lazycargo_tui/src/util.rs lazycargo_tui/src/target_analyzer.rs lazycargo_tui/src/dep_tree.rs \
       lazycargo_tui/src/build_history.rs lazycargo_tui/src/config.rs \
       lazycargo_tui/src/ui/runner.rs lazycargo_tui/src/ui/search_job.rs
```

`src/lib.rs`：

```rust
pub mod args;
pub mod core;
pub mod keymap;
pub mod state;
pub mod ui;

pub use core::command::{CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile, TaskScope};
pub use core::config::AppConfig;
pub use core::project::*;
pub use core::search::{CrateSearchQuery, DependencyAddPlan, DependencyKind};
pub use core::{parse_args, ArgsError, CliAction}; // args 仍为 src/args.rs（Task 17 删除）
```

将 `src/args.rs` 内部 `use crate::cargo_task::...`/`use crate::crates::...` 改为 `use crate::core::command::...`/`use crate::core::search::...`。

用 `cargo build` 反复定位所有残留 `crate::metadata`/`crate::cargo_task`/`crate::crates`/`crate::util`/`crate::target_analyzer`/`crate::dep_tree`/`crate::config`/`crate::build_history`/`crate::state::{ProcessState,...}` 引用并修正为 `crate::core::*`。

- [ ] **Step 2: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: PASS（`tests/cli_args.rs` 仍走 lib 导出 API，未变）

- [ ] **Step 3: Commit**

```bash
git add -A
git commit -m "refactor: drop forwarding shims, wire references to core directly"
```

---

### Task 10: ui/controller.rs —— Page trait + 视图状态类型

**Files:**
- Create: `lazycargo_tui/src/ui/controller.rs`
- Modify: `lazycargo_tui/src/ui/mod.rs`（先建空 `ui` 模块树：`pages`/`components` 子模块声明，Task 11+ 填充）

**Interfaces:**
- Produces:
  ```rust
  pub struct MouseState {
      pub panel_areas: Vec<(Focus, Rect, usize)>,
      pub link_areas: Vec<(Rect, String)>,
      pub tab_areas: Vec<(Rect, ContextTab)>,
      pub right_scrollbar_area: Option<Rect>,
      pub right_scrollbar_content_len: usize,
      pub right_scrollbar_visible_rows: usize,
  }
  ```
  （从 ui.rs:179-187 迁入，字段 `pub`）
  ```rust
  pub trait Page {
      fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool;
      fn handle_tick(&mut self, core: &mut CoreState);
      fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState;
      fn title(&self) -> &'static str;
  }
  ```
  ```rust
  pub enum PageId { Workspace, Search, Docs, Init }
  ```
  ```rust
  pub struct WorkspaceView {
      pub ws_tab: WorkspaceTab,
      pub build_tab: BuildCoreTab,
      pub deps_tab: DependenciesTab,
      pub selected: SelectedIndex,   // { workspace, dependency, build, target_crate, tree }
      pub tree_expanded: HashMap<String, bool>,
      pub search_return_focus: Focus,
  }
  pub struct SelectedIndex { pub workspace: usize, pub dependency: usize, pub build: usize, pub target_crate: usize, pub tree: usize }
  ```
  ```rust
  pub struct DocsView { pub scroll: usize, pub follow_tail: bool, pub visible_rows: usize }
  ```
  （`focus`/`current_focus`/`input_mode` 等全局导航仍留在 `state::NavigationState`，由根 App 持有——Task 8 保留，Task 15 使用）
- Consumes: `core::model::CoreState`；`Focus`/`FocusPanel`/`WorkspaceTab`/`BuildCoreTab`/`DependenciesTab`/`ContextTab`/`InputMode`（从 ui.rs 迁到 `controller.rs`，Task 12 起被 pages 引用）

- [ ] **Step 1: 创建 `ui/controller.rs`**

把 `Focus`、`InputMode`、`FocusPanel`、`WorkspaceTab`、`BuildCoreTab`、`DependenciesTab`、`ContextTab`（含各 `impl` 的 `from_digit`/`next`/`title`/`label`）从 ui.rs:64-171 原样迁入，可见性 `pub(crate)`。新增 `MouseState`、`Page`、`PageId`、`WorkspaceView`、`SelectedIndex`、`DocsView`、`InitView`。`WorkspaceView::default()` 按原 `SelectionState`/`NavigationState` 的初始值（workspace_selected:0 等）实现。

- [ ] **Step 2: 建空 `ui/pages/mod.rs` 和 `ui/components/mod.rs`**

```rust
// ui/pages/mod.rs
pub mod docs;
pub mod init;
pub mod search;
pub mod workspace;
// ui/components/mod.rs
pub mod command_log;
pub mod menu;
pub mod panel;
pub mod reader;
pub mod scrollbar;
pub mod search_input;
pub mod status_bar;
pub mod style;
pub mod tab_bar;
```

每个空模块建最小文件（`// 占位`），保证 `cargo build` 通过。`ui/mod.rs` 写：

```rust
mod components;
mod controller;
mod pages;

pub use controller::*;
```

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS（此时旧 ui.rs 仍在 src/，新 ui/ 目录尚未接入 lib.rs 的 `pub mod ui;` 冲突——需要处理）

**冲突处理**：`src/ui.rs`（文件）与 `src/ui/`（目录）不能共存。本任务先建 `src/ui/` 目录，把 `src/ui.rs` 移到 `src/ui/app.rs` 或保留为 `src/ui/mod.rs`？——**本任务把现有 `src/ui.rs` 整体移动到 `src/ui/mod.rs`**，作为临时根模块（包含旧 App 与新 module 声明），后续任务逐步把内容拆到 pages/components/controller。`git mv src/ui.rs src/ui/mod.rs`，并在 `src/ui/mod.rs` 顶部追加 `mod components; mod controller; mod pages;`。

- [ ] **Step 4: 编译验证**

Run: `cargo build`
Expected: PASS（旧 App 逻辑 + 新空模块共存）

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: introduce Page trait and view-state types, migrate ui.rs to ui/mod.rs"
```

---

### Task 11: ui/components —— 通用渲染组件

把 `ui/mod.rs` 内的自由渲染函数拆到 components，样式辅助拆到 `components/style.rs`。

**Files:**
- Modify: `lazycargo_tui/src/ui/components/panel.rs`
- Modify: `lazycargo_tui/src/ui/components/menu.rs`
- Modify: `lazycargo_tui/src/ui/components/scrollbar.rs`
- Modify: `lazycargo_tui/src/ui/components/search_input.rs`
- Modify: `lazycargo_tui/src/ui/components/command_log.rs`
- Modify: `lazycargo_tui/src/ui/components/style.rs`
- Create: `lazycargo_tui/src/ui/components/tab_bar.rs`、`status_bar.rs`、`dialog.rs`
- Modify: `lazycargo_tui/src/ui/mod.rs`

**Interfaces:**
- Produces（`components::panel`）:
  ```rust
  pub fn render_panel(frame: &mut Frame<'_>, lines: &[String], selected: usize, focused: bool, title: &str, filter_active: bool) -> usize
  ```
  （从 layout.rs:41 的 `render_panel` 改造：去掉 `&App` 参数，改为纯数据输入；内部 `apply_filter`/`list_offset` 从 dashboard.rs 迁入并改签名 `apply_filter(lines: &[String], filter: &str, active: bool) -> Vec<String>`、`list_offset(selected, visible, len) -> usize`）
- Produces（`components::scrollbar`）: `render_scrollbar(frame, area, offset, content_len, visible_rows)`
- Produces（`components::menu`）: `render_menu(frame, title: &str, items: &[&str], selected: usize)`
- Produces（`components::search_input`）: `render_search_input(frame, query: &str, focused: bool)`
- Produces（`components::command_log`）: `render_command_log(frame, lines: &[Line<'static>])`
- Produces（`components::style`）: `panel_block(title, style) -> Block<'static>`、`output_line_to_lines(...)`、`semantic_output_line(...)`、`feature_marker_line(...)` 等（从 style.rs 原样迁，签名不变）
- Produces（`components::tab_bar`）: `render_tab_bar(frame, area, tabs: &[(&'static str, bool)])`
- Produces（`components::status_bar`）: `render_status_bar(frame, area, left: &str, right: &str)`
- Produces（`components::dialog`）: `render_dialog(frame, area, title: &str, lines: &[String])`
- Consumes: `MouseState`（tab_bar/menu 交互区写入由各 page 处理）

- [ ] **Step 1: 拆分样式与通用渲染**

从 `src/ui/mod.rs`（旧 ui.rs）及 `src/ui/layout.rs`（若未拆，旧 layout.rs 仍在 `src/ui/`？—— Task 10 移动后旧 layout.rs/dashboard.rs/style.rs 已随 `src/ui.rs` 一起在 `src/ui/` 下）把下列函数移到对应 components 文件，改 `&App` 参数为数据参数（见 Interfaces）：
- `render_panel` → `components/panel.rs`
- `render_scrollbar` → `components/scrollbar.rs`
- `render_menu` → `components/menu.rs`
- `render_search_input` → `components/search_input.rs`
- `render_command_log` → `components/command_log.rs`
- `panel_block`/`output_line_to_lines`/`semantic_output_line`/其余 `*_line` 函数 → `components/style.rs`
- `key_dialog_lines`/`key_line` → 移入 `components/menu.rs` 私有函数

新建 `tab_bar.rs`/`status_bar.rs`/`dialog.rs`，实现 `render_tab_bar`/`render_status_bar`/`render_dialog`（见 Interfaces 签名，参考现有 layout.rs 的样式惯例）。

旧 `src/ui/layout.rs`、`src/ui/dashboard.rs`、`src/ui/style.rs` 在内容搬完后删除（dashboard.rs 的部分函数归 view，Task 12；只剩 apply_filter/list_offset 归 panel）。

- [ ] **Step 2: 接入 `src/ui/mod.rs`**

旧 mod.rs 中 `mod dashboard; mod layout; mod style; mod terminal_support;` 删除，保留 `use components::...`。`App` 内所有调用点改签名（`render_panel(frame, app, area, focus, lines, selected)` → `render_panel(frame, &lines, selected, focused, title, filter_active)`）。

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS（逐点修编译器提示）

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: extract stateless ui components"
```

---

### Task 12: ui/pages/workspace（controller + view）

**Files:**
- Create: `lazycargo_tui/src/ui/pages/workspace/controller.rs`
- Create: `lazycargo_tui/src/ui/pages/workspace/view.rs`
- Modify: `lazycargo_tui/src/ui/pages/workspace/mod.rs`
- Modify: `lazycargo_tui/src/ui/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct WorkspacePage { pub view: WorkspaceView, pub title_pending: Option<String> }
  impl Page for WorkspacePage { ... }
  impl WorkspacePage {
      pub fn items_for_focus(&self, core: &CoreState) -> (Vec<String>, usize); // 依赖列表/构建项/workspace 项，按当前焦点
      pub fn run_command(&mut self, core: &mut CoreState, kind: CargoTaskKind); // 组装 CargoTask + 调用 core 命令
  }
  ```
- Consumes: `controller::{Page, WorkspaceView, Focus, FocusPanel, WorkspaceTab, BuildCoreTab, DependenciesTab, ContextTab, MouseState}`；`state::NavigationState`（全局 focus/input_mode 由根 App 在调 render 前写入 view 或作为参数传入）；`core::model::CoreState`；`core::command::{CargoTask, CargoTaskKind, TaskScope, FeatureSelection, Profile}`；`components::*`；`dashboard.rs` 的文本生成函数（`output_lines`/`workspace_items`/`build_items`/`dependency_items_for`/`*_lines`）迁入 `view.rs` 并改签名 `fn(core: &CoreState, view: &WorkspaceView, nav: &NavigationState) -> Vec<String>`

- [ ] **Step 1: 迁移 dashboard 文本生成到 `view.rs`**

`dashboard.rs` 的 `output_lines`、`workspace_detail_lines`、`build_detail_lines`、`dependency_detail_lines`、`workspace_metrics_lines`、`target_analysis_lines`、`dependency_tree_lines`、`feature_state_lines`、`dependency_path_lines`、`history_lines`、`selected_dependency`、`selected_package`、`package_targets`、`package_source_size_label`、`package_target_cache_label`、`disk_pressure_bar`、`selected_item_detail`、`selected_dependency_detail`、`workspace_items`、`build_items`、`dependency_items_for` 全部迁入 `view.rs`。

签名统一改造：凡取 `app.navigation.ws_tab` → `view.ws_tab`；`app.navigation.focus`/`current_focus` → `nav.focus`/`nav.current_focus`；`app.selection.*` → `view.selected.*`；`app.workspace.project` → `core.project`；`app.workspace.disk` → `core.disk`；`app.build_history` → `core.history`；`app.output_lines_for(slot)` → `core.slot_lines(slot)`；`app.output_store` → `core.output`；`app.history`（命令日志）→ 由参数传入 `&[HistoryEntry]`。

- [ ] **Step 2: 写 `controller.rs`**

```rust
use crossterm::event::KeyEvent;
use ratatui::Frame;

use crate::core::command::CargoTaskKind;
use crate::core::model::CoreState;
use crate::state::NavigationState;
use crate::ui::components;
use crate::ui::controller::{MouseState, Page, WorkspaceView};
use crate::ui::keymap;

pub struct WorkspacePage {
    pub view: WorkspaceView,
    pub pending_message: Option<String>,
}

impl Page for WorkspacePage {
    fn handle_key(&mut self, core: &mut CoreState, key: KeyEvent) -> bool {
        // 翻译 NormalKeyAction（复用 keymap::normal_key_action + self.view 上下文）
        // 视图类动作：MoveUp/MoveDown/Activate/SwitchTab/FocusNext/OpenFilter/ScrollRight…
        // core 类动作：CargoCheck/CargoBuild/TreeOffline/… → self.run_command(core, kind) 或 self.trigger_tree(core)
        // 返回 false 表示退出（Quit）
    }
    fn handle_tick(&mut self, core: &mut CoreState) {
        core.poll_disk_snapshot();
        core.poll_search_job(); // 副作用项（last_status/message/history）暂存 pending，由 render 前 flush 到状态栏
    }
    fn render(&self, core: &CoreState, frame: &mut Frame<'_>, area: Rect) -> MouseState {
        // 左 3 面板（render_panel 组件） + 右 output（ContextTab 分派 → view::render 文本 → 组件渲染）
        // 装配 tab_bar / scrollbar / status_bar
    }
    fn title(&self) -> &'static str { "[1]-Workspace" }
}
```

按键路由的核心逻辑（`handle_normal_key` 的 `match`，ui.rs:757-840）迁入，`self.*` 替换为 `core.*`/`self.view.*`；`process` 相关迁移在 Task 16 与根 App 协调，此处 `run_command` 先实现完整调用（组装 `CargoTask` 并调用 `core` 的进程方法，进程方法在 Task 13 由根 App 提供公共函数或迁入 `CoreState`）。

**简化决策**：为避免双向借用，把"命令执行 + 进程管理 + 输出缓冲刷新"作为 `CoreState` 方法（`core.run_cargo(&mut self, spec, slot)`），由所有 page 共用。本任务先加 `CoreState::run_cargo`，把 ui.rs:1098-1147 的 `run_cargo`/`drain_all_streams`/`finish_cargo_output` 逻辑迁入 core。

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "refactor: workspace page controller and view"
```

---

### Task 13: ui/pages/search 与 ui/pages/init

**Files:**
- Create: `lazycargo_tui/src/ui/pages/search/controller.rs`
- Create: `lazycargo_tui/src/ui/pages/search/view.rs`
- Modify: `lazycargo_tui/src/ui/pages/search/mod.rs`
- Create: `lazycargo_tui/src/ui/pages/init.rs`
- Modify: `lazycargo_tui/src/ui/mod.rs`

**Interfaces:**
- Produces:
  ```rust
  pub struct SearchPage { pub search_return: Option<Focus> }
  impl Page for SearchPage { ... }
  // render_search_page 从 ui.rs:1873 迁入 view.rs，读 core.search.state + 组件
  ```
  ```rust
  pub struct InitPage; // render_project_new_confirm 从 layout.rs 迁入；handle_key 用 keymap::project_new_confirm_action
  ```
- Consumes: `keymap::*`、`components::*`、`core::model::CoreState`、`core::task::{run_search_job, run_info_job, SearchJobConfig}`、`lazycargo_search::{SearchLinkTarget, SearchState}`

- [ ] **Step 1: 实现 SearchPage**

从 `ui.rs` 迁：`render_search_page`（1873-1973）→ `view.rs`（读 `core.search.state`）；`handle_crate_search_key`（874-897）→ controller；`open_search`/`search_crates`（1466-1492）/`search_job_config`（1493-1500）/`inspect_selected_crate`（1588-1618）/`record_crate_inspection`/`open_search_link`/`copy_search_detail` → controller。搜索任务用 `core::task` 的函数 + `std::thread` 生成 `Receiver<SearchJobResult>` 存入 `core.search_receiver`。

注意 `SearchJobConfig` 需要 `core.config` 的 `network_timeout`/`cargo_info_timeout`/`search_limit`。

- [ ] **Step 2: 实现 InitPage**

`render_project_new_confirm`（layout.rs）迁入 `init.rs`；`handle_project_new_confirm_key`（ui.rs:899-914）迁入；`is_project_init_command`（ui.rs:1727）迁入 core 或 cli 侧。

- [ ] **Step 3: 接线到 `ui/mod.rs`**

根 App 的 `PageId` 路由：`search.state.expanded` → SearchPage；`input_mode == ProjectNewConfirm` → InitPage overlay。Task 16 完成根 App 重写后接管；本任务先保证三个 page 编译并可由根 App 临时调用。

- [ ] **Step 4: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: search and init pages"
```

---

### Task 14: ui/pages/docs（文档阅读页）+ reader 组件

**Files:**
- Create: `lazycargo_tui/src/ui/pages/docs/controller.rs`
- Create: `lazycargo_tui/src/ui/pages/docs/view.rs`
- Modify: `lazycargo_tui/src/ui/pages/docs/mod.rs`
- Create: `lazycargo_tui/src/ui/components/reader.rs`
- Modify: `lazycargo_tui/src/ui/mod.rs`
- Modify: `lazycargo_tui/src/core/model.rs`（`DocsModel` + `OutputSlot::DocsReadme`/`DocsFallback` 已在 Task 8 占位，此处填充字段）

**Interfaces:**
- Produces:
  ```rust
  pub struct DocsModel { pub name: String, pub version: String, pub source: DocsSource, pub text: String }
  pub enum DocsSource { Readme, Description }
  ```
  ```rust
  // core 方法
  impl CoreState {
      pub fn open_docs(&mut self, name: &str, version: &str, timeout: Duration) -> Result<(), DocsError>;
      // 后台线程 fetch_readme → 成功写 OutputSlot::DocsReadme + DocsSource::Readme；
      // 失败回退 fetch_description → DocsSource::Description；都失败写错误行到 DocsFallback
  }
  ```
  ```rust
  pub struct DocsPage { pub view: DocsView }
  impl Page for DocsPage { fn handle_key: j/k 或 arrows 滚动（复用 ContextOutput 的 scroll 语义，本页用 DocsView.scroll） }
  ```
  ```rust
  // components::reader
  pub fn render_reader(frame: &mut Frame<'_>, area: Rect, markdown: &str, scroll: usize, title: &str)
  // 内部：tui_markdown::convert(markdown) -> Text（tui-markdown 0.3.9 的转换 API，若名称为 render/convert 以 docs.rs 为准），
  // 用 ratatui Paragraph + 手动 offset 实现滚动；宽限 wrap
  ```
- Consumes: `core::docs::{fetch_readme, fetch_description, DocsError}`、`tui_markdown`、`components::scrollbar`

- [ ] **Step 1: 填充 `core/model.rs` DocsModel**

加字段 + `CoreState::open_docs`（用 `std::thread::spawn` + `mpsc::channel` 异步取 README，结果存 `core.output`；同步失败回退逻辑在接收线程里完成）。接收结果写入 `OutputSlot::DocsReadme`。`docs_readme(name, version)` 辅助拼 `core::docs::docs_url`。

- [ ] **Step 2: 写失败测试（core.open_docs 的回退链路）**

在 `core/model.rs` 测试中验证：无网络时 `open_docs` 不 panic，最终 `OutputSlot` 存在失败说明行。此测试不做真实网络（用 `fetch_description` 传 `Duration::from_millis(1)` 触发超时）。

- [ ] **Step 3: 实现 `components/reader.rs`**

```rust
use ratatui::layout::{Margin, Rect};
use ratatui::text::Text;
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

pub fn render_reader(frame: &mut Frame<'_>, area: Rect, markdown: &str, scroll: usize, title: &str) {
    let text = tui_markdown::convert(markdown);
    let inner = area.inner(&Margin { vertical: 1, horizontal: 1 });
    let visible = inner.height as usize;
    let total = text.height();
    let offset = scroll.min(total.saturating_sub(visible));
    frame.render_widget(
        Paragraph::new(text)
            .scroll((offset as u16, 0))
            .block(Block::default().title(title).borders(Borders::ALL))
            .wrap(Wrap { trim: false }),
        area,
    );
}
```

`tui_markdown::convert` 的确切签名以 `cargo doc -p lazycargo-tui` 或 `docs.rs` 为准（0.3.9 API：`pub fn convert(text: &str) -> Text`，若为其他拼写按编译提示调整）。

- [ ] **Step 4: 实现 DocsPage**

controller：`Page::handle_key` 处理 `j/k`/上下箭头（`scroll += delta`，钳制 0..total）。`handle_tick` 无。`render` 调 `render_reader(frame, area, &core.output[&DocsReadme].lines.join("\n"), self.view.scroll, title)`；无内容时显示占位。view.rs 提供 `detail_lines(core, name, version) -> Vec<String>`（来源/url/时长）。

- [ ] **Step 5: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test --lib model::tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: docs reading page with tui-markdown reader"
```

---

### Task 15: 根 App（ui/mod.rs）重写 + 删除旧 App

把 `src/ui/mod.rs` 中残留的旧 `App` 全部方法迁移完毕，根 App 重写为装配器，删除旧 `App`/`HistoryEntry`/`MouseState`（后者已迁 controller）。

**Files:**
- Modify: `lazycargo_tui/src/ui/mod.rs`
- Modify: `lazycargo_tui/src/ui/pages/workspace/controller.rs`（`handle_tick` 副作用 flush 到根 App 的 navigation）

**Interfaces:**
- Produces:
  ```rust
  pub struct App {
      pub core: CoreState,
      pub navigation: NavigationState,
      pub history: Vec<HistoryEntry>,   // 命令日志（保留在根）
      pub page: WorkspacePage,          // 当前活动页
      pub docs: DocsPage,
  }
  pub fn run() -> io::Result<()>;
  ```
  事件循环（原 ui.rs:1767 `run_app`）：tick 250ms → 调 `page.handle_tick(&mut core)` → 各 page poll → 渲染当前页；按键 → `page.handle_key(&mut core, key)`；搜索展开状态决定渲染 SearchPage 还是 WorkspacePage；`ProjectNewConfirm` overlay 走 InitPage。

- [ ] **Step 1: 迁移剩余 App 方法到 pages / core**

清点并迁移（均已在前序任务覆盖或此处收尾）：
- `handle_mouse`/`link_areas`/`tab_areas`/`panel_areas`/`mouse_over_focus`/`set_focus`/`scrollbar_to_row` → workspace controller 的 `render` 内联（用 `MouseState`）
- `run_cargo`/`drain_all_streams`/`finish_cargo_output`/`record_build_history` → `CoreState`（Task 12 已开始）
- `kill_running_child`/`toggle_copy_mode` → workspace controller
- `reload_project_after_init`/`fallback_project_info` → `core/project.rs` 或 ui/mod.rs 顶层
- `setup_terminal`/`restore_terminal` → `ui/mod.rs` 私有函数
- `project_health_snapshot`/`load_disk_snapshot_async`/`dir_size`/`latest_crate_timings`/`collect_crate_timings` → `core/target_analyzer.rs`（`load_disk_snapshot_async` 涉及 `thread`+`mpsc`，迁 core 合理；`latest_crate_timings`/`collect_crate_timings` 用 serde_json 解析 `build_history` 的 timings，迁 `core/build_history.rs`）

- [ ] **Step 2: 重写 `ui/mod.rs` 根 App**

按 Interfaces 写 `App` + `run()` + `run_app` 事件循环。删除旧的 `impl App` 及所有旧自由函数（确认无引用后）。`history: Vec<HistoryEntry>` 迁至根（HistoryEntry 定义留在 `ui/mod.rs`）。

- [ ] **Step 3: 编译验证**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: PASS

Run: `cargo clippy --all-targets -- -D warnings`
Expected: PASS（如 dead code 提示，清理）

- [ ] **Step 4: 手动冒烟**

在终端运行 `cargo run --` 进入 TUI，验证：面板渲染、`j/k` 移动、`Tab` 切换、`c/b` 触发 check/build、`[`/`]` 切 tab、`s` 进搜索、`q` 退出。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "refactor: rewrite root App as page assembler, drop god object"
```

---

### Task 16: CLI 真执行 + clap 解析（cli.rs）

**Files:**
- Create: `lazycargo_tui/src/cli.rs`
- Modify: `lazycargo_tui/src/main.rs`
- Delete: `lazycargo_tui/src/args.rs`
- Modify: `lazycargo_tui/src/lib.rs`

**Interfaces:**
- Produces:
  ```rust
  #[derive(clap::Parser)]
  #[command(name = "lazycargo", about = "a lazygit-style Cargo workspace TUI")]
  pub struct Cli {
      #[command(subcommand)]
      pub command: Command,
  }
  #[derive(clap::Subcommand)]
  pub enum Command {
      Check(TaskArgs), Build(TaskArgs), Test(TestArgs), Clippy(TaskArgs),
      Doc(TaskArgs), Run(RunArgs), Update(UpdateArgs),
      Add(AddArgs), Search(SearchArgs), Config,
  }
  pub fn run_cli() -> anyhow::Result<()>; // 解析 → 执行 → 打印
  ```
  ```rust
  // 公共执行器
  fn execute(spec: &CommandSpec, cwd: &Path) -> anyhow::Result<()>
  // 用 Command::new("cargo") 继承 stdout/stderr，exit code 传播
  ```
- Consumes: `clap`、`anyhow`、`core::command::{CargoTask, CargoTaskKind, CommandSpec}`、`core::process::run_captured`
- 兼容性：`lazycargo check -p app -F sqlite,serde --release` 等现有测试行为由 clap 复现；`config` 子命令打印配置

- [ ] **Step 1: 写失败测试 `tests/cli.rs`（替换 tests/cli_args.rs）**

```rust
// tests/cli.rs —— 用 clap 的 CommandFactory::command() 做无副作用断言
use clap::CommandFactory;
use lazycargo::cli::Cli;

#[test]
fn check_command_builds_task_args() {
    use clap::Parser;
    let cli = Cli::try_parse_from(["lazycargo", "check", "-p", "app", "-F", "sqlite,serde", "--release"]).unwrap();
    let Command::Check(args) = cli.command else { panic!("expected check") };
    let task: CargoTask = args.into();
    assert_eq!(task.to_command().display(), "cargo check -p app --release --features serde,sqlite");
}
```

同时覆盖：test filter+nocapture、run --bin --args、add dev/build、search --limit、config。这些测试要求 `CargoTask: From<TaskArgs>`。删除 `tests/cli_args.rs`。

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --test cli`
Expected: FAIL（`cli` 模块不存在）

- [ ] **Step 3: 实现 `src/cli.rs`**

`TaskArgs`/`TestArgs`/`RunArgs`/`UpdateArgs`/`AddArgs`/`SearchArgs` 用 clap derive，每个实现 `From<XxxArgs> for CargoTask`/`DependencyAddPlan`/`CrateSearchQuery`，语义对齐旧 `parse_args`（`--workspace`→`TaskScope::Workspace`、`--release`→`Profile::Release`、`-F` 逗号拆分、`--all-features` 与 `--features` 互斥报错、`--dev`/`--build` 互斥）。`Command::Config` 打印 `serde_json::to_string_pretty(&AppConfig::load_or_create()?)`。

`execute` 用 `std::process::Command` 继承 IO，返回 `anyhow::Result<()>`，非零退出码 `bail!("command failed with exit code {code}")`。

- [ ] **Step 4: 改 `src/main.rs`**

```rust
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() {
        return lazycargo::ui::run().map_err(anyhow::Error::from);
    }
    lazycargo::cli::run_cli()
}
```

`src/lib.rs` 加 `pub mod cli;`，删除 `pub use args::{...}` 与 `pub mod args;`（转发）。`cargo_task`/`crates` 的 `pub use` 保留（tests 用）。

- [ ] **Step 5: 编译 + 测试**

Run: `cargo build`
Expected: PASS

Run: `cargo test --test cli`
Expected: PASS

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: 手动验证真执行**

Run: `cargo run -- check` （在 `lazycargo_tui` 目录）
Expected: 打印 `cargo check` 并真实执行（exit code 与 cargo 一致）

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "feat: cli executes commands for real via clap, drop hand-rolled parser"
```

---

### Task 17: 智能默认 scope

**Files:**
- Modify: `lazycargo_tui/src/cli.rs`

**Interfaces:**
- Produces:
  ```rust
  fn auto_scope(cwd: &Path, project: &ProjectInfo) -> TaskScope
  // 规则：
  //   单 package → TaskScope::CurrentPackage
  //   workspace root（cwd 在 workspace_root 内且不含 member manifest 的子目录）→ TaskScope::Workspace
  //   member 目录内 → TaskScope::Package(member_name)（最长匹配 manifest_path 前缀）
  pub fn smart_scope_label(scope: &TaskScope) -> &'static str // "auto-scope" 提示用
  ```
- Consumes: `core::project::ProjectInfo`（用 `cargo_metadata` 现成 packages/manifest_path）

- [ ] **Step 1: 写失败测试**

`tests/cli.rs` 追加：

```rust
#[test]
fn auto_scope_picks_member_for_cwd_inside_package() {
    // 构造 ProjectInfo：workspace_root=/ws, workspace_packages 两个 member manifest /ws/a/Cargo.toml, /ws/b/Cargo.toml
    // cwd=/ws/a/src → TaskScope::Package("a")
}
#[test]
fn auto_scope_uses_workspace_at_root() {
    // cwd=/ws → TaskScope::Workspace
}
```

- [ ] **Step 2: 运行验证失败**

Run: `cargo test --test cli auto_scope`
Expected: FAIL（`auto_scope` 未定义）

- [ ] **Step 3: 实现 `auto_scope`**

在 `cli.rs` 用 `ProjectInfo::load()`（需在 cwd 执行，`MetadataCommand` 默认读当前目录）。实现最长前缀匹配：对每个 workspace package 的 `manifest_path`，若 `cwd` 是该 manifest 所在目录或其后代，选路径最深的那个 package。

- [ ] **Step 4: 接线**

`run_cli` 中对 Check/Build/Test/Clippy/Doc/Run 分支：若用户未显式传 `-p`/`--workspace`（clap 的 Option 为空），则用 `auto_scope` 填 `task.scope`，并打印 `running: {spec.display()} ({smart_scope_label})`；否则打印 `running: {spec.display()}`。

- [ ] **Step 5: 测试**

Run: `cargo test --test cli`
Expected: PASS

Run: `cargo build`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: smart default scope for build-family commands"
```

---

### Task 18: doc 集成接线（快捷键 + 本地打开 + CLI doc）

**Files:**
- Modify: `lazycargo_tui/src/ui/pages/workspace/controller.rs`（依赖面板 `D`/`d` 快捷键）
- Modify: `lazycargo_tui/src/ui/pages/search/controller.rs`（`d` 进入 docs 页）
- Modify: `lazycargo_tui/src/ui/mod.rs`（DocsPage 路由）
- Modify: `lazycargo_tui/src/core/task.rs` 或新建 `core/doc_runner.rs`（doc 任务）
- Modify: `lazycargo_tui/src/cli.rs`（`doc` 成功后 open）

**Interfaces:**
- Produces:
  ```rust
  // core（供 UI/CLI 复用）
  impl CoreState {
      pub fn run_doc_task(&mut self, scope: TaskScope) -> Result<(), ProcessError>; // CargoTask Doc + run_cargo
  }
  pub fn open_local_docs(project: &ProjectInfo, name: &str) -> io::Result<()>; // 复用 ui::terminal_support::open_url
  ```
- Consumes: `core::docs::{docs_url, local_doc_path}`、`open`（现有依赖）、`terminal_support::{open_url}`（`first_url` 迁 `core/util.rs`）

- [ ] **Step 1: workspace 依赖面板快捷键**

`keymap.rs` 增加 `NormalKeyAction::{OpenDocsJump, OpenDocsRead}`；workspace controller `handle_key` 依赖焦点下：`D` → `open::that(docs_url(name, version))`；`d` → `core.open_docs(name, version, timeout)` 并切到 DocsPage。`HELP_SECTIONS` 的 Actions 段补 `d / D  read docs / open docs.rs`。

- [ ] **Step 2: search 页 `d` 键**

search controller 已有 `OpenDocs`（打开浏览器）；新增 `d`（小写）进入 docs 阅读：`self.open_docs(core, name, version)` + 根 App 切页。

- [ ] **Step 3: build 面板 doc 构建完成打开本地**

`CoreState::run_cargo` 的 `finish` 路径：若命令是 `doc` 且成功，找 `target/doc/index.html` 或所选 package 的 `local_doc_path`，存在则 `open::that`。在 `record_build_history` 处追加。

- [ ] **Step 4: CLI `lazycargo doc`**

`cli.rs` `Command::Doc` 分支：`execute` 成功后解析 `target/doc/<name>/index.html`（单 package 用 package 名，workspace 用 `index.html`）→ `open::that`。超时/无路径时静默。

- [ ] **Step 5: 编译 + 测试**

Run: `cargo build`
Expected: PASS

Run: `cargo test`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: doc integration - jump/read/CLI/open-local"
```

---

### Task 19: 收尾（clippy/fmt/全量测试/文档）

**Files:**
- Modify: 修复 clippy/fmt 报出的所有问题
- Modify: `README.md` / `README.zh-CN.md`（CLI 说明改为"真执行 + 智能 scope"，快捷键表补 doc 键位）
- Modify: `docs/TESTING.md` 若有 CLI 描述

- [ ] **Step 1: 全量验证**

Run: `cargo fmt --check`（先 `cargo fmt` 再 check）
Run: `cargo clippy --all-targets -- -D warnings`
Run: `cargo test`
Run: `cargo build --release`
Expected: 全部 PASS

- [ ] **Step 2: 删除确认**

确认已删：`src/ui.rs`、`src/ui/dashboard.rs`、`src/ui/layout.rs`、`src/ui/style.rs`、`src/ui/search_job.rs`、`src/ui/runner.rs`、`src/state/mod.rs`（若内容已迁）、`src/metadata.rs`、`src/cargo_task.rs`、`src/crates.rs`、`src/args.rs`。

- [ ] **Step 3: 更新文档**

README 快捷键表加：`d` 阅读文档 / `D` 打开 docs.rs；CLI 段改为真执行+智能 scope 说明。说明 lazycargo 依赖新增（clap/tui-markdown 等）。**把 README 的 rust 版本徽章从 1.81 更新为 1.88**（`docs/pics` 无关，改 README.md 与 README.zh-CN.md 中 `rust-1.81+` 字样）。

- [ ] **Step 4: 最终手动验收**

在真实 workspace 跑：`lazycargo`（TUI 冒烟）、`lazycargo check`、`lazycargo doc`、TUI 内 `d`/`D`、构建面板 doc 任务后自动打开。

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore: finalize refactor - clippy clean, docs updated"
```

---

## 自审备注

- Spec 的"业务/视图状态分离"由 Task 8（CoreState）+ Task 10（WorkspaceView 等）落实。
- Spec 的"控制器不持有 core 引用"由 `Page` trait 的 `&mut CoreState` 参数落实（Task 10）。
- Spec 的 CLI 三目标（真执行/clap/智能 scope）由 Task 16/17 落实。
- Spec 的 doc 四形态由 Task 7（core/docs）、Task 14（阅读页）、Task 18（接线）落实。
- Spec 的 clap/thiserror/anyhow/tempfile 由 Task 1/4 落实，tui-markdown 由 Task 14 落实。
- Spec 的 store 统一持久化由 Task 4 落实。
- 残余风险：`tui_markdown::convert` 的确切 API 名需在 Task 14 编译时按 docs.rs 校准；`NamedTempFile::persist` 在个别平台的行为差异需现场验证。
