//! CLI tests: clap-parsed command surface drives real CargoTask construction.
//!
//! Parsing is side-effect free: `Cli::try_parse_from` / `CommandFactory::command()`
//! never spawn `cargo`, so these tests assert command construction only.

use std::path::Path;

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use lazycargo::cli::{auto_scope, Cli, Command};
use lazycargo::{CargoTask, PackageInfo, ProjectInfo, TaskScope};

fn task_from(command: Command) -> CargoTask {
    match command {
        Command::Check(args) => CargoTask::from(args),
        Command::Build(args) => CargoTask::from(args),
        Command::Test(args) => CargoTask::from(args),
        Command::Clippy(args) => CargoTask::from(args),
        Command::Doc(args) => CargoTask::from(args),
        Command::Run(args) => CargoTask::from(args),
        Command::Update(args) => CargoTask::from(args),
        other => panic!("expected task subcommand, got {other:?}"),
    }
}

#[test]
fn check_command_builds_package_scope_release_and_sorted_features() {
    let cli = Cli::try_parse_from([
        "lazycargo",
        "check",
        "-p",
        "app",
        "-F",
        "sqlite,serde",
        "--release",
    ])
    .unwrap();
    let task = task_from(cli.command);

    assert_eq!(
        task.to_command().display(),
        "cargo check -p app --release --features serde,sqlite"
    );
}

#[test]
fn check_command_accepts_workspace_scope_and_target() {
    let cli = Cli::try_parse_from([
        "lazycargo",
        "check",
        "--workspace",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--no-default-features",
    ])
    .unwrap();
    let task = task_from(cli.command);

    assert_eq!(
        task.to_command().display(),
        "cargo check --workspace --target x86_64-unknown-linux-gnu --no-default-features"
    );
}

#[test]
fn test_command_builds_filter_with_nocapture() {
    let cli = Cli::try_parse_from([
        "lazycargo",
        "test",
        "-p",
        "core",
        "user_service",
        "--nocapture",
    ])
    .unwrap();
    let task = task_from(cli.command);

    assert_eq!(
        task.to_command().display(),
        "cargo test -p core user_service -- --nocapture"
    );
}

#[test]
fn run_command_builds_binary_with_passthrough_args() {
    let cli = Cli::try_parse_from([
        "lazycargo",
        "run",
        "--bin",
        "server",
        "--",
        "--port",
        "3000",
    ])
    .unwrap();
    let task = task_from(cli.command);

    assert_eq!(
        task.to_command().display(),
        "cargo run --bin server -- --port 3000"
    );
}

#[test]
fn add_dev_dependency_builds_plan() {
    let cli = Cli::try_parse_from(["lazycargo", "add", "--dev", "tempfile"]).unwrap();
    let Command::Add(args) = cli.command else {
        panic!("expected add");
    };
    let plan = lazycargo::DependencyAddPlan::from(args);

    assert_eq!(plan.kind, lazycargo::DependencyKind::Dev);
    assert_eq!(plan.to_command().display(), "cargo add tempfile --dev");
}

#[test]
fn add_build_optional_dependency_builds_plan() {
    let cli = Cli::try_parse_from(["lazycargo", "add", "cc", "--build", "--optional"]).unwrap();
    let Command::Add(args) = cli.command else {
        panic!("expected add");
    };
    let plan = lazycargo::DependencyAddPlan::from(args);

    assert_eq!(plan.kind, lazycargo::DependencyKind::Build);
    assert!(plan.optional);
    assert_eq!(
        plan.to_command().display(),
        "cargo add cc --build --optional"
    );
}

#[test]
fn add_normal_dependency_builds_plan_with_package_and_features() {
    let cli = Cli::try_parse_from([
        "lazycargo",
        "add",
        "tokio",
        "-p",
        "app",
        "-F",
        "macros,rt-multi-thread",
    ])
    .unwrap();
    let Command::Add(args) = cli.command else {
        panic!("expected add");
    };
    let plan = lazycargo::DependencyAddPlan::from(args);

    assert_eq!(plan.name, "tokio");
    assert_eq!(plan.package.as_deref(), Some("app"));
    assert_eq!(
        plan.to_command().display(),
        "cargo add tokio -p app --features macros,rt-multi-thread"
    );
}

#[test]
fn search_command_builds_query_with_limit() {
    let cli = Cli::try_parse_from(["lazycargo", "search", "tokio", "--limit", "5"]).unwrap();
    let Command::Search(args) = cli.command else {
        panic!("expected search");
    };
    let query = lazycargo::CrateSearchQuery::from(args);

    assert_eq!(query.query, "tokio");
    assert_eq!(query.limit, 5);
}

#[test]
fn config_command_is_recognized() {
    let cli = Cli::try_parse_from(["lazycargo", "config"]).unwrap();
    assert!(matches!(cli.command, Command::Config));
}

