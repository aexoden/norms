// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Rust-specific checks.

use std::collections::BTreeSet;
use std::path::Path;

use semver::Comparator;

use crate::config::{
    CargoConfig, CargoToml, CargoWorkspace, ClippyToml, FormatterRun, Workflow, WorkflowJob,
    WorkflowStep, commands,
};
use crate::context::{
    ComponentContext, ConfigStatus, LEGACY_RUST_TOOLCHAIN, ProjectContext, RUST_TOOLCHAIN,
};
use crate::models::Language;
use crate::report::{Issue, Outcome, Report, file_exists};

/// Whether the package whose manifest, `config`, is in `dir` builds a binary, from an explicit `[[bin]]` target or,
/// unless `autobins = false`, `src/main.rs` or `src/bin/`. A virtual manifest, without a `[package]`, builds nothing.
fn builds_binary(dir: &Path, config: &CargoToml) -> bool {
    let Some(package) = &config.package else {
        return false;
    };

    config.has_explicit_bin_targets()
        || (package.autobins.unwrap_or(true)
            && (dir.join("src/main.rs").is_file() || dir.join("src/bin").is_dir()))
}

/// Whether the component is an application rather than a library: whether its package, or any member of its
/// workspace, builds a binary. A package with both a library and a binary counts as an application, and so does a
/// workspace with both library and binary members.
fn is_application(ctx: &ComponentContext, config: &CargoToml) -> bool {
    builds_binary(ctx.path(), config)
        || ctx
            .cargo_members()
            .parsed()
            .any(|(path, member)| builds_binary(&ctx.path().join(path), member))
}

/// Whether the component is a published library: its `Cargo.toml` parses, neither its package nor any member of its
/// workspace builds a binary, and one of them may be published, as it may unless its `publish` is `false` or `[]`.
pub(crate) fn publishes_library(ctx: &ComponentContext) -> bool {
    ctx.cargo().as_ref_ok().is_some_and(|config| {
        let workspace = config.workspace.as_ref();
        !is_application(ctx, config)
            && (config.is_publishable(workspace)
                || ctx
                    .cargo_members()
                    .parsed()
                    .any(|(_, member)| member.is_publishable(workspace)))
    })
}

/// The label of a workspace member's manifest, by its path from the component.
fn member_manifest(path: &Path) -> String {
    let parts: Vec<_> = path.iter().map(|part| part.to_string_lossy()).collect();
    format!("{}/Cargo.toml", parts.join("/"))
}

//
// Project configuration
//

pub(crate) fn check_cargo(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("cargo-toml", "Cargo.toml", ctx.cargo()) else {
        return;
    };

    // A virtual manifest has no package of its own; its members are checked instead.
    if config.package.is_some() {
        check_package_metadata(ctx, "Cargo.toml", config, config.workspace.as_ref(), report);
    }
}

/// Check the metadata of the package whose manifest, labeled `label`, is `manifest`, with the fields it inherits
/// resolved from `workspace`.
fn check_package_metadata(
    ctx: &ComponentContext,
    label: &str,
    manifest: &CargoToml,
    workspace: Option<&CargoWorkspace>,
    report: &mut Report,
) {
    let package = manifest.metadata(workspace);

    report.should(
        "cargo-toml-license",
        format!("{label}: has license"),
        package.license.is_some(),
    );

    if let Some(license) = package.license {
        report.check(
            "cargo-toml-license-texts",
            format!("{label}: license has its texts in LICENSES/"),
            ctx.license_files().check_expression(license),
        );
    }

    report.should(
        "cargo-toml-description",
        format!("{label}: has description"),
        package.description.is_some(),
    );

    report.should(
        "cargo-toml-repository",
        format!("{label}: has repository"),
        package.repository.is_some(),
    );

    report.should(
        "cargo-toml-edition",
        format!("{label}: has edition"),
        package.edition.is_some(),
    );
}

/// Check that the members of the component's Cargo workspace can be found, and that each has a valid manifest and
/// the package metadata.
pub(crate) fn check_cargo_workspace(ctx: &ComponentContext, report: &mut Report) {
    let Some(workspace) = ctx.cargo().as_ref_ok().and_then(|c| c.workspace.as_ref()) else {
        return;
    };

    let members = ctx.cargo_members();
    let is_empty =
        members.members.is_empty() && ctx.cargo().as_ref_ok().is_some_and(|c| c.package.is_none());
    report.should(
        "cargo-workspace-members",
        "Cargo.toml: workspace members found",
        if !members.errors.is_empty() {
            Outcome::fail(members.errors.join("; "))
        } else if is_empty {
            Outcome::fail("The virtual workspace has no members, so no package is checked")
        } else {
            Outcome::Pass
        },
    );

    for (path, status) in &members.members {
        let label = member_manifest(path);
        if let Some(member) = report.require_parsed("cargo-toml", &label, status) {
            check_package_metadata(ctx, &label, member, Some(workspace), report);
        }
    }
}

