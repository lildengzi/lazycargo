use crate::core::command::CommandSpec;
use std::ffi::OsString;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrateSearchQuery {
    pub query: String,
    pub limit: usize,
}

impl CrateSearchQuery {
    pub fn new(query: impl Into<String>) -> Self {
        Self {
            query: query.into(),
            limit: 20,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DependencyAddPlan {
    pub name: String,
    pub package: Option<String>,
    pub features: Vec<String>,
    pub kind: DependencyKind,
    pub optional: bool,
}

impl DependencyAddPlan {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            package: None,
            features: Vec::new(),
            kind: DependencyKind::Normal,
            optional: false,
        }
    }

    pub fn to_command(&self) -> CommandSpec {
        let mut args = vec!["add".into(), OsString::from(&self.name)];

        if let Some(package) = &self.package {
            args.push("-p".into());
            args.push(package.into());
        }

        if !self.features.is_empty() {
            args.push("--features".into());
            args.push(self.features.join(",").into());
        }

        match self.kind {
            DependencyKind::Normal => {}
            DependencyKind::Dev => args.push("--dev".into()),
            DependencyKind::Build => args.push("--build".into()),
        }

        if self.optional {
            args.push("--optional".into());
        }

        CommandSpec {
            program: "cargo".into(),
            args,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DependencyKind {
    Normal,
    Dev,
    Build,
}
