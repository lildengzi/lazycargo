use std::collections::BTreeSet;

use crate::metadata::{DependencyInfo, PackageInfo, ProjectInfo};

use super::{App, BuildCoreTab, DependenciesTab, FocusPanel, HistoryEntry, WorkspaceTab};

#[derive(Clone)]
pub(super) struct CommandItem {
    pub(super) key: &'static str,
    pub(super) label: String,
    detail: String,
}

pub(super) fn output_lines(app: &App) -> Vec<String> {
    match app.current_focus {
        FocusPanel::Workspace => match app.ws_tab {
            WorkspaceTab::CrateInfo => workspace_detail_lines(app),
            WorkspaceTab::Metrics => workspace_metrics_lines(app),
        },
        FocusPanel::BuildCore => match app.build_tab {
            BuildCoreTab::TaskConfig => build_detail_lines(app),
            BuildCoreTab::LiveOutput => app.output.clone(),
        },
        FocusPanel::Dependencies => match app.deps_tab {
            DependenciesTab::Features => dependency_detail_lines(app),
            DependenciesTab::DependencyTree => app.tree_detail.clone(),
        },
    }
}

pub(super) fn workspace_items(project: &ProjectInfo) -> Vec<String> {
    let mut items = vec!["workspace".to_owned()];
    items.extend(
        project
            .packages
            .iter()
            .map(|package| format!("pkg {package}")),
    );
    items
}

pub(super) fn dependency_items_for(
    project: &ProjectInfo,
    workspace_selected: usize,
) -> Vec<String> {
    let dependencies = selected_package(project, workspace_selected)
        .map(|package| package.dependencies.as_slice())
        .unwrap_or(project.dependencies.as_slice());

    if dependencies.is_empty() {
        return vec!["no direct dependencies".to_owned()];
    }

    dependencies
        .iter()
        .map(|dependency| {
            let features = if dependency.features.is_empty() {
                String::new()
            } else {
                format!(" +{}", dependency.features.join(","))
            };
            let optional = if dependency.optional { " optional" } else { "" };
            format!(
                "{} {} [{}]{}{}",
                dependency.name,
                dependency.req,
                dependency.kind.label(),
                features,
                optional
            )
        })
        .collect()
}

pub(super) fn build_items() -> Vec<CommandItem> {
    vec![
        CommandItem {
            key: "check",
            label: "check".to_owned(),
            detail: "cargo check".to_owned(),
        },
        CommandItem {
            key: "build",
            label: "build".to_owned(),
            detail: "cargo build".to_owned(),
        },
        CommandItem {
            key: "test",
            label: "test".to_owned(),
            detail: "cargo test".to_owned(),
        },
        CommandItem {
            key: "run",
            label: "run".to_owned(),
            detail: "cargo run".to_owned(),
        },
        CommandItem {
            key: "release",
            label: "build --release".to_owned(),
            detail: "cargo build --release".to_owned(),
        },
        CommandItem {
            key: "clippy",
            label: "clippy".to_owned(),
            detail: "cargo clippy --all-targets".to_owned(),
        },
        CommandItem {
            key: "doc",
            label: "doc --no-deps".to_owned(),
            detail: "cargo doc --no-deps".to_owned(),
        },
        CommandItem {
            key: "update",
            label: "update lockfile".to_owned(),
            detail: "cargo update".to_owned(),
        },
        CommandItem {
            key: "clean",
            label: "clean target".to_owned(),
            detail: "cargo clean".to_owned(),
        },
        CommandItem {
            key: "new",
            label: "new project".to_owned(),
            detail: "cargo new <name>".to_owned(),
        },
        CommandItem {
            key: "timings",
            label: "build --timings".to_owned(),
            detail: "cargo build --timings".to_owned(),
        },
        CommandItem {
            key: "diagnostics",
            label: "show diagnostics".to_owned(),
            detail: "show warnings/errors from last command".to_owned(),
        },
    ]
}

pub(super) fn apply_filter(lines: Vec<String>, filter: &str, active: bool) -> Vec<String> {
    if !active || filter.is_empty() {
        return lines;
    }
    lines
        .into_iter()
        .filter(|line| line.to_lowercase().contains(&filter.to_lowercase()))
        .collect()
}

pub(super) fn list_offset(selected: usize, visible_rows: usize, len: usize) -> usize {
    if len <= visible_rows {
        return 0;
    }

    selected.saturating_sub(visible_rows.saturating_sub(1))
}