//
// Version pinning
//

/// Check that an application pins its dependencies exactly, in its root manifest and in each workspace member's.
pub(crate) fn check_cargo_dependencies(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    if !is_application(ctx, config) {
        return;
    }

    let members = ctx
        .cargo_members()
        .parsed()
        .map(|(path, member)| (member_manifest(path), member));
    for (label, manifest) in std::iter::once(("Cargo.toml".to_owned(), config)).chain(members) {
        report.should(
            "cargo-toml-dependencies-exact",
            format!("{label}: dependencies pinned exactly (application)"),
            manifest.check_dependencies_exact(),
        );
    }
}

//
// MSRV / Toolchain
//

pub(crate) fn check_rust_toolchain(ctx: &ComponentContext, report: &mut Report) {
    let (label, status) = ctx.rust_toolchain();
    if label.ends_with(LEGACY_RUST_TOOLCHAIN) {
        report.should(
            "rust-toolchain-toml-exists",
            format!("{label}: exists"),
            Outcome::fail(format!(
                "rustup uses the legacy {label}, which should be named {RUST_TOOLCHAIN}"
            )),
        );
    } else if let Some(toolchain) = report.require_parsed("rust-toolchain-toml", label, status) {
        report.should(
            "rust-toolchain-toml-channel",
            format!("{label}: exact channel"),
            toolchain.check_channel(),
        );
        report.should(
            "rust-toolchain-toml-components",
            format!("{label}: clippy and rustfmt components"),
            toolchain.check_components(),
        );
    }
}

/// Check that a library sets its MSRV: its package, unless the manifest is virtual, and each member of its workspace
/// set `rust-version`, directly or inherited from the workspace. Applications need none, since `rust-toolchain.toml`
/// already pins their compiler.
pub(crate) fn check_rust_version(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };
    if is_application(ctx, config) {
        return;
    }

    let workspace = config.workspace.as_ref();
    let root = config
        .package
        .is_some()
        .then(|| ("Cargo.toml".to_owned(), config));
    let members = ctx
        .cargo_members()
        .parsed()
        .map(|(path, member)| (member_manifest(path), member));
    for (label, manifest) in root.into_iter().chain(members) {
        report.should(
            "cargo-toml-rust-version",
            format!("{label}: MSRV set (rust-version, library)"),
            manifest.metadata(workspace).rust_version.is_some(),
        );
    }
}

//
// Clippy lints
//

/// Check the Clippy lints of the component's package and of each member of its workspace. A virtual manifest has no
/// package of its own to lint.
pub(crate) fn check_clippy_lints(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };
    let workspace = config.workspace.as_ref();

    if config.package.is_some() {
        check_package_lints("Cargo.toml", config, workspace, report);
    }
    for (path, member) in ctx.cargo_members().parsed() {
        check_package_lints(&member_manifest(path), member, workspace, report);
    }
}

/// Check the Clippy lints of the package whose manifest, labeled `label`, is `manifest`, with any it inherits resolved
/// from `workspace`.
fn check_package_lints(
    label: &str,
    manifest: &CargoToml,
    workspace: Option<&CargoWorkspace>,
    report: &mut Report,
) {
    let name = format!("{label}: Clippy lints configured");
    let lints = match manifest.clippy_lints(workspace) {
        Ok(lints) => lints,
        Err(error) => {
            report.must("clippy-lints", name, Outcome::fail(error));
            return;
        }
    };

    report.must("clippy-lints", name, Outcome::Pass);
    report.should(
        "clippy-pedantic",
        format!("{label}: Clippy pedantic enabled"),
        lints.contains_key("pedantic"),
    );
}

/// Check the Clippy configurations that apply to the component: the one its package uses, and any that members of its
/// workspace use instead. Each must let tests use `unwrap`, `expect` and `panic`, and limit absolute paths to three
/// segments.
pub(crate) fn check_clippy_toml(ctx: &ComponentContext, report: &mut Report) {
    for (label, status) in std::iter::once(ctx.clippy_toml()).chain(ctx.member_clippy_tomls()) {
        if let Some(config) = report.require_parsed("clippy-toml", label, status) {
            check_clippy_toml_settings(label, config, report);
        }
    }
}

fn check_clippy_toml_settings(label: &str, config: &ClippyToml, report: &mut Report) {
    report.should(
        "clippy-toml-allow-in-tests",
        format!("{label}: unwrap, expect and panic allowed in tests"),
        config.check_allowed_in_tests(),
    );
    report.should(
        "clippy-toml-absolute-paths",
        format!("{label}: absolute paths limited to 3 segments"),
        config.check_absolute_paths_max_segments(),
    );
}

//
// Dependency auditing
//

