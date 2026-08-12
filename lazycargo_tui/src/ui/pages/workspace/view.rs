use std::collections::BTreeSet;

use crate::core::build_history::format_duration_ms;
use crate::core::dep_tree;
use crate::core::model::{CoreState, OutputSlot};
use crate::core::project::{DependencyInfo, PackageInfo, ProjectInfo};
use crate::core::util::{format_bytes, progress_bar};
use crate::ui::controller::{
    BuildCoreTab, DependenciesTab, FocusPanel, WorkspaceTab, WorkspaceView,
};
use crate::ui::HistoryEntry;

#[derive(Clone)]
pub(crate) struct CommandItem {
    pub(crate) key: &'static str,
    pub(crate) label: String,
    detail: String,
}

pub(crate) fn output_lines(
    core: &CoreState,
    view: &WorkspaceView,
    current_focus: FocusPanel,
    last_status: &str,
    history: &[HistoryEntry],
) -> Vec<String> {
    match current_focus {
        FocusPanel::Workspace => match view.ws_tab {
            WorkspaceTab::CrateInfo => workspace_detail_lines(core, view),
            WorkspaceTab::Metrics => workspace_metrics_lines(core, view, history, last_status),
            WorkspaceTab::Target => target_analysis_lines(core, view),
        },
        FocusPanel::BuildCore => match view.build_tab {
            BuildCoreTab::TaskConfig => build_detail_lines(core, view),
            BuildCoreTab::LiveOutput => core.slot_lines(OutputSlot::BuildLive),
        },
        FocusPanel::Dependencies => match view.deps_tab {
            DependenciesTab::Features => dependency_detail_lines(core, view),
            DependenciesTab::DependencyTree
                if core
                    .output
                    .get(&OutputSlot::DepsTree)
                    .map(|ctx| ctx.tree_nodes.is_empty())
                    .unwrap_or(true) =>
            {
                core.slot_lines(OutputSlot::DepsTree)
            }
            DependenciesTab::DependencyTree => dependency_tree_lines(core, view),
        },
    }
}

pub(crate) fn workspace_items(project: &ProjectInfo) -> Vec<String> {
    let mut items = vec!["workspace".to_owned()];
    items.extend(
        project
            .packages
            .iter()
            .map(|package| format!("pkg {package}")),
    );
    items
}