fn workspace_detail_lines(app: &App) -> Vec<String> {
    let package = selected_package(&app.project, app.workspace_selected);
    let package_name = package
        .map(|package| package.name.as_str())
        .unwrap_or(app.project.name.as_str());
    let package_version = package
        .map(|package| package.version.as_str())
        .unwrap_or(app.project.version.as_str());
    let manifest_path = package
        .map(|package| package.manifest_path.as_str())
        .unwrap_or(app.project.manifest_path.as_str());

    let mut lines = vec![
        "Workspace scope".to_owned(),
        String::new(),
        selected_scope_label(app),
        String::new(),
        "Health snapshot".to_owned(),
        format!("  rustc: {}", app.project.rustc_version),
        format!(
            "  msrv: {}",
            package
                .and_then(|package| package.rust_version.as_deref())
                .unwrap_or("<not declared>")
        ),
        format!("  target total: {}", app.disk.target_size),
        String::new(),
        "Package identity".to_owned(),
        format!("  name: {package_name}"),
        format!("  version: {package_version}"),
        format!("  manifest: {manifest_path}"),
        String::new(),
        "Disk tracking".to_owned(),
        format!(
            "  source size: {}",
            package_source_size_label(app, package_name)
        ),
        format!("  target total: {}", app.disk.target_size),
        format!(
            "  crate cache estimate: {}",
            package_target_cache_label(app, package_name)
        ),
        String::new(),
        "Targets".to_owned(),
    ];
    lines.extend(package_targets(package, &app.project));
    lines.extend([
        String::new(),
        "Effect".to_owned(),
        "  test/check/build/clippy use this scope".to_owned(),
        "  workspace -> --workspace".to_owned(),
        "  package -> -p <package>".to_owned(),
        String::new(),
        "Members".to_owned(),
    ]);
    lines.extend(
        app.project
            .packages
            .iter()
            .map(|package| format!("  {package}")),
    );
    lines
}

fn build_detail_lines(app: &App) -> Vec<String> {
    vec![
        "Build".to_owned(),
        String::new(),
        selected_item_detail(&build_items(), app.build_selected),
        format!("scope: {}", selected_scope_label(app)),
        String::new(),
        "Last build output".to_owned(),
    ]
    .into_iter()
    .chain(app.build_detail.clone())
    .collect()
}

fn dependency_detail_lines(app: &App) -> Vec<String> {
    let dependency = selected_dependency(app);
    let mut lines = vec![
        "Dependencies".to_owned(),
        String::new(),
        selected_dependency_detail(app, dependency),
        format!("scope: {}", selected_scope_label(app)),
        String::new(),
        "Feature state".to_owned(),
    ];
    lines.extend(feature_state_lines(app, dependency));
    lines.extend([String::new(), "Local path".to_owned()]);
    lines.extend(dependency_path_lines(app, dependency));
    lines.extend([
        String::new(),
        "Actions".to_owned(),
        "  enter: inspect selected dependency".to_owned(),
        "  t: cargo tree".to_owned(),
        "  i: cargo tree -i <dependency>".to_owned(),
        "  m: terminal copy mode".to_owned(),
    ]);
    lines.extend(app.dependency_detail.clone());
    lines
}

fn workspace_metrics_lines(app: &App) -> Vec<String> {
    let mut lines = vec![
        "Workspace metrics".to_owned(),
        String::new(),
        format!("last status: {}", app.last_status),
        format!("scope: {}", selected_scope_label(app)),
        format!("targets: {}", app.project.targets.len()),
        format!("dependencies: {}", app.project.dependencies.len()),
        format!("diagnostics: {}", app.diagnostics.len()),
        format!("history entries: {}", app.history.len()),
        format!("target dir size: {}", app.disk.target_size),
        String::new(),
        "Package disk snapshot".to_owned(),
    ];
    lines.extend(app.disk.packages.iter().map(|package| {
        format!(
            "  {} source={} cache={}",
            package.name, package.source_size, package.target_cache
        )
    }));
    lines.extend([String::new(), "Recent command timings".to_owned()]);
    lines.extend(
        history_lines(&app.history)
            .into_iter()
            .take(10)
            .map(|line| format!("  {line}")),
    );
    lines
}

fn history_lines(history: &[HistoryEntry]) -> Vec<String> {
    if history.is_empty() {
        return vec!["no commands yet".to_owned()];
    }

    history
        .iter()
        .map(|entry| {
            let mark = if entry.success { "ok" } else { "x" };
            format!(
                "{mark} {:.2}s {}",
                entry.duration.as_secs_f32(),
                entry.command
            )
        })
        .collect()
}