/// Check the component's `deny.toml`: that it allows multiple versions of a crate, and that every ignored advisory
/// gives a reason.
pub(crate) fn check_cargo_deny(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("deny-toml", "deny.toml", ctx.deny_toml()) else {
        return;
    };

    report.should(
        "deny-toml-multiple-versions",
        "deny.toml: multiple versions allowed (bans)",
        config.check_multiple_versions(),
    );
    report.should(
        "deny-toml-ignore-reasons",
        "deny.toml: every ignored advisory has a reason",
        config.check_ignore_reasons(),
    );
}

//
// Lock file
//

pub(crate) fn check_cargo_lock(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    if is_application(ctx, config) {
        report.must(
            "cargo-lock-exists",
            "Rust: Cargo.lock comitted (binary)",
            file_exists(&ctx.path().join("Cargo.lock")),
        );
    }
}

//
// MSRV in CI
//

/// Whether a script line compiles the crate: `cargo check`, `cargo build` or `cargo test`, with or without a
/// `+toolchain`.
fn compiles_crate(line: &str) -> bool {
    commands(line).any(|words| {
        cargo_subcommand(&words)
            .is_some_and(|(subcommand, _)| matches!(subcommand, "check" | "build" | "test"))
    })
}

/// The toolchain a setup-rust-toolchain step makes the default for the job: the last of its comma-separated
/// `toolchain` input, unless `override: false` leaves `rust-toolchain.toml` in charge.
fn default_toolchain(step: &WorkflowStep) -> Option<String> {
    if step.input("override").as_deref() == Some("false") {
        return None;
    }
    let toolchains = step.input("toolchain")?;
    Some(
        toolchains
            .rsplit(',')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned(),
    )
}

/// Whether the toolchain `toolchain` is the Rust version `msrv`: the same major and minor version, and the same patch
/// version if both give one.
fn is_msrv_toolchain(toolchain: &str, msrv: &str) -> bool {
    let (Ok(toolchain), Ok(msrv)) = (
        toolchain.trim().parse::<Comparator>(),
        msrv.trim().parse::<Comparator>(),
    ) else {
        return false;
    };
    toolchain.major == msrv.major
        && toolchain.minor.is_some()
        && toolchain.minor == msrv.minor
        && (toolchain.patch.is_none() || msrv.patch.is_none() || toolchain.patch == msrv.patch)
}

/// Whether some job of `workflow` that compiles the crate installs each of `msrvs` as its default toolchain, through
/// setup-rust-toolchain's `toolchain` input. Without any such job, the MSRV isn't checked at all, which fails; a job
/// checking with another version warns.
fn msrvs_tested(msrvs: &BTreeSet<&str>, workflow: &Workflow) -> Result<(), Issue> {
    let toolchains: Vec<String> = workflow
        .steps_using_in_jobs_running("actions-rust-lang/setup-rust-toolchain", compiles_crate)
        .filter_map(default_toolchain)
        .collect();
    if toolchains.is_empty() {
        return Err(Issue::fail(
            "No job installs a toolchain with setup-rust-toolchain's toolchain input and runs cargo check",
        ));
    }
    // cargo-toml-rust-version reports the missing MSRV itself.
    if msrvs.is_empty() {
        return Err(Issue::warn(
            "Cargo.toml sets no rust-version for the job to match",
        ));
    }

    let untested: Vec<&str> = msrvs
        .iter()
        .filter(|msrv| {
            !toolchains
                .iter()
                .any(|toolchain| is_msrv_toolchain(toolchain, msrv))
        })
        .copied()
        .collect();
    if untested.is_empty() {
        Ok(())
    } else {
        Err(Issue::warn(format!(
            "No job checks with the rust-version {} (the jobs install {})",
            untested.join(", "),
            toolchains.join(", ")
        )))
    }
}

/// The `rust-version` of each package in the component: its own package's and its workspace members', with inherited
/// values resolved.
fn declared_msrvs<'a>(ctx: &'a ComponentContext, config: &'a CargoToml) -> BTreeSet<&'a str> {
    let workspace = config.workspace.as_ref();
    std::iter::once(config)
        .chain(ctx.cargo_members().parsed().map(|(_, member)| member))
        .filter_map(|manifest| manifest.metadata(workspace).rust_version)
        .collect()
}

/// Check that a library's CI checks it with its MSRV: some job that compiles the crate installs each `rust-version`
/// its packages set.
pub(crate) fn check_msrv_job(
    project: &ProjectContext,
    ctx: &ComponentContext,
    report: &mut Report,
) {
    let (Some(config), ConfigStatus::Ok(workflow)) =
        (ctx.cargo().as_ref_ok(), project.ci_workflow())
    else {
        return;
    };
    if is_application(ctx, config) {
        return;
    }

    report.check(
        "rust-ci-msrv",
        "Rust CI: MSRV checked (library)",
        msrvs_tested(&declared_msrvs(ctx, config), workflow),
    );
}

//
// Release age
//