pub(crate) fn dependency_items_for(
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

pub(crate) fn selected_dependency_name(
    project: &ProjectInfo,
    workspace_selected: usize,
    selected: usize,
) -> Option<String> {
    let dependencies = if workspace_selected == 0 {
        project
            .workspace_packages
            .first()
            .map(|package| package.dependencies.as_slice())
            .unwrap_or(project.dependencies.as_slice())
    } else {
        project
            .workspace_packages
            .get(workspace_selected.saturating_sub(1))
            .map(|package| package.dependencies.as_slice())
            .unwrap_or(project.dependencies.as_slice())
    };

    dependencies
        .get(selected)
        .map(|dependency| dependency.name.clone())
}

pub(crate) fn build_items() -> Vec<CommandItem> {
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

fn workspace_detail_lines(core: &CoreState, view: &WorkspaceView) -> Vec<String> {
    let package = selected_package(&core.project, view.selected.workspace);
    let package_name = package
        .map(|package| package.name.as_str())
        .unwrap_or(core.project.name.as_str());
    let package_version = package
        .map(|package| package.version.as_str())
        .unwrap_or(core.project.version.as_str());
    let manifest_path = package
        .map(|package| package.manifest_path.as_str())
        .unwrap_or(core.project.manifest_path.as_str());

    let mut lines = vec![
        format!(
            "scope: {}",
            scope_label(&core.project, view.selected.workspace)
        ),
        String::new(),
        "Health snapshot".to_owned(),
        format!("  rustc: {}", core.project.rustc_version),
        format!(
            "  msrv: {}",
            package
                .and_then(|package| package.rust_version.as_deref())
                .unwrap_or("<not declared>")
        ),
        format!("  target total: {}", core.disk.total_label),
        String::new(),
        "Package identity".to_owned(),
        format!("  name: {package_name}"),
        format!("  version: {package_version}"),
        format!("  manifest: {manifest_path}"),
        String::new(),
        "Disk tracking".to_owned(),
        format!(
            "  source size: {}",
            package_source_size_label(core, package_name)
        ),
        format!("  target total: {}", core.disk.total_label),
        format!(
            "  crate cache estimate: {}",
            package_target_cache_label(core, package_name)
        ),
        String::new(),
        "Targets".to_owned(),
    ];
    lines.extend(package_targets(package, &core.project));
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
        core.project
            .packages
            .iter()
            .map(|package| format!("  {package}")),
    );
    lines
}

fn build_detail_lines(core: &CoreState, view: &WorkspaceView) -> Vec<String> {
    vec![
        selected_item_detail(&build_items(), view.selected.build),
        format!(
            "scope: {}",
            scope_label(&core.project, view.selected.workspace)
        ),
        String::new(),
        "Last build output".to_owned(),
    ]
    .into_iter()
    .chain(core.slot_lines(OutputSlot::BuildConfig))
    .collect()
}

fn dependency_detail_lines(core: &CoreState, view: &WorkspaceView) -> Vec<String> {
    let dependency = selected_dependency(core, view);
    let mut lines = vec![
        selected_dependency_detail(dependency),
        format!(
            "scope: {}",
            scope_label(&core.project, view.selected.workspace)
        ),
        String::new(),
        "Feature state".to_owned(),
    ];
    lines.extend(feature_state_lines(core, dependency));
    lines.extend([String::new(), "Local path".to_owned()]);
    lines.extend(dependency_path_lines(core, view, dependency));
    lines.extend([
        String::new(),
        "Actions".to_owned(),
        "  enter: inspect".to_owned(),
        "  d: read docs in TUI".to_owned(),
        "  D: open docs.rs in browser".to_owned(),
        "  t: cargo tree --offline -e features".to_owned(),
        "  i: cargo tree --offline -e features -i <dependency>".to_owned(),
        "  T/I: allow Cargo to fetch missing registry packages".to_owned(),
        "  a: preview cargo add".to_owned(),
    ]);
    lines.extend(core.slot_lines(OutputSlot::DepsFeatures));
    lines
}

fn workspace_metrics_lines(
    core: &CoreState,
    view: &WorkspaceView,
    history: &[HistoryEntry],
    last_status: &str,
) -> Vec<String> {
    let mut lines = vec![
        format!("last status: {last_status}"),
        format!(
            "scope: {}",
            scope_label(&core.project, view.selected.workspace)
        ),
        format!("targets: {}", core.project.targets.len()),
        format!("dependencies: {}", core.project.dependencies.len()),
        format!("diagnostics: {}", core.diagnostics.len()),
        format!("history entries: {}", history.len()),
        format!("target dir size: {}", core.disk.total_label),
        String::new(),
        "Build Metrics".to_owned(),
        "Last 10 builds".to_owned(),
    ];
    lines.extend(core.history.entries.iter().take(10).map(|entry| {
        let mark = if entry.success { "ok" } else { "x" };
        format!(
            "  {} {:>7} {} {}",
            mark,
            format_duration_ms(entry.duration_ms),
            entry.timestamp.format("%m-%d %H:%M"),
            entry.command
        )
    }));
    if core.history.entries.is_empty() {
        lines.push("  no build history yet".to_owned());
    }
    lines.extend([String::new(), "Recent averages".to_owned()]);
    lines.extend(core.history.daily_stats(1).into_iter().map(|stats| {
        format!(
            "  {} avg: {} ({} runs)",
            stats.command,
            format_duration_ms(stats.average_ms),
            stats.count
        )
    }));
    lines.extend([String::new(), "Slowest crates today".to_owned()]);
    let slowest = core.history.slowest_crates(10);
    if slowest.is_empty() {
        lines.push("  <run cargo build --timings to populate>".to_owned());
    } else {
        lines.extend(slowest.into_iter().map(|timing| {
            format!(
                "  {:<22} {}",
                timing.name,
                format_duration_ms(timing.duration_ms)
            )
        }));
    }
    lines.extend([String::new(), "Package disk snapshot".to_owned()]);
    lines.extend(core.project.workspace_packages.iter().map(|package| {
        format!(
            "  {} source={} cache={}",
            package.name,
            package_source_size_label(core, &package.name),
            package_target_cache_label(core, &package.name)
        )
    }));
    lines.extend([String::new(), "Recent command timings".to_owned()]);
    lines.extend(
        history_lines(history)
            .into_iter()
            .take(10)
            .map(|line| format!("  {line}")),
    );
    lines
}

fn target_analysis_lines(core: &CoreState, view: &WorkspaceView) -> Vec<String> {
    let selected = view
        .selected
        .target_crate
        .min(core.disk.by_crate.len().saturating_sub(1));
    let mut lines = vec![
        format!(
            "Total: {}  |  Stale: {} ({}d+)",
            core.disk.total_label, core.disk.stale_label, core.config.target_stale_days
        ),
        format!("Disk pressure: {}", disk_pressure_bar(core.disk.total_size)),
        format!(
            "Last updated: {:.1}s ago",
            core.disk.last_updated.elapsed().as_secs_f32()
        ),
        String::new(),
        "By Profile".to_owned(),
    ];
    if core.disk.by_profile.is_empty() {
        lines.push("  <target directory not built yet>".to_owned());
    } else {
        lines.extend(core.disk.by_profile.iter().map(|profile| {
            format!(
                "  {:<12} {:>10} {:>7} files  last {}",
                profile.profile,
                profile.label,
                profile.file_count,
                profile.last_modified.format("%m-%d %H:%M")
            )
        }));
    }
    lines.extend([
        String::new(),
        format!("Top {} Crates", core.config.target_top_crates),
    ]);
    if core.disk.by_crate.is_empty() {
        lines.push("  <no attributable crate artifacts>".to_owned());
    } else {
        lines.extend(
            core.disk
                .by_crate
                .iter()
                .take(core.config.target_top_crates)
                .enumerate()
                .map(|(index, item)| {
                    let local = if item.is_local { "local" } else { "dep" };
                    let marker = if index == selected { ">" } else { " " };
                    format!(
                        "{marker} {:<24} {:>10} {:<8} {}",
                        item.name, item.label, item.profile, local
                    )
                }),
        );
    }
    lines.extend([String::new(), "Selected crate".to_owned()]);
    if let Some(item) = core.disk.by_crate.get(selected) {
        lines.extend([
            format!("  name: {}", item.name),
            format!("  size: {}", item.label),
            format!("  profile: {}", item.profile),
            format!(
                "  ownership: {}",
                if item.is_local {
                    "workspace crate"
                } else {
                    "dependency"
                }
            ),
            "  enter: inspect selected crate".to_owned(),
            "  future: precise single-crate artifact clean".to_owned(),
        ]);
    } else {
        lines.push("  <no crate selected>".to_owned());
    }
    lines.extend([
        String::new(),
        "Actions".to_owned(),
        "  r: refresh target analysis".to_owned(),
        "  d: dry-run stale clean".to_owned(),
        format!(
            "  c: clean stale .rmeta/.rlib files older than {} days",
            core.config.target_stale_days
        ),
    ]);
    lines
}

fn disk_pressure_bar(size: u64) -> String {
    const SOFT_LIMIT: u64 = 20 * 1024 * 1024 * 1024;
    const WIDTH: usize = 20;
    let limit = SOFT_LIMIT.max(size);
    format!(
        "{} {} / {}",
        progress_bar(size as f64 / limit as f64, WIDTH),
        format_bytes(size),
        format_bytes(SOFT_LIMIT)
    )
}

fn dependency_tree_lines(core: &CoreState, view: &WorkspaceView) -> Vec<String> {
    let tree_nodes = core
        .output
        .get(&OutputSlot::DepsTree)
        .map(|ctx| ctx.tree_nodes.as_slice())
        .unwrap_or(&[]);
    let visible = dep_tree::flatten_visible(tree_nodes);
    let selected = visible.get(view.selected.tree);
    let mut lines = Vec::new();
    lines.extend(dep_tree::render_tree_lines(tree_nodes, view.selected.tree));
    lines.extend([String::new(), "Selected detail".to_owned()]);
    lines.extend(dep_tree::node_detail(selected));
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

fn selected_dependency<'a>(
    core: &'a CoreState,
    view: &WorkspaceView,
) -> Option<&'a DependencyInfo> {
    selected_package(&core.project, view.selected.workspace)
        .map(|package| package.dependencies.as_slice())
        .unwrap_or(core.project.dependencies.as_slice())
        .get(view.selected.dependency)
}