fn selected_scope_label(app: &App) -> String {
    if app.workspace_selected == 0 {
        "workspace".to_owned()
    } else {
        app.project
            .packages
            .get(app.workspace_selected.saturating_sub(1))
            .map(|package| format!("package {package}"))
            .unwrap_or_else(|| "workspace".to_owned())
    }
}

fn selected_dependency(app: &App) -> Option<&DependencyInfo> {
    selected_package(&app.project, app.workspace_selected)
        .map(|package| package.dependencies.as_slice())
        .unwrap_or(app.project.dependencies.as_slice())
        .get(app.dependency_selected)
}

fn selected_dependency_detail(_app: &App, dependency: Option<&DependencyInfo>) -> String {
    dependency
        .map(|dependency| {
            let features = if dependency.features.is_empty() {
                "<none>".to_owned()
            } else {
                dependency.features.join(", ")
            };
            let default = if dependency.uses_default_features {
                "on"
            } else {
                "off"
            };
            format!(
                "{} {} kind={} default={} features={} optional={}",
                dependency.name,
                dependency.req,
                dependency.kind.label(),
                default,
                features,
                dependency.optional
            )
        })
        .unwrap_or_else(|| "no dependency selected".to_owned())
}

fn selected_item_detail(items: &[CommandItem], selected: usize) -> String {
    items
        .get(selected)
        .map(|item| item.detail.to_owned())
        .unwrap_or_else(|| "no selection".to_owned())
}

fn selected_package(project: &ProjectInfo, workspace_selected: usize) -> Option<&PackageInfo> {
    if workspace_selected == 0 {
        project.workspace_packages.first()
    } else {
        project
            .workspace_packages
            .get(workspace_selected.saturating_sub(1))
    }
}

fn package_targets(package: Option<&PackageInfo>, project: &ProjectInfo) -> Vec<String> {
    let targets = package
        .map(|package| package.targets.as_slice())
        .unwrap_or(project.targets.as_slice());
    if targets.is_empty() {
        return vec!["  <none>".to_owned()];
    }

    targets
        .iter()
        .map(|target| format!("  {} [{}]", target.name, target.kind.join(",")))
        .collect()
}

fn package_source_size_label(app: &App, package_name: &str) -> String {
    app.disk
        .package(package_name)
        .map(|package| package.source_size.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn package_target_cache_label(app: &App, package_name: &str) -> String {
    app.disk
        .package(package_name)
        .map(|package| package.target_cache.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn feature_state_lines(app: &App, dependency: Option<&DependencyInfo>) -> Vec<String> {
    let Some(dependency) = dependency else {
        return vec!["  <select a dependency>".to_owned()];
    };
    let explicit = dependency.features.iter().cloned().collect::<BTreeSet<_>>();
    let package_features = app
        .project
        .dependency_packages
        .iter()
        .find(|package| package.name == dependency.name);
    let resolved = package_features
        .map(|package| {
            package
                .enabled_features
                .iter()
                .cloned()
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let mut all = package_features
        .map(|package| package.features.iter().cloned().collect::<BTreeSet<_>>())
        .unwrap_or_default();
    all.extend(explicit.iter().cloned());
    all.extend(resolved.iter().cloned());
    if dependency.uses_default_features {
        all.insert("default".to_owned());
    }

    if all.is_empty() {
        return vec![
            "  <feature list unavailable>".to_owned(),
            "  run `cargo info <crate>` from Search for remote feature detail".to_owned(),
        ];
    }

    let mut lines = Vec::new();
    for feature in all {
        let mark = if explicit.contains(&feature)
            || (feature == "default" && dependency.uses_default_features)
        {
            "[x]"
        } else if resolved.contains(&feature) {
            "[-]"
        } else {
            "[ ]"
        };
        lines.push(format!("  {mark} {feature}"));
    }
    lines
}

fn dependency_path_lines(app: &App, dependency: Option<&DependencyInfo>) -> Vec<String> {
    let Some(dependency) = dependency else {
        return vec!["  <select a dependency>".to_owned()];
    };
    let package_name = selected_package(&app.project, app.workspace_selected)
        .map(|package| package.name.as_str())
        .unwrap_or(app.project.name.as_str());
    vec![
        format!("  {package_name} -> {}", dependency.name),
        "  press i for cargo tree -i <dependency>".to_owned(),
    ]
}