/// Check the release age in the root's `.cargo/config.toml`, which applies wherever Cargo runs in the repository, and
/// in the `.cargo/config.toml` of any Rust component in a subdirectory, which overrides it there.
pub(crate) fn check_cargo_config(ctx: &ProjectContext, report: &mut Report) {
    check_cargo_config_file(ctx.cargo_config(), report);

    for component_ctx in ctx.component_contexts().iter().filter(|c| {
        c.component().language == Language::Rust && !c.component().path.as_os_str().is_empty()
    }) {
        if !matches!(component_ctx.cargo_config(), ConfigStatus::NotFound) {
            report.with_component(component_ctx.component(), |report| {
                check_cargo_config_file(component_ctx.cargo_config(), report);
            });
        }
    }
}

fn check_cargo_config_file(status: &ConfigStatus<CargoConfig>, report: &mut Report) {
    let Some(config) = report.require_parsed("cargo-config", ".cargo/config.toml", status) else {
        return;
    };

    report.should(
        "cargo-config-min-publish-age",
        "Cargo: release age of 3 days (global-min-publish-age)",
        config.check_min_publish_age(),
    );
}

//
// Pre-commit integration
//

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.check(
        "rust-precommit-cargo-lock",
        "Rust pre-commit: cargo-lock hook",
        config.check_commit_hook("cargo-lock"),
    );

    report.check(
        "rust-precommit-rustfmt",
        "Rust pre-commit: rustfmt hook",
        config.check_commit_hook("rustfmt"),
    );

    report.check(
        "rust-precommit-clippy",
        "Rust pre-commit: clippy hook",
        config.check_commit_hook("clippy"),
    );

    report.check(
        "rust-precommit-cargo-test",
        "Rust pre-commit: cargo-test hook",
        config.check_commit_hook("cargo-test"),
    );

    report.check(
        "rust-precommit-cargo-deny",
        "Rust pre-commit: cargo-deny hook",
        config.check_commit_hook("cargo-deny"),
    );
}

//
// CI integration
//

pub(crate) fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    report.must(
        "rust-ci-cargo-lock",
        "Rust CI: Cargo.lock up to date",
        workflow.run_lines().any(|line| checks_cargo_lock(&line)),
    );
    report.check(
        "rust-ci-rustfmt",
        "Rust CI: format check",
        if workflow.uses_action("actions-rust-lang/rustfmt") {
            Ok(())
        } else {
            workflow.check_formatter("cargo fmt", rustfmt_run)
        },
    );
    report.must(
        "rust-ci-clippy",
        "Rust CI: clippy check",
        workflow.runs_command("cargo clippy"),
    );
    report.must(
        "rust-ci-test",
        "Rust CI: test",
        workflow.runs_command("cargo test"),
    );
    report.must(
        "rust-ci-cargo-deny",
        "Rust CI: cargo deny",
        workflow.runs_command("cargo deny")
            || workflow.uses_action("EmbarkStudios/cargo-deny-action"),
    );
}

/// Whether a command's words run rustfmt, through `cargo fmt` or as the command itself, and if so how: with `--check`,
/// which fails on unformatted files instead of rewriting them, or not. `cargo fmt` takes the flag itself or passes it
/// on after `--`. A command that only names rustfmt, such as `rustup component add rustfmt`, doesn't run it.
fn rustfmt_run(words: &[&str]) -> Option<FormatterRun> {
    let args = match cargo_subcommand(words) {
        Some(("fmt", args)) => args,
        _ => words
            .split_first()
            .filter(|(program, _)| program.rsplit('/').next() == Some("rustfmt"))?
            .1
            .to_vec(),
    };
    Some(FormatterRun::from_args(&args, &["--check"], &[]))
}

/// The actions that build no Rust code.
const NON_BUILDING_ACTIONS: [&str; 5] = [
    "actions/checkout",
    "actions-rust-lang/rustfmt",
    "actions-rust-lang/setup-rust-toolchain",
    "EmbarkStudios/cargo-deny-action",
    "taiki-e/install-action",
];

/// The Cargo subcommands that build no Rust code.
const NON_BUILDING_CARGO_SUBCOMMANDS: [&str; 9] = [
    "deny",
    "fetch",
    "fmt",
    "generate-lockfile",
    "metadata",
    "tree",
    "update",
    "version",
    "--version",
];

/// Whether a command, as its words, builds no Rust code: it is empty or a comment, runs `rustfmt`, runs `rustup` other
/// than `rustup run`, which runs any command, or runs one of the Cargo subcommands that don't build. Any other command
/// may build.
fn builds_nothing(words: &[&str]) -> bool {
    match words.first() {
        Some(word) if word.starts_with('#') => true,
        None | Some(&"rustfmt") => true,
        Some(&"rustup") => words.get(1) != Some(&"run"),
        Some(&"cargo") => cargo_subcommand(words)
            .is_some_and(|(subcommand, _)| NON_BUILDING_CARGO_SUBCOMMANDS.contains(&subcommand)),
        Some(_) => false,
    }
}