fn selected_dependency_detail(dependency: Option<&DependencyInfo>) -> String {
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

fn package_source_size_label(core: &CoreState, package_name: &str) -> String {
    let manifest = core
        .project
        .workspace_packages
        .iter()
        .find(|package| package.name == package_name)
        .map(|package| package.manifest_path.as_str())
        .unwrap_or(core.project.manifest_path.as_str());
    let Some(root) = std::path::Path::new(manifest).parent() else {
        return "<unknown>".to_owned();
    };
    crate::ui::dir_size(&root.join("src"))
        .map(format_bytes)
        .unwrap_or_else(|_| "<unknown>".to_owned())
}

fn package_target_cache_label(core: &CoreState, package_name: &str) -> String {
    core.disk
        .package(package_name)
        .map(|package| package.target_cache.clone())
        .unwrap_or_else(|| "<unknown>".to_owned())
}

fn feature_state_lines(core: &CoreState, dependency: Option<&DependencyInfo>) -> Vec<String> {
    let Some(dependency) = dependency else {
        return vec!["  <select a dependency>".to_owned()];
    };
    let explicit = dependency.features.iter().cloned().collect::<BTreeSet<_>>();
    let package_features = core
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

fn dependency_path_lines(
    core: &CoreState,
    view: &WorkspaceView,
    dependency: Option<&DependencyInfo>,
) -> Vec<String> {
    let Some(dependency) = dependency else {
        return vec!["  <select a dependency>".to_owned()];
    };
    let package_name = selected_package(&core.project, view.selected.workspace)
        .map(|package| package.name.as_str())
        .unwrap_or(core.project.name.as_str());
    vec![
        format!("  {package_name} -> {}", dependency.name),
        "  press i for cargo tree -i <dependency>".to_owned(),
    ]
}

pub(crate) fn scope_label(project: &ProjectInfo, workspace_selected: usize) -> String {
    if workspace_selected == 0 {
        "workspace".to_owned()
    } else {
        project
            .packages
            .get(workspace_selected.saturating_sub(1))
            .map(|package| format!("package {package}"))
            .unwrap_or_else(|| "workspace".to_owned())
    }
}
