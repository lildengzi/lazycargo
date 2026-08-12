use std::path::Path;

use anyhow::Context;
use clap::{Args, Parser, Subcommand};

use crate::core::command::{CargoTask, CargoTaskKind, CommandSpec, FeatureSelection, Profile, TaskScope};
use crate::core::config::AppConfig;
use crate::core::process::run_captured;
use crate::core::project::ProjectInfo;
use crate::core::search::{CrateSearchQuery, DependencyAddPlan, DependencyKind};

/// a lazygit-style Cargo workspace TUI
#[derive(Debug, Parser)]
#[command(name = "lazycargo", version, about)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Run `cargo check`
    #[command(alias = "c")]
    Check(TaskArgs),
    /// Run `cargo build`
    #[command(alias = "b")]
    Build(TaskArgs),
    /// Run `cargo test`
    #[command(alias = "t")]
    Test(TestArgs),
    /// Run `cargo clippy`
    #[command(alias = "l")]
    Clippy(TaskArgs),
    /// Run `cargo doc`
    #[command(alias = "d")]
    Doc(TaskArgs),
    /// Run `cargo run`
    #[command(alias = "r")]
    Run(RunArgs),
    /// Update dependencies
    #[command(alias = "u")]
    Update(UpdateArgs),
    /// Add a dependency
    #[command(alias = "a")]
    Add(AddArgs),
    /// Search crates.io
    #[command(alias = "s")]
    Search(SearchArgs),
    /// Print the effective configuration
    Config,
}

#[derive(Debug, Clone, Args)]
pub struct TaskArgs {
    /// Operate on a specific workspace member
    #[arg(short = 'p', long = "package")]
    pub package: Option<String>,

    /// Operate on all workspace members
    #[arg(long)]
    pub workspace: bool,

    /// Use the release profile
    #[arg(long)]
    pub release: bool,

    /// Build for a specific target triple
    #[arg(long)]
    pub target: Option<String>,

    /// Comma-separated list of features to enable
    #[arg(short = 'F', long = "features", value_delimiter = ',')]
    pub features: Vec<String>,

    /// Enable every feature of the target package
    #[arg(long, conflicts_with = "features")]
    pub all_features: bool,

    /// Disable the package's default features
    #[arg(long)]
    pub no_default_features: bool,
}

impl From<TaskArgs> for CargoTask {
    fn from(args: TaskArgs) -> Self {
        let scope = if args.workspace {
            TaskScope::Workspace
        } else if let Some(package) = args.package {
            TaskScope::Package(package)
        } else {
            TaskScope::CurrentPackage
        };

        let mut task = CargoTask::new(CargoTaskKind::Check);
        task.scope = scope;
        task.features = FeatureSelection {
            features: normalize_features(args.features),
            all_features: args.all_features,
            no_default_features: args.no_default_features,
        };
        task.profile = if args.release {
            Profile::Release
        } else {
            Profile::Dev
        };
        task.target = args.target;
        task
    }
}

#[derive(Debug, Clone, Args)]
pub struct TestArgs {
    #[command(flatten)]
    pub task: TaskArgs,

    /// Test name filter
    pub filter: Option<String>,

    /// Run tests without output capture (forwards `-- --nocapture`)
    #[arg(long)]
    pub nocapture: bool,

    /// Extra arguments passed through after `--`
    #[arg(last = true)]
    pub extra: Vec<String>,
}

impl From<TestArgs> for CargoTask {
    fn from(args: TestArgs) -> Self {
        let mut task = CargoTask::from(args.task);
        let nocapture = args.nocapture || args.extra.iter().any(|arg| arg == "--nocapture");
        let mut extra_args = Vec::new();
        if !nocapture && !args.extra.is_empty() {
            extra_args.push("--".to_owned());
            extra_args.extend(args.extra);
        }
        task.kind = CargoTaskKind::Test {
            filter: args.filter,
            nocapture,
        };
        task.extra_args = extra_args;
        task
    }
}

#[derive(Debug, Clone, Args)]
pub struct RunArgs {
    #[command(flatten)]
    pub task: TaskArgs,

    /// Binary to run
    #[arg(long)]
    pub bin: Option<String>,

    /// Arguments passed to the binary after `--`
    #[arg(last = true)]
    pub args: Vec<String>,
}

impl From<RunArgs> for CargoTask {
    fn from(args: RunArgs) -> Self {
        let mut task = CargoTask::from(args.task);
        task.kind = CargoTaskKind::Run {
            bin: args.bin,
            args: args.args,
        };
        task
    }
}

#[derive(Debug, Clone, Args)]
pub struct UpdateArgs {
    #[command(flatten)]
    pub task: TaskArgs,