#[test]
fn doc_command_runs_cargo_doc_not_check() {
    let cli = Cli::try_parse_from(["lazycargo", "doc"]).unwrap();
    let task = lazycargo::cli::command_task(cli.command);
    assert_eq!(task.to_command().display(), "cargo doc");
}

#[test]
fn doc_command_scopes_to_workspace_member() {
    let cli =
        Cli::try_parse_from(["lazycargo", "doc", "-p", "app", "--no-default-features"]).unwrap();
    let task = lazycargo::cli::command_task(cli.command);
    assert_eq!(
        task.to_command().display(),
        "cargo doc -p app --no-default-features"
    );
}

#[test]
fn build_command_runs_cargo_build_not_check() {
    let cli = Cli::try_parse_from(["lazycargo", "build", "--release"]).unwrap();
    let task = lazycargo::cli::command_task(cli.command);
    assert_eq!(task.to_command().display(), "cargo build --release");
}

#[test]
fn clippy_command_runs_cargo_clippy() {
    let cli = Cli::try_parse_from(["lazycargo", "clippy"]).unwrap();
    let task = lazycargo::cli::command_task(cli.command);
    assert_eq!(task.to_command().display(), "cargo clippy --all-targets");
}

#[test]
fn docs_index_resolution_workspace_scope_uses_root() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("target").join("doc");
    std::fs::create_dir_all(doc.join("app")).unwrap();
    std::fs::write(doc.join("index.html"), "root").unwrap();
    std::fs::write(doc.join("app").join("index.html"), "app").unwrap();
    assert_eq!(
        lazycargo::cli::docs_index_after_task(dir.path(), None),
        Some(doc.join("index.html"))
    );
}

#[test]
fn docs_index_resolution_package_scope_uses_package_index() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("target").join("doc");
    std::fs::create_dir_all(doc.join("app")).unwrap();
    std::fs::write(doc.join("index.html"), "root").unwrap();
    std::fs::write(doc.join("app").join("index.html"), "app").unwrap();
    assert_eq!(
        lazycargo::cli::docs_index_after_task(dir.path(), Some("app")),
        Some(doc.join("app").join("index.html"))
    );
}

#[test]
fn docs_index_resolution_missing_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        lazycargo::cli::docs_index_after_task(dir.path(), Some("app")),
        None
    );
}

#[test]
fn rejects_all_features_combined_with_specific_features() {
    let error =
        Cli::try_parse_from(["lazycargo", "check", "--all-features", "-F", "sqlite"]).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn rejects_dev_combined_with_build_dependency_kind() {
    let error = Cli::try_parse_from(["lazycargo", "add", "tokio", "--dev", "--build"]).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

fn member(name: &str, manifest_path: &str) -> PackageInfo {
    PackageInfo {
        name: name.to_owned(),
        manifest_path: manifest_path.to_owned(),
        ..PackageInfo::default()
    }
}

fn project(workspace_root: &str, members: &[(&str, &str)]) -> ProjectInfo {
    ProjectInfo {
        workspace_root: workspace_root.to_owned(),
        workspace_packages: members
            .iter()
            .map(|(name, path)| member(name, path))
            .collect(),
        ..ProjectInfo::default()
    }
}

#[test]
fn auto_scope_picks_workspace_member_from_inside_its_dir() {
    let project = project(
        "/ws",
        &[("a", "/ws/a/Cargo.toml"), ("b", "/ws/b/Cargo.toml")],
    );

    assert_eq!(
        auto_scope(Path::new("/ws/a/src"), &project),
        TaskScope::Package("a".to_owned())
    );
}

#[test]
fn auto_scope_returns_workspace_from_root_outside_any_member_dir() {
    let project = project(
        "/ws",
        &[("a", "/ws/a/Cargo.toml"), ("b", "/ws/b/Cargo.toml")],
    );

    assert_eq!(auto_scope(Path::new("/ws"), &project), TaskScope::Workspace);
}

#[test]
fn auto_scope_single_package_project_stays_current_package() {
    let project = project("/p", &[("p", "/p/Cargo.toml")]);

    assert_eq!(
        auto_scope(Path::new("/p/src"), &project),
        TaskScope::CurrentPackage
    );
}

#[test]
fn auto_scope_picks_deepest_member_for_nested_workspace() {
    let project = project(
        "/ws",
        &[
            ("outer", "/ws/Cargo.toml"),
            ("inner", "/ws/inner/Cargo.toml"),
        ],
    );

    assert_eq!(
        auto_scope(Path::new("/ws/inner/src"), &project),
        TaskScope::Package("inner".to_owned())
    );
}

#[test]
fn command_surface_exposes_cargo_subcommands() {
    let command = Cli::command();
    let names: Vec<&str> = command
        .get_subcommands()
        .map(|sub| sub.get_name())
        .collect();
    for expected in [
        "check", "build", "test", "clippy", "doc", "run", "update", "add", "search", "config",
    ] {
        assert!(names.contains(&expected), "missing subcommand: {expected}");
    }
}
