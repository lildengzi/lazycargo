use lazycargo::{
    parse_args, CargoTaskKind, CliAction, CrateSearchQuery, DependencyAddPlan, DependencyKind,
    Profile, TaskScope,
};

fn parse(input: &[&str]) -> lazycargo::CargoTask {
    match parse_args(input.iter().copied()).unwrap() {
        CliAction::Run(task) => task,
        other => panic!("expected task, got {other:?}"),
    }
}

#[test]
fn builds_package_check_command_with_features() {
    let task = parse(&["check", "-p", "app", "-F", "sqlite,serde", "--release"]);

    assert_eq!(task.kind, CargoTaskKind::Check);
    assert_eq!(task.scope, TaskScope::Package("app".to_owned()));
    assert_eq!(task.profile, Profile::Release);
    assert_eq!(task.features.features, ["serde", "sqlite"]);
    assert_eq!(
        task.to_command().display(),
        "cargo check -p app --release --features serde,sqlite"
    );
}

#[test]
fn builds_test_command_with_filter_and_nocapture() {
    let task = parse(&["test", "-p", "core", "user_service", "--nocapture"]);

    assert_eq!(
        task.kind,
        CargoTaskKind::Test {
            filter: Some("user_service".to_owned()),
            nocapture: true,
        }
    );
    assert_eq!(
        task.to_command().display(),
        "cargo test -p core user_service -- --nocapture"
    );
}

#[test]
fn builds_run_command_with_binary_and_args() {
    let task = parse(&["run", "--bin", "server", "--", "--port", "3000"]);

    assert_eq!(
        task.kind,
        CargoTaskKind::Run {
            bin: Some("server".to_owned()),
            args: vec!["--port".to_owned(), "3000".to_owned()],
        }
    );
    assert_eq!(
        task.to_command().display(),
        "cargo run --bin server -- --port 3000"
    );
}

#[test]
fn rejects_all_features_with_specific_features() {
    let error = parse_args(["check", "--all-features", "-F", "sqlite"]).unwrap_err();

    assert_eq!(
        error.message(),
        "--all-features cannot be combined with --features"
    );
}

#[test]
fn builds_crate_search_query() {
    let action = parse_args(["search", "tokio", "--limit", "5"]).unwrap();

    assert_eq!(
        action,
        CliAction::SearchCrates(CrateSearchQuery {
            query: "tokio".to_owned(),
            limit: 5,
        })
    );
}

#[test]
fn builds_add_dependency_command_with_package_and_features() {
    let action = parse_args(["add", "tokio", "-p", "app", "-F", "macros,rt-multi-thread"]).unwrap();

    let CliAction::AddDependency(plan) = action else {
        panic!("expected add dependency");
    };

    assert_eq!(
        plan,
        DependencyAddPlan {
            name: "tokio".to_owned(),
            package: Some("app".to_owned()),
            features: vec!["macros".to_owned(), "rt-multi-thread".to_owned()],
            kind: DependencyKind::Normal,
            optional: false,
        }
    );
    assert_eq!(
        plan.to_command().display(),
        "cargo add tokio -p app --features macros,rt-multi-thread"
    );
}

#[test]
fn builds_dev_dependency_command_with_options_before_name() {
    let action = parse_args(["add", "--dev", "tempfile"]).unwrap();

    let CliAction::AddDependency(plan) = action else {
        panic!("expected add dependency");
    };

    assert_eq!(plan.kind, DependencyKind::Dev);
    assert_eq!(plan.to_command().display(), "cargo add tempfile --dev");
}

#[test]
fn builds_build_dependency_command() {
    let action = parse_args(["add", "cc", "--build", "--optional"]).unwrap();

    let CliAction::AddDependency(plan) = action else {
        panic!("expected add dependency");
    };

    assert_eq!(plan.kind, DependencyKind::Build);
    assert!(plan.optional);
    assert_eq!(
        plan.to_command().display(),
        "cargo add cc --build --optional"
    );
}

#[test]
fn rejects_conflicting_dependency_kinds() {
    let error = parse_args(["add", "tokio", "--dev", "--build"]).unwrap_err();

    assert_eq!(error.message(), "--dev and --build cannot be combined");
}