/// Whether a job builds no Rust code, so that caching its build is wasted: each of its steps uses one of the actions
/// that build nothing, and every command of its scripts builds nothing. A job running an unrecognized action or
/// command may build, so it is never counted.
fn job_builds_nothing(job: &WorkflowJob) -> bool {
    job.steps.iter().all(|step| {
        step.uses.is_none()
            || NON_BUILDING_ACTIONS
                .iter()
                .any(|action| step.uses_action(action))
    }) && job
        .run_lines()
        .all(|line| commands(&line).all(|words| builds_nothing(&words)))
}

/// The jobs of `workflow`, by ID, that build no Rust code but cache it: a setup-rust-toolchain step in them doesn't
/// set `cache: false`.
fn jobs_caching_nothing(workflow: &Workflow) -> Vec<&str> {
    workflow
        .jobs
        .iter()
        .filter(|(_, job)| {
            job_builds_nothing(job)
                && job
                    .steps_using("actions-rust-lang/setup-rust-toolchain")
                    .any(|step| step.input("cache").as_deref() != Some("false"))
        })
        .map(|(id, _)| id.as_str())
        .collect()
}

/// Check that the jobs of every workflow that build no Rust code, such as formatting, turn off setup-rust-toolchain's
/// cache with `cache: false`.
pub(crate) fn check_toolchain_cache(ctx: &ProjectContext, report: &mut Report) {
    let mut parsed = false;
    let mut caching = Vec::new();
    for (name, status) in ctx.workflows() {
        if let ConfigStatus::Ok(workflow) = status {
            parsed = true;
            caching.extend(
                jobs_caching_nothing(workflow)
                    .into_iter()
                    .map(|job| format!("{name}: {job}")),
            );
        }
    }
    if !parsed {
        return;
    }

    report.should(
        "rust-ci-toolchain-cache",
        "Rust CI: jobs that build nothing set cache: false",
        if caching.is_empty() {
            Outcome::Pass
        } else {
            Outcome::fail(format!(
                "These jobs build nothing, but setup-rust-toolchain caches their build: {}",
                caching.join(", ")
            ))
        },
    );
}

/// The Cargo subcommand a command's words run, after any `+toolchain`, and the arguments that follow it.
fn cargo_subcommand<'a>(words: &[&'a str]) -> Option<(&'a str, Vec<&'a str>)> {
    let mut words = words
        .iter()
        .copied()
        .skip_while(|word| *word != "cargo")
        .skip(1)
        .skip_while(|word| word.starts_with('+'));
    Some((words.next()?, words.collect()))
}

/// Whether a script line checks that `Cargo.lock` is up to date: `cargo update` with `--locked` (or `--frozen`, which
/// implies it), which fails instead of changing the lock file, and `--workspace` (or `-w`), which limits it to the
/// workspace's own packages so that a current lock file needs no change.
fn checks_cargo_lock(line: &str) -> bool {
    commands(line).any(|words| {
        cargo_subcommand(&words).is_some_and(|(subcommand, args)| {
            subcommand == "update"
                && args
                    .iter()
                    .any(|arg| matches!(*arg, "--locked" | "--frozen"))
                && args.iter().any(|arg| matches!(*arg, "--workspace" | "-w"))
        })
    })
}

/// The checks cargo-deny's `check` command runs: those it names, or every one if it names none.
const CARGO_DENY_CHECKS: [&str; 4] = ["advisories", "bans", "licenses", "sources"];

/// Whether cargo-deny's arguments, as they follow `cargo deny` or as the action's `command` and `command-arguments`
/// inputs give them, run its advisories check: `check`, naming either no checks or `advisories` among them. The
/// arguments end at a shell operator, where another command begins.
fn cargo_deny_checks_advisories(args: &str) -> bool {
    let args = args.split(['&', '|', ';']).next().unwrap_or_default();
    let mut words = args.split_whitespace().skip_while(|word| *word != "check");
    if words.next().is_none() {
        return false;
    }
    let checks: Vec<&str> = words
        .filter(|word| CARGO_DENY_CHECKS.contains(word))
        .collect();
    checks.is_empty() || checks.contains(&"advisories")
}

/// Whether `workflow` runs cargo-deny's advisories check, either from a script line or through
/// `EmbarkStudios/cargo-deny-action`, whose `command` input defaults to `check` and is followed by its
/// `command-arguments`.
fn runs_cargo_deny_advisories(workflow: &Workflow) -> bool {
    let runs = workflow.run_lines().any(|line| {
        ["cargo deny", "cargo-deny"].iter().any(|command| {
            line.split_once(command)
                .is_some_and(|(_, args)| cargo_deny_checks_advisories(args))
        })
    });
    runs || workflow
        .steps_using("EmbarkStudios/cargo-deny-action")
        .any(|step| {
            let command = step.input("command");
            let arguments = step.input("command-arguments");
            cargo_deny_checks_advisories(&format!(
                "{} {}",
                command.as_deref().unwrap_or("check"),
                arguments.as_deref().unwrap_or_default()
            ))
        })
}

