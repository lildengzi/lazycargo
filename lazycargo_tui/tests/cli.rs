//! CLI tests: clap-parsed command surface drives real CargoTask construction.
//!
//! Parsing is side-effect free: `Cli::try_parse_from` / `CommandFactory::command()`
//! never spawn `cargo`, so these tests assert command construction only.

use clap::error::ErrorKind;
use clap::{CommandFactory, Parser};

use lazycargo::cli::{Cli, Command};
use lazycargo::CargoTask;

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
    let cli =
        Cli::try_parse_from(["lazycargo", "check", "-p", "app", "-F", "sqlite,serde", "--release"])
            .unwrap();
    let task = task_from(cli.command);

    assert_eq!(task.to_command().display(), "cargo check -p app --release --features serde,sqlite");
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
    let cli =
        Cli::try_parse_from(["lazycargo", "test", "-p", "core", "user_service", "--nocapture"])
            .unwrap();
    let task = task_from(cli.command);

    assert_eq!(
        task.to_command().display(),
        "cargo test -p core user_service -- --nocapture"
    );
}

#[test]
fn run_command_builds_binary_with_passthrough_args() {
    let cli =
        Cli::try_parse_from(["lazycargo", "run", "--bin", "server", "--", "--port", "3000"])
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
    assert_eq!(plan.to_command().display(), "cargo add cc --build --optional");
}

#[test]
fn add_normal_dependency_builds_plan_with_package_and_features() {
    let cli =
        Cli::try_parse_from(["lazycargo", "add", "tokio", "-p", "app", "-F", "macros,rt-multi-thread"])
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
fn rejects_all_features_combined_with_specific_features() {
    let error =
        Cli::try_parse_from(["lazycargo", "check", "--all-features", "-F", "sqlite"]).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn rejects_dev_combined_with_build_dependency_kind() {
    let error =
        Cli::try_parse_from(["lazycargo", "add", "tokio", "--dev", "--build"]).unwrap_err();

    assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
}

#[test]
fn command_surface_exposes_cargo_subcommands() {
    let command = Cli::command();
    let names: Vec<&str> = command.get_subcommands().map(|sub| sub.get_name()).collect();
    for expected in [
        "check",
        "build",
        "test",
        "clippy",
        "doc",
        "run",
        "update",
        "add",
        "search",
        "config",
    ] {
        assert!(names.contains(&expected), "missing subcommand: {expected}");
    }
}
