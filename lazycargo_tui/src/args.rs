use crate::core::command::{CargoTask, CargoTaskKind, FeatureSelection, Profile, TaskScope};
use crate::core::search::{CrateSearchQuery, DependencyAddPlan, DependencyKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run(CargoTask),
    SearchCrates(CrateSearchQuery),
    AddDependency(DependencyAddPlan),
    PrintConfig,
    PrintHelp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArgsError {
    message: String,
}

impl ArgsError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.message.fmt(f)
    }
}

impl std::error::Error for ArgsError {}

pub fn parse_args<I, S>(input: I) -> Result<CliAction, ArgsError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = input.into_iter().map(Into::into).peekable();
    let Some(command) = args.next() else {
        return Ok(CliAction::PrintHelp);
    };

    if matches!(command.as_str(), "-h" | "--help" | "help") {
        return Ok(CliAction::PrintHelp);
    }

    if command == "config" {
        return Ok(CliAction::PrintConfig);
    }

    if command == "search" || command == "s" {
        return parse_search_args(args);
    }

    if command == "add" || command == "a" {
        return parse_add_args(args);
    }

    let mut task = match command.as_str() {
        "check" | "c" => CargoTask::new(CargoTaskKind::Check),
        "build" | "b" => CargoTask::new(CargoTaskKind::Build),
        "run" | "r" => CargoTask::new(CargoTaskKind::Run {
            bin: None,
            args: Vec::new(),
        }),
        "test" | "t" => CargoTask::new(CargoTaskKind::Test {
            filter: None,
            nocapture: false,
        }),
        "clippy" | "l" => CargoTask::new(CargoTaskKind::Clippy),
        "doc" | "d" => CargoTask::new(CargoTaskKind::Doc),
        "update" | "u" => CargoTask::new(CargoTaskKind::Update { package: None }),
        other => return Err(ArgsError::new(format!("unknown command: {other}"))),
    };

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => {
                let package = take_value(&mut args, &arg)?;
                task.scope = TaskScope::Package(package);
            }
            "--workspace" => {
                task.scope = TaskScope::Workspace;
            }
            "--release" => {
                task.profile = Profile::Release;
            }
            "--target" => {
                task.target = Some(take_value(&mut args, &arg)?);
            }
            "-F" | "--features" => {
                let value = take_value(&mut args, &arg)?;
                task.features.features.extend(split_features(&value));
            }
            "--all-features" => {
                task.features.all_features = true;
            }
            "--no-default-features" => {
                task.features.no_default_features = true;
            }
            "--bin" => {
                let bin = take_value(&mut args, &arg)?;
                match &mut task.kind {
                    CargoTaskKind::Run { bin: current, .. } => *current = Some(bin),
                    _ => return Err(ArgsError::new("--bin is only valid for run")),
                }
            }
            "--nocapture" => match &mut task.kind {
                CargoTaskKind::Test { nocapture, .. } => *nocapture = true,
                _ => return Err(ArgsError::new("--nocapture is only valid for test")),
            },
            "--" => {
                let rest = args.collect::<Vec<_>>();
                match &mut task.kind {
                    CargoTaskKind::Run { args: run_args, .. } => run_args.extend(rest),
                    CargoTaskKind::Test { nocapture, .. }
                        if rest.iter().any(|arg| arg == "--nocapture") =>
                    {
                        *nocapture = true;
                    }
                    CargoTaskKind::Test { .. } => {
                        task.extra_args.push("--".to_owned());
                        task.extra_args.extend(rest);
                    }
                    _ => {
                        task.extra_args.push("--".to_owned());
                        task.extra_args.extend(rest);
                    }
                }
                break;
            }
            value if value.starts_with('-') => {
                task.extra_args.push(value.to_owned());
            }
            value => match &mut task.kind {
                CargoTaskKind::Test { filter, .. } => {
                    if filter.is_some() {
                        task.extra_args.push(value.to_owned());
                    } else {
                        *filter = Some(value.to_owned());
                    }
                }
                CargoTaskKind::Update { package } => {
                    if package.is_some() {
                        return Err(ArgsError::new("update accepts at most one package name"));
                    }
                    *package = Some(value.to_owned());
                    task.extra_args.push("-p".to_owned());
                    task.extra_args.push(value.to_owned());
                }
                _ => task.extra_args.push(value.to_owned()),
            },
        }
    }

    normalize_features(&mut task.features)?;
    Ok(CliAction::Run(task))
}