    /// Crate name to update
    pub package: Option<String>,
}

impl From<UpdateArgs> for CargoTask {
    fn from(args: UpdateArgs) -> Self {
        let mut task = CargoTask::from(args.task);
        task.kind = CargoTaskKind::Update {
            package: args.package.clone(),
        };
        if let Some(package) = args.package {
            task.extra_args.push("-p".into());
            task.extra_args.push(package);
        }
        task
    }
}

#[derive(Debug, Clone, Args)]
pub struct AddArgs {
    /// Crate to add
    pub name: String,

    /// Package whose manifest to modify
    #[arg(short = 'p', long = "package")]
    pub package: Option<String>,

    /// Comma-separated features of the dependency to enable
    #[arg(short = 'F', long = "features", value_delimiter = ',')]
    pub features: Vec<String>,

    /// Add as a dev dependency
    #[arg(long, conflicts_with = "build")]
    pub dev: bool,

    /// Add as a build dependency
    #[arg(long)]
    pub build: bool,

    /// Mark the dependency as optional
    #[arg(long)]
    pub optional: bool,
}

impl From<AddArgs> for DependencyAddPlan {
    fn from(args: AddArgs) -> Self {
        let kind = if args.dev {
            DependencyKind::Dev
        } else if args.build {
            DependencyKind::Build
        } else {
            DependencyKind::Normal
        };
        DependencyAddPlan {
            name: args.name,
            package: args.package,
            features: normalize_features(args.features),
            kind,
            optional: args.optional,
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct SearchArgs {
    /// Search query
    pub query: String,

    /// Maximum number of results
    #[arg(long, default_value_t = 20)]
    pub limit: usize,
}

impl From<SearchArgs> for CrateSearchQuery {
    fn from(args: SearchArgs) -> Self {
        CrateSearchQuery {
            query: args.query,
            limit: args.limit,
        }
    }
}

fn normalize_features(values: Vec<String>) -> Vec<String> {
    let mut features = values
        .into_iter()
        .flat_map(|value| {
            value
                .split(',')
                .flat_map(str::split_whitespace)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    features.sort();
    features.dedup();
    features
}

/// Parse `std::env::args` and execute the resolved Cargo command.
pub fn run_cli() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir()?;
    match cli.command {
        Command::Check(args) => execute_task(command_task(Command::Check(args)), &cwd),
        Command::Build(args) => execute_task(command_task(Command::Build(args)), &cwd),
        Command::Test(args) => execute_task(CargoTask::from(args), &cwd),
        Command::Clippy(args) => execute_task(command_task(Command::Clippy(args)), &cwd),
        Command::Doc(args) => execute_doc(command_task(Command::Doc(args)), &cwd),
        Command::Run(args) => execute_task(CargoTask::from(args), &cwd),
        Command::Update(args) => execute(&CargoTask::from(args).to_command(), &cwd, false),
        Command::Add(args) => execute(&DependencyAddPlan::from(args).to_command(), &cwd, false),
        Command::Search(args) => run_search(&CrateSearchQuery::from(args)),
        Command::Config => print_config(),
    }
}

/// 从解析后的子命令构造 CargoTask；`From<TaskArgs>` 一律落到 Check，
/// 因此这里按子命令把 kind 设回目标值（Doc/Build/Clippy 会真实执行对应子命令）。
/// Test/Run/Update 有自己的 From 实现（含 filter/bin/package 等额外参数）。
pub fn command_task(command: Command) -> CargoTask {
    match command {
        Command::Check(args) => task_with_kind(CargoTaskKind::Check, args),
        Command::Build(args) => task_with_kind(CargoTaskKind::Build, args),
        Command::Test(args) => CargoTask::from(args),
        Command::Clippy(args) => task_with_kind(CargoTaskKind::Clippy, args),
        Command::Doc(args) => task_with_kind(CargoTaskKind::Doc, args),
        Command::Run(args) => CargoTask::from(args),
        Command::Update(args) => CargoTask::from(args),
        other => panic!("expected task subcommand, got {other:?}"),
    }
}

fn task_with_kind(kind: CargoTaskKind, args: TaskArgs) -> CargoTask {
    let mut task = CargoTask::from(args);
    task.kind = kind;
    task
}

/// `lazycargo doc` 执行成功后，解析本地生成的文档首页并在浏览器打开。
/// `cargo doc` 只为每个 crate 生成 `target/doc/<crate>/index.html`（无根 index.html），
/// 因此单包/`-p <package>` → `target/doc/<package>/index.html`，
/// workspace → 根 package 的 `target/doc/<root>/index.html`。
fn execute_doc(mut task: CargoTask, cwd: &Path) -> anyhow::Result<()> {
    let auto_scoped = apply_auto_scope(&mut task, cwd);
    execute(&task.to_command(), cwd, auto_scoped)?;
    let project = ProjectInfo::load().ok();
    let workspace_root = project
        .as_ref()
        .map(|project| project.workspace_root.clone())
        .unwrap_or_else(|| cwd.to_string_lossy().into_owned());
    let package = match &task.scope {
        TaskScope::Package(name) => Some(name.clone()),
        _ => project
            .map(|project| project.name)
            .or_else(|| cwd.file_name().map(|name| name.to_string_lossy().into_owned())),
    };
    match docs_index_after_task(Path::new(&workspace_root), package.as_deref()) {
        Some(path) => {
            open::that(&path).map_err(|error| anyhow::anyhow!("failed to open {}: {error}", path.display()))?;
            println!("opened docs: {}", path.display());
        }
        None => println!("docs built, but no target/doc/<crate>/index.html found"),
    }
    Ok(())
}

/// 解析 doc 构建产物首页；workspace 用根 index.html，单包优先 `target/doc/<name>/index.html`。
pub fn docs_index_after_task(workspace_root: &Path, package_name: Option<&str>) -> Option<std::path::PathBuf> {
    crate::core::docs::local_docs_index(&workspace_root.to_string_lossy(), package_name)
}

/// 根据当前工作目录推断 scope。cwd 属于某个 workspace member → Package；在 workspace 根 → Workspace；否则 CurrentPackage。
pub fn auto_scope(cwd: &Path, project: &ProjectInfo) -> TaskScope {
    if project.workspace_packages.len() <= 1 {
        return TaskScope::CurrentPackage;
    }
    let best = project
        .workspace_packages
        .iter()
        .filter_map(|package| {
            let pkg_dir = Path::new(&package.manifest_path).parent()?;
            cwd.starts_with(pkg_dir).then_some((package, pkg_dir.as_os_str().len()))
        })
        .max_by_key(|(_, depth)| *depth);
    if let Some((package, _)) = best {
        return TaskScope::Package(package.name.clone());
    }
    if cwd.starts_with(Path::new(&project.workspace_root)) {
        TaskScope::Workspace
    } else {
        TaskScope::CurrentPackage
    }
}

/// 用户未显式指定 scope 时按 cwd 推断；返回是否应用了自动 scope。
fn apply_auto_scope(task: &mut CargoTask, cwd: &Path) -> bool {
    if task.scope != TaskScope::CurrentPackage {
        return false;
    }
    let Ok(project) = ProjectInfo::load() else {
        return false;
    };
    let scope = auto_scope(cwd, &project);
    if scope == TaskScope::CurrentPackage {
        return false;
    }
    task.scope = scope;
    true
}

fn execute_task(mut task: CargoTask, cwd: &Path) -> anyhow::Result<()> {
    let auto_scoped = apply_auto_scope(&mut task, cwd);
    execute(&task.to_command(), cwd, auto_scoped)
}

/// Run a Cargo command inheriting stdout/stderr; propagate non-zero exits.
fn execute(spec: &CommandSpec, cwd: &Path, auto_scoped: bool) -> anyhow::Result<()> {
    let annotation = if auto_scoped { " (auto-scope)" } else { "" };
    println!("running: {}{}", spec.display(), annotation);
    let status = std::process::Command::new(&spec.program)
        .current_dir(cwd)
        .args(&spec.args)
        .status()
        .with_context(|| format!("failed to run `{}`", spec.display()))?;
    if status.success() {
        return Ok(());
    }
    match status.code() {
        Some(code) => anyhow::bail!("command failed with exit code {code}"),
        None => anyhow::bail!("command failed (terminated by signal)"),
    }
}

fn run_search(query: &CrateSearchQuery) -> anyhow::Result<()> {
    let config = AppConfig::load_or_create()?;
    let limit = query.limit.to_string();
    let output = run_captured(
        "cargo",
        &["search", &query.query, "--limit", &limit],
        config.network_timeout(),
    )
    .map_err(|error| anyhow::anyhow!("failed to run cargo search: {error}"))?;

    let Some(output) = output else {
        anyhow::bail!("cargo search timed out");
    };
    if !output.status.success() {
        anyhow::bail!("cargo search failed: {}", output.status);
    }
    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn print_config() -> anyhow::Result<()> {
    let config = AppConfig::load_or_create()?;
    println!("path: {}", AppConfig::config_path().display());
    println!("{}", serde_json::to_string_pretty(&config)?);
    Ok(())
}
