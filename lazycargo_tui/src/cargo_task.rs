use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CargoTask {
    pub kind: CargoTaskKind,
    pub scope: TaskScope,
    pub features: FeatureSelection,
    pub profile: Profile,
    pub target: Option<String>,
    pub extra_args: Vec<String>,
}

impl CargoTask {
    pub fn new(kind: CargoTaskKind) -> Self {
        Self {
            kind,
            scope: TaskScope::CurrentPackage,
            features: FeatureSelection::default(),
            profile: Profile::Dev,
            target: None,
            extra_args: Vec::new(),
        }
    }

    pub fn to_command(&self) -> CommandSpec {
        let mut args = vec![self.kind.cargo_subcommand().into()];

        match &self.scope {
            TaskScope::CurrentPackage => {}
            TaskScope::Workspace => args.push("--workspace".into()),
            TaskScope::Package(package) => {
                args.push("-p".into());
                args.push(package.into());
            }
        }

        if self.profile == Profile::Release {
            args.push("--release".into());
        }

        if let Some(target) = &self.target {
            args.push("--target".into());
            args.push(target.into());
        }

        self.features.push_args(&mut args);

        match &self.kind {
            CargoTaskKind::Test { filter, nocapture } => {
                if let Some(filter) = filter {
                    args.push(filter.into());
                }

                if *nocapture {
                    args.push("--".into());
                    args.push("--nocapture".into());
                }
            }
            CargoTaskKind::Run {
                bin,
                args: run_args,
            } => {
                if let Some(bin) = bin {
                    args.push("--bin".into());
                    args.push(bin.into());
                }

                if !run_args.is_empty() {
                    args.push("--".into());
                    args.extend(run_args.iter().map(OsString::from));
                }
            }
            CargoTaskKind::Clippy => {
                args.push("--all-targets".into());
            }
            _ => {}
        }

        args.extend(self.extra_args.iter().map(OsString::from));

        CommandSpec {
            program: "cargo".into(),
            args,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CargoTaskKind {
    Check,
    Build,
    Run {
        bin: Option<String>,
        args: Vec<String>,
    },
    Test {
        filter: Option<String>,
        nocapture: bool,
    },
    Clippy,
    Doc,
    Update {
        package: Option<String>,
    },
}

impl CargoTaskKind {
    fn cargo_subcommand(&self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Build => "build",
            Self::Run { .. } => "run",
            Self::Test { .. } => "test",
            Self::Clippy => "clippy",
            Self::Doc => "doc",
            Self::Update { .. } => "update",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskScope {
    CurrentPackage,
    Workspace,
    Package(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FeatureSelection {
    pub features: Vec<String>,
    pub all_features: bool,
    pub no_default_features: bool,
}

impl FeatureSelection {
    fn push_args(&self, args: &mut Vec<OsString>) {
        if self.all_features {
            args.push("--all-features".into());
        }

        if self.no_default_features {
            args.push("--no-default-features".into());
        }

        if !self.features.is_empty() {
            args.push("--features".into());
            args.push(self.features.join(",").into());
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Dev,
    Release,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: OsString,
    pub args: Vec<OsString>,
}

impl CommandSpec {
    pub fn display(&self) -> String {
        std::iter::once(self.program.to_string_lossy().into_owned())
            .chain(
                self.args
                    .iter()
                    .map(|arg| shell_quote(&arg.to_string_lossy())),
            )
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn shell_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/' | ':' | ',' | '='))
    {
        value.to_owned()
    } else {
        format!("'{}'", value.replace('\'', "'\\''"))
    }
}