/// Check that `audit.yaml` runs cargo-deny's advisories check.
pub(crate) fn check_audit(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.audit_workflow() else {
        return;
    };

    report.must(
        "rust-audit-cargo-deny",
        "Rust audit: cargo deny advisories",
        runs_cargo_deny_advisories(workflow),
    );
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::{Component, Status};

    /// Whether a package with the given manifest and files is an application.
    fn is_application_with(manifest: &str, files: &[&str]) -> bool {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            let content = if path.ends_with("Cargo.toml") {
                "[package]\nname = \"member\"\n"
            } else {
                "fn main() {}\n"
            };
            fs::write(path, content).unwrap();
        }

        let component = Component {
            language: Language::Rust,
            path: PathBuf::new(),
        };
        let project = ProjectContext::new(dir.path().to_path_buf());
        let ctx = ComponentContext::new(&project, &component);
        let ConfigStatus::Ok(config) = ctx.cargo() else {
            panic!("invalid manifest");
        };
        is_application(&ctx, config)
    }

    #[test]
    fn applications_build_a_binary() {
        let package = "[package]\nname = \"x\"\n";
        assert!(is_application_with(package, &["src/main.rs"]));
        assert!(is_application_with(package, &["src/lib.rs", "src/main.rs"]));
        assert!(is_application_with(package, &["src/bin/tool.rs"]));
        assert!(is_application_with(
            "[package]\nname = \"x\"\n\n[[bin]]\nname = \"tool\"\npath = \"tool.rs\"\n",
            &["tool.rs"]
        ));
        assert!(is_application_with(
            "[package]\nname = \"x\"\nautobins = false\n\n[[bin]]\nname = \"x\"\npath = \"src/main.rs\"\n",
            &["src/main.rs"]
        ));

        assert!(!is_application_with(package, &["src/lib.rs"]));
        assert!(!is_application_with(
            "[package]\nname = \"x\"\nautobins = false\n",
            &["src/lib.rs", "src/main.rs", "src/bin/tool.rs"]
        ));
    }

    #[test]
    fn workspaces_with_a_binary_member_are_applications() {
        let workspace = "[workspace]\nmembers = [\"crates/*\"]\n";
        assert!(is_application_with(
            workspace,
            &[
                "crates/lib/Cargo.toml",
                "crates/lib/src/lib.rs",
                "crates/app/Cargo.toml",
                "crates/app/src/main.rs"
            ]
        ));
        assert!(is_application_with(
            "[package]\nname = \"x\"\n\n[workspace]\n",
            &["src/main.rs"]
        ));

        assert!(!is_application_with(
            workspace,
            &["crates/lib/Cargo.toml", "crates/lib/src/lib.rs"]
        ));
        // A virtual manifest builds nothing itself, and a directory without a manifest isn't a package.
        assert!(!is_application_with(
            workspace,
            &["src/main.rs", "crates/app/src/main.rs"]
        ));
    }

    #[test]
    fn empty_virtual_workspaces_warn() {
        let warnings = |manifest: &str| {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
            let component = Component {
                language: Language::Rust,
                path: PathBuf::new(),
            };
            let project = ProjectContext::new(dir.path().to_path_buf());
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_cargo_workspace(&ComponentContext::new(&project, &component), &mut report);
            report.warnings()
        };

        assert_eq!(warnings("[workspace]\n"), 1);
        assert_eq!(warnings("[package]\nname = \"x\"\n\n[workspace]\n"), 0);
    }

    #[test]
    fn msrv_toolchains_match_rust_version() {
        for (toolchain, msrv) in [("1.85", "1.85"), ("1.85.0", "1.85"), ("1.85", "1.85.1")] {
            assert!(is_msrv_toolchain(toolchain, msrv), "{toolchain} {msrv}");
        }
        for (toolchain, msrv) in [
            ("1.86", "1.85"),
            ("1.85.1", "1.85.0"),
            ("1", "1.85"),
            ("stable", "1.85"),
            ("${{ matrix.rust }}", "1.85"),
        ] {
            assert!(!is_msrv_toolchain(toolchain, msrv), "{toolchain} {msrv}");
        }
    }

    #[test]
    fn msrv_jobs_install_every_rust_version() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  msrv:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: \"nightly, 1.85\"
      - run: cargo check --workspace --all-features --locked
  clippy:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: \"1.90\"
      - run: cargo clippy --workspace
  kept:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: \"1.80\"
          override: false
      - run: cargo test
  listed:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v1
        with:
          toolchain: \"1.81, nightly\"
      - run: cargo build