fn parse_search_args<I>(mut args: std::iter::Peekable<I>) -> Result<CliAction, ArgsError>
where
    I: Iterator<Item = String>,
{
    let mut query = None;
    let mut limit = 20;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--limit" => {
                let value = take_value(&mut args, &arg)?;
                limit = value
                    .parse::<usize>()
                    .map_err(|_| ArgsError::new("--limit requires a positive integer"))?;
            }
            value if value.starts_with('-') => {
                return Err(ArgsError::new(format!("unknown search option: {value}")));
            }
            value => {
                if query.is_some() {
                    return Err(ArgsError::new("search accepts one query"));
                }
                query = Some(value.to_owned());
            }
        }
    }

    let query = query.ok_or_else(|| ArgsError::new("search requires a query"))?;
    Ok(CliAction::SearchCrates(CrateSearchQuery { query, limit }))
}

fn parse_add_args<I>(mut args: std::iter::Peekable<I>) -> Result<CliAction, ArgsError>
where
    I: Iterator<Item = String>,
{
    let mut name = None;
    let mut package = None;
    let mut features = Vec::new();
    let mut kind = DependencyKind::Normal;
    let mut optional = false;

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-p" | "--package" => {
                package = Some(take_value(&mut args, &arg)?);
            }
            "-F" | "--features" => {
                let value = take_value(&mut args, &arg)?;
                features.extend(split_features(&value));
            }
            "--dev" => {
                set_dependency_kind(&mut kind, DependencyKind::Dev)?;
            }
            "--build" => {
                set_dependency_kind(&mut kind, DependencyKind::Build)?;
            }
            "--optional" => {
                optional = true;
            }
            value if value.starts_with('-') => {
                return Err(ArgsError::new(format!("unknown add option: {value}")));
            }
            value => {
                if name.is_some() {
                    return Err(ArgsError::new("add accepts one crate name"));
                }
                name = Some(value.to_owned());
            }
        }
    }

    let mut plan =
        DependencyAddPlan::new(name.ok_or_else(|| ArgsError::new("add requires a crate name"))?);
    plan.package = package;
    plan.features = features;
    plan.features.sort();
    plan.features.dedup();
    plan.kind = kind;
    plan.optional = optional;
    Ok(CliAction::AddDependency(plan))
}

fn set_dependency_kind(
    current: &mut DependencyKind,
    next: DependencyKind,
) -> Result<(), ArgsError> {
    if *current != DependencyKind::Normal && *current != next {
        return Err(ArgsError::new("--dev and --build cannot be combined"));
    }

    *current = next;
    Ok(())
}

fn take_value<I>(args: &mut std::iter::Peekable<I>, flag: &str) -> Result<String, ArgsError>
where
    I: Iterator<Item = String>,
{
    args.next()
        .ok_or_else(|| ArgsError::new(format!("{flag} requires a value")))
}

fn split_features(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(',')
        .flat_map(str::split_whitespace)
        .filter(|feature| !feature.is_empty())
        .map(ToOwned::to_owned)
}

fn normalize_features(features: &mut FeatureSelection) -> Result<(), ArgsError> {
    if features.all_features && !features.features.is_empty() {
        return Err(ArgsError::new(
            "--all-features cannot be combined with --features",
        ));
    }

    features.features.sort();
    features.features.dedup();
    Ok(())
}

pub fn help_text() -> &'static str {
    "lazycargo <command> [options]\n\nCommands:\n  check|c\n  build|b\n  run|r\n  test|t\n  clippy|l\n  doc|d\n  update|u\n  search|s <query>\n  add|a <crate>\n  config\n\nCommon task options:\n  -p, --package <name>\n      --workspace\n      --release\n      --target <triple>\n  -F, --features <a,b>\n      --all-features\n      --no-default-features\n\nCommand options:\n      --bin <name>         run only\n      --nocapture          test only\n      --limit <n>          search only\n      --dev                add only\n      --build              add only\n      --optional           add only\n\nNon-TUI commands preview the Cargo command instead of executing it.\n"
}