",
        )
        .unwrap();
        assert_eq!(msrvs_tested(&BTreeSet::from(["1.85"]), &workflow), Ok(()));
        for msrv in ["1.90", "1.80", "1.81"] {
            assert_eq!(
                msrvs_tested(&BTreeSet::from(["1.85", msrv]), &workflow),
                Err(Issue::warn(format!(
                    "No job checks with the rust-version {msrv} (the jobs install nightly, 1.85)"
                ))),
                "{msrv}"
            );
        }
        assert_eq!(
            msrvs_tested(&BTreeSet::new(), &workflow),
            Err(Issue::warn(
                "Cargo.toml sets no rust-version for the job to match"
            ))
        );
        assert_eq!(
            msrvs_tested(&BTreeSet::from(["1.85"]), &Workflow::default()),
            Err(Issue::fail(
                "No job installs a toolchain with setup-rust-toolchain's toolchain input and runs cargo check"
            ))
        );
        for line in [
            "cargo check",
            "cargo +1.85 test --locked",
            "cd crate && cargo build",
        ] {
            assert!(compiles_crate(line), "{line}");
        }
        for line in ["cargo clippy", "cargo fmt --check", "echo cargo-check"] {
            assert!(!compiles_crate(line), "{line}");
        }
    }

    #[test]
    fn workspace_members_declare_their_msrvs() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[workspace]\nmembers = [\"a\", \"b\", \"c\"]\n\n[workspace.package]\nrust-version = \"1.85\"\n",
        )
        .unwrap();
        for (member, rust_version) in [
            ("a", "rust-version.workspace = true"),
            ("b", "rust-version = \"1.88\""),
            ("c", ""),
        ] {
            fs::create_dir_all(dir.path().join(member)).unwrap();
            fs::write(
                dir.path().join(member).join("Cargo.toml"),
                format!("[package]\nname = \"{member}\"\n{rust_version}\n"),
            )
            .unwrap();
        }

        let component = Component {
            language: Language::Rust,
            path: PathBuf::new(),
        };
        let project = ProjectContext::new(dir.path().to_path_buf());
        let ctx = ComponentContext::new(&project, &component);
        let ConfigStatus::Ok(config) = ctx.cargo() else {
            panic!("invalid manifest");
        };
        assert_eq!(
            declared_msrvs(&ctx, config),
            BTreeSet::from(["1.85", "1.88"])
        );
    }

    #[test]
    fn deny_toml_allows_multiple_versions_and_explains_ignores() {
        let results = |deny: &str| {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("deny.toml"), deny).unwrap();
            let component = Component {
                language: Language::Rust,
                path: PathBuf::new(),
            };
            let project = ProjectContext::new(dir.path().to_path_buf());
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_cargo_deny(&ComponentContext::new(&project, &component), &mut report);
            report
                .results
                .iter()
                .map(|r| (r.id.clone(), r.status))
                .collect::<Vec<_>>()
        };
        let ids = [
            "deny-toml-exists",
            "deny-toml-valid",
            "deny-toml-multiple-versions",
            "deny-toml-ignore-reasons",
        ];
        let with = |statuses: [Status; 4]| {
            ids.iter()
                .map(|id| (*id).to_owned())
                .zip(statuses)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            results("[bans]\nmultiple-versions = \"allow\"\n"),
            with([Status::Pass; 4])
        );
        assert_eq!(
            results(
                "[advisories]\nignore = [\"RUSTSEC-2024-0001\"]\n\n[bans]\nmultiple-versions = \"allow\"\n"
            ),
            with([Status::Pass, Status::Pass, Status::Pass, Status::Warn])
        );
        assert_eq!(
            results("[bans]\nmultiple-versions = \"deny\"\n"),
            with([Status::Pass, Status::Pass, Status::Warn, Status::Pass])
        );
    }

    #[test]
    fn libraries_set_their_msrvs() {
        let results = |manifest: &str, files: &[(&str, &str)]| {
            let dir = tempfile::tempdir().unwrap();
            fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
            for (path, contents) in files {
                let path = dir.path().join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, contents).unwrap();
            }
            let component = Component {
                language: Language::Rust,
                path: PathBuf::new(),
            };
            let project = ProjectContext::new(dir.path().to_path_buf());
            let mut report = Report::new(dir.path().to_path_buf(), Vec::new());
            check_rust_version(&ComponentContext::new(&project, &component), &mut report);
            report
                .results
                .iter()
                .map(|result| (result.name.clone(), result.status))
                .collect::<Vec<_>>()
        };
        let workspace = "[workspace]\nmembers = [\"crates/*\"]\n\n[workspace.package]\nrust-version = \"1.85\"\n";
        let members = [
            (
                "crates/a/Cargo.toml",
                "[package]\nname = \"a\"\nrust-version.workspace = true\n",
            ),
            ("crates/b/Cargo.toml", "[package]\nname = \"b\"\n"),
        ];

        // The virtual root isn't a package, so only its members need an MSRV.
        assert_eq!(
            results(workspace, &members),
            [
                (
                    "crates/a/Cargo.toml: MSRV set (rust-version, library)".to_owned(),
                    Status::Pass
                ),
                (
                    "crates/b/Cargo.toml: MSRV set (rust-version, library)".to_owned(),
                    Status::Warn
                ),
            ]
        );
        assert_eq!(
            results("[package]\nname = \"x\"\nrust-version = \"1.85\"\n", &[]),
            [(
                "Cargo.toml: MSRV set (rust-version, library)".to_owned(),
                Status::Pass
            )]
        );

        // An application needs no MSRV, nor do its members.
        let none = Vec::<(String, Status)>::new();
        assert_eq!(
            results("[package]\nname = \"x\"\n", &[("src/main.rs", "")]),
            none
        );
        assert_eq!(
            results(workspace, &[members[1], ("crates/b/src/main.rs", "")]),
            none
        );
    }

    #[test]
    fn cargo_lock_needs_locked_workspace_update() {
        for line in [
            "cargo update --workspace --locked",
            "cargo +1.99.1 update -w --locked --manifest-path backend/Cargo.toml",
            "cd backend && cargo update --locked --workspace",
            "cargo update --workspace --frozen",
        ] {
            assert!(checks_cargo_lock(line), "{line}");
        }
        for line in [
            "cargo update --workspace",
            "cargo update --locked",
            "cargo test --workspace --locked",
            "cargo update; cargo test --workspace --locked",
            "echo cargo-update --workspace --locked",
        ] {
            assert!(!checks_cargo_lock(line), "{line}");
        }
    }

    #[test]
    fn jobs_building_nothing_disable_the_cache() {
        let workflow: Workflow = serde_saphyr::from_str(
            "
jobs:
  rustfmt:
    steps:
      - uses: actions/checkout@v6
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - uses: actions-rust-lang/rustfmt@v1
  lock:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - run: |
          # Fails if the lock file is stale.
          cargo +1.99.1 update --workspace --locked && cargo tree
  uncached:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
        with:
          cache: \"false\"
      - run: cargo fmt --check
  deny:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - uses: taiki-e/install-action@v2
        with:
          tool: cargo-deny
      - run: cargo deny check bans; rustup show
  test:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - run: cargo fmt --check && cargo test
  rustup-run:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - run: rustup run stable cargo build
  hooks:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - run: uvx pre-commit run --all-files
  build-action:
    steps:
      - uses: actions-rust-lang/setup-rust-toolchain@v2
      - uses: ./.github/actions/build
  no-toolchain:
    steps:
      - run: cargo fmt --check
  reusable:
    uses: ./.github/workflows/rust.yaml
",
        )
        .unwrap();
        assert_eq!(jobs_caching_nothing(&workflow), ["deny", "lock", "rustfmt"]);
    }

    #[test]
    fn cargo_deny_must_check_advisories() {
        for args in [
            "check",
            "--workspace --all-features check -D warnings advisories",
            "check advisories bans",
            "check -D warnings",
        ] {
            assert!(cargo_deny_checks_advisories(args), "{args}");
        }
        for args in [
            "",
            "fetch",
            "check bans licenses sources",
            "list advisories",
            "fetch && echo check",
            "check bans; echo advisories",
        ] {
            assert!(!cargo_deny_checks_advisories(args), "{args}");
        }
    }

    #[test]
    fn audit_runs_cargo_deny_advisories() {
        let runs = |steps: &str| {
            let yaml = format!("jobs:\n  audit:\n    steps:\n{steps}");
            runs_cargo_deny_advisories(&serde_saphyr::from_str(&yaml).expect("workflow parses"))
        };
        let action = "      - uses: EmbarkStudios/cargo-deny-action@abc # v2.1.1\n";
        for steps in [
            "      - run: cargo deny --workspace check -D warnings advisories\n".to_owned(),
            "      - run: |\n          cargo deny --workspace --all-features \\\n            check -D warnings advisories\n".to_owned(),
            action.to_owned(),
            format!("{action}        with:\n          command: check advisories\n"),
        ] {
            assert!(runs(&steps), "{steps}");
        }
        for steps in [
            "      - run: cargo deny check bans\n".to_owned(),
            "      - run: cargo build\n".to_owned(),
            format!(
                "{action}        with:\n          command: check\n          command-arguments: bans licenses\n"
            ),
        ] {
            assert!(!runs(&steps), "{steps}");
        }
    }

    #[test]
    fn rustfmt_in_ci_checks_rather_than_writes() {
        let checks = |line: &str| rustfmt_run(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "cargo fmt --all --check",
            "cargo +nightly fmt --check",
            "cargo fmt --all -- --check",
            "rustfmt --check src/main.rs",
        ] {
            assert_eq!(checks(line), Some(FormatterRun::Checks), "{line}");
        }
        for line in ["cargo fmt --all", "cargo fmt", "rustfmt src/main.rs"] {
            assert_eq!(checks(line), Some(FormatterRun::Rewrites), "{line}");
        }
        for line in [
            "cargo clippy --check",
            "cargo test",
            "echo cargo-fmt --check",
            "rustup component add rustfmt",
            "rustup component add rustfmt clippy",
        ] {
            assert_eq!(checks(line), None, "{line}");
        }
    }
}
