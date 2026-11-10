// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Rust-specific checks.

use std::collections::{BTreeSet, HashMap};
use std::path::Path;

use semver::Comparator;

use crate::config::{
    CLIPPY_TOML, CargoConfig, CargoToml, CargoWorkspace, ClippyToml, DENY_TOML, FormatterRun,
    HIDDEN_CLIPPY_TOML, LintLevel, PrecommitConfig, Workflow, WorkflowJob, WorkflowStep, commands,
    is_other_deny_toml, is_program,
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

/// The latest Rust edition, which the standards require. It changes with the template's `Cargo.toml` when a new
/// edition ships.
const RUST_EDITION: &str = "2024";

/// Check that a package's `edition` is [`RUST_EDITION`].
fn check_edition(edition: Option<&str>) -> Result<(), String> {
    match edition {
        Some(RUST_EDITION) => Ok(()),
        Some(edition) => Err(format!("edition is {edition}, not {RUST_EDITION}")),
        None => Err("Missing edition".to_owned()),
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
        format!("{label}: edition {RUST_EDITION}"),
        check_edition(package.edition),
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
// Clippy and compiler lints
//

/// Check the Clippy and compiler lints of the component's package and of each member of its workspace. A virtual
/// manifest has no package of its own to lint.
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

/// The Clippy lint groups the standard enables, with the IDs of their checks.
const CLIPPY_GROUPS: [(&str, &str); 3] = [
    ("clippy-pedantic", "pedantic"),
    ("clippy-nursery", "nursery"),
    ("clippy-cargo", "cargo"),
];

/// The compiler's lints and lint groups the standard sets in `[lints.rust]`, with the least level each needs.
const RUSTC_LINTS: [(&str, LintLevel); 5] = [
    ("future_incompatible", LintLevel::Warn),
    ("let_underscore", LintLevel::Warn),
    ("unreachable_pub", LintLevel::Warn),
    ("unsafe_code", LintLevel::Forbid),
    ("unused_qualifications", LintLevel::Warn),
];

/// Whether `lints` sets the lint `name` to `level` or a stricter one.
fn lint_at_least(
    lints: &HashMap<String, toml::Value>,
    name: &str,
    level: LintLevel,
) -> Result<(), String> {
    match lints.get(name).map(LintLevel::of) {
        None => Err(format!("{name} isn't set")),
        Some(Some(set)) if set >= level => Ok(()),
        Some(Some(set)) if level == LintLevel::Forbid => {
            Err(format!("{name} is {}, not forbid", set.name()))
        }
        Some(Some(set)) => Err(format!(
            "{name} is {}, not {} or stricter",
            set.name(),
            level.name()
        )),
        Some(None) => Err(format!("{name} has no level norms recognizes")),
    }
}

/// Check the Clippy and compiler lints of the package whose manifest, labeled `label`, is `manifest`, with any it
/// inherits resolved from `workspace`.
fn check_package_lints(
    label: &str,
    manifest: &CargoToml,
    workspace: Option<&CargoWorkspace>,
    report: &mut Report,
) {
    let name = format!("{label}: Clippy lints configured");
    match manifest.clippy_lints(workspace) {
        Ok(lints) => {
            report.must("clippy-lints", name, Outcome::Pass);
            for (id, group) in CLIPPY_GROUPS {
                report.should(
                    id,
                    format!("{label}: Clippy {group} enabled"),
                    lint_at_least(lints, group, LintLevel::Warn),
                );
            }
        }
        Err(error) => {
            report.must("clippy-lints", name, Outcome::fail(error));
        }
    }

    report.should(
        "rustc-lints",
        format!("{label}: compiler lints configured ([lints.rust])"),
        manifest.lints("rust", workspace).and_then(|lints| {
            let issues: Vec<String> = RUSTC_LINTS
                .into_iter()
                .filter_map(|(lint, level)| lint_at_least(lints, lint, level).err())
                .collect();
            if issues.is_empty() {
                Ok(())
            } else {
                Err(issues.join("; "))
            }
        }),
    );
}

/// Check the Clippy configurations that apply to the component: the one its package uses, and any that members of its
/// workspace use instead. Each must let tests use `unwrap`, `expect` and `panic`, and limit absolute paths to three
/// segments.
pub(crate) fn check_clippy_toml(ctx: &ComponentContext, report: &mut Report) {
    for (label, status) in std::iter::once(ctx.clippy_toml()).chain(ctx.member_clippy_tomls()) {
        if label.rsplit('/').next() == Some(HIDDEN_CLIPPY_TOML) {
            report.should(
                "clippy-toml-exists",
                format!("{label}: exists"),
                Outcome::fail(format!(
                    "Clippy uses {label}, which should be named {CLIPPY_TOML}"
                )),
            );
        } else if let Some(config) = report.require_parsed("clippy-toml", label, status) {
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

/// Check the cargo-deny configuration that applies to the component: that it is named `deny.toml`, allows multiple
/// versions of a crate, checks for crates that `std` replaces, and gives a reason for every ignored advisory.
pub(crate) fn check_cargo_deny(ctx: &ComponentContext, report: &mut Report) {
    let (label, status) = ctx.deny_toml();
    if is_other_deny_toml(Path::new(label)) {
        report.should(
            "deny-toml-exists",
            format!("{label}: exists"),
            Outcome::fail(format!(
                "cargo-deny uses {label}, which should be named {DENY_TOML}"
            )),
        );
        return;
    }
    let Some(config) = report.require_parsed("deny-toml", label, status) else {
        return;
    };

    report.should(
        "deny-toml-multiple-versions",
        format!("{label}: multiple versions allowed (bans)"),
        config.check_multiple_versions(),
    );
    report.should(
        "deny-toml-std-replacements",
        format!("{label}: crates that std replaces are banned"),
        config.check_std_replacements(),
    );
    report.should(
        "deny-toml-ignore-reasons",
        format!("{label}: every ignored advisory has a reason"),
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
            "Rust: Cargo.lock committed (binary)",
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

/// The IDs of Rust's language hooks, which the standard's hook table lists.
const LANGUAGE_HOOKS: [&str; 5] = [
    "cargo-lock",
    "rustfmt",
    "clippy",
    "cargo-test",
    "cargo-deny",
];

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
        "Rust pre-commit: rustfmt hook (--check)",
        config.check_formatter_hook("rustfmt", "rustfmt", rustfmt_run),
    );

    report.check(
        "rust-precommit-clippy",
        "Rust pre-commit: clippy hook (-D warnings)",
        clippy_hooks(config),
    );

    report.check(
        "rust-precommit-cargo-test",
        "Rust pre-commit: cargo-test hook",
        config.check_commit_hook("cargo-test"),
    );

    report.check(
        "rust-precommit-cargo-deny",
        "Rust pre-commit: cargo-deny hook",
        cargo_deny_hooks(config),
    );
    report.should(
        "rust-precommit-local-hooks",
        "Rust pre-commit: language hooks are local, unsupported and take no file names",
        config.check_language_hooks(&LANGUAGE_HOOKS),
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
        rustfmt_in_ci(workflow),
    );
    report.check(
        "rust-ci-clippy",
        "Rust CI: clippy check (-D warnings)",
        clippy_in_ci(workflow),
    );
    report.must(
        "rust-ci-test",
        "Rust CI: test",
        workflow.runs_command("cargo test"),
    );
    report.check(
        "rust-ci-cargo-deny",
        "Rust CI: cargo deny",
        cargo_deny_in_ci(workflow),
    );
}

/// The action that checks Rust formatting in CI.
const RUSTFMT_ACTION: &str = "actions-rust-lang/rustfmt";

/// The action that runs cargo-deny in CI and the audit.
const CARGO_DENY_ACTION: &str = "EmbarkStudios/cargo-deny-action";

/// The arguments CI passes to cargo-deny, as the standard's hook table gives them.
const CARGO_DENY_CI_ARGS: &str =
    "--workspace --all-features check -D warnings bans licenses sources";

/// The arguments the audit passes to cargo-deny, as the standard's audit tools give them.
const CARGO_DENY_AUDIT_ARGS: &str = "--workspace --all-features check -D warnings advisories";

/// The arguments a [`CARGO_DENY_ACTION`] step passes to cargo-deny after its own options: its `arguments`, `command`
/// and `command-arguments` inputs, each defaulting as the action's does, joined with single spaces, since the action
/// passes them on unquoted.
fn cargo_deny_action_args(step: &WorkflowStep) -> String {
    let arguments = step.input("arguments");
    let command = step.input("command");
    let command_arguments = step.input("command-arguments");
    [
        arguments.as_deref().unwrap_or("--all-features"),
        command.as_deref().unwrap_or("check"),
        command_arguments.as_deref().unwrap_or_default(),
    ]
    .iter()
    .flat_map(|input| input.split_whitespace())
    .collect::<Vec<_>>()
    .join(" ")
}

/// Whether `workflow` runs cargo-deny through [`CARGO_DENY_ACTION`] with the arguments `expected`. A step passing
/// others gives the issue `other` returns for its arguments, as [`cargo_deny_action_args`] gives them; the least
/// severe of these decides, since one step with the right arguments is enough.
fn cargo_deny_action_in(
    workflow: &Workflow,
    expected: &str,
    other: impl Fn(&str) -> Issue,
) -> Result<(), Issue> {
    let mut issues = Vec::new();
    for step in workflow.steps_using(CARGO_DENY_ACTION) {
        let args = cargo_deny_action_args(step);
        if args == expected {
            return Ok(());
        }
        issues.push(other(&args));
    }
    if issues.is_empty() {
        Err(workflow.missing_action(CARGO_DENY_ACTION, "cargo deny", runs_cargo_deny))
    } else {
        issues.dedup();
        Issue::alternatives(issues)
    }
}

/// The warning for a [`CARGO_DENY_ACTION`] step passing `args` rather than the standard's `expected`: it works, but
/// differs from the standard.
fn cargo_deny_args_differ(args: &str, expected: &str) -> Issue {
    Issue::warn(format!(
        "cargo-deny-action runs cargo deny {args} rather than {expected}"
    ))
}

/// Whether `workflow` runs cargo-deny's checks through [`CARGO_DENY_ACTION`] with the arguments the standard gives,
/// [`CARGO_DENY_CI_ARGS`]. A step passing others works, but differs from the standard, so it warns.
fn cargo_deny_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    cargo_deny_action_in(workflow, CARGO_DENY_CI_ARGS, |args| {
        cargo_deny_args_differ(args, CARGO_DENY_CI_ARGS)
    })
}

/// Whether `workflow` checks formatting with [`RUSTFMT_ACTION`]. With no such step, a step running rustfmt itself
/// warns.
fn rustfmt_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    if workflow.uses_action(RUSTFMT_ACTION) {
        Ok(())
    } else {
        Err(workflow.missing_action(RUSTFMT_ACTION, "rustfmt", runs_rustfmt))
    }
}

/// Whether a command's words run the Cargo subcommand `subcommand`, through `cargo` or as the command
/// `cargo-<subcommand>` itself. A command that only names it, such as `cargo install cargo-deny`, doesn't run it.
fn runs_cargo_subcommand(words: &[&str], subcommand: &str) -> bool {
    cargo_subcommand(words).is_some_and(|(name, _)| name == subcommand)
        || words
            .first()
            .is_some_and(|first| is_program(first, &format!("cargo-{subcommand}")))
}

/// Whether a command's words run rustfmt, through `cargo fmt` or as the command itself. A command that only names
/// rustfmt, such as `rustup component add rustfmt`, doesn't run it.
fn runs_rustfmt(words: &[&str]) -> bool {
    runs_cargo_subcommand(words, "fmt")
        || words
            .first()
            .is_some_and(|first| is_program(first, "rustfmt"))
}

/// Whether a command's words run rustfmt, as [`runs_rustfmt`] tells, and if so how: with `--check`, which fails on
/// unformatted files instead of rewriting them, or without it. `cargo fmt` takes the flag itself or passes it on to
/// rustfmt after `--`.
fn rustfmt_run(words: &[&str]) -> Option<FormatterRun> {
    runs_rustfmt(words).then(|| FormatterRun::from_args(words, &["--check"], &[]))
}

/// Whether a command's words run cargo-deny, through `cargo deny` or as the command itself.
fn runs_cargo_deny(words: &[&str]) -> bool {
    runs_cargo_subcommand(words, "deny")
}

/// The options `cargo clippy` takes, before `--`, in the standard's command, so that it lints every package, target
/// and feature.
const CLIPPY_OPTIONS: [&str; 3] = ["--workspace", "--all-targets", "--all-features"];

/// Whether `args`, the arguments `cargo clippy` passes on to Clippy after `--`, deny warnings: `-D warnings`, as the
/// standard gives it, or another spelling of it.
fn denies_warnings(args: &[&str]) -> bool {
    args.iter()
        .any(|arg| matches!(*arg, "-Dwarnings" | "--deny=warnings"))
        || args
            .windows(2)
            .any(|pair| matches!(pair, ["-D" | "--deny", "warnings"]))
}

/// Whether a command's words run Clippy through `cargo clippy`, and if so, its issue: a failure if it doesn't deny
/// warnings, since the lints table sets every lint to `warn`, so Clippy then passes whatever it finds, and a warning
/// if it lacks one of [`CLIPPY_OPTIONS`], so that it lints less than the whole workspace.
fn clippy_run(words: &[&str]) -> Option<Result<(), Issue>> {
    let (subcommand, args) = cargo_subcommand(words)?;
    if subcommand != "clippy" {
        return None;
    }
    let (options, clippy_args) =
        args.iter()
            .position(|arg| *arg == "--")
            .map_or((args.as_slice(), &[][..]), |split| {
                (
                    args.get(..split).unwrap_or_default(),
                    args.get(split + 1..).unwrap_or_default(),
                )
            });
    if !denies_warnings(clippy_args) {
        return Some(Err(Issue::fail(
            "cargo clippy runs without -D warnings, so it passes on every lint, which the lints table sets to warn",
        )));
    }
    let missing: Vec<&str> = CLIPPY_OPTIONS
        .into_iter()
        .filter(|option| !options.contains(option))
        .collect();
    Some(if missing.is_empty() {
        Ok(())
    } else {
        Err(Issue::warn(format!(
            "cargo clippy runs without {}",
            missing.join(", ")
        )))
    })
}

/// Check that each `clippy` hook runs `cargo clippy` as [`clippy_run`] requires. A hook that runs it nowhere norms can
/// see warns, as does one without an entry of its own, as a remote hook may leave it to the hook's repository, since
/// norms can't see the arguments it passes.
fn clippy_hooks(config: &PrecommitConfig) -> Result<(), Issue> {
    let issues = config.commit_hooks("clippy")?.into_iter().map(|hook| {
        if hook.entry.is_none() {
            return Err(Issue::warn(
                "The clippy hook sets no entry, so norms can't see whether it denies warnings",
            ));
        }
        let runs = hook.filter_map_commands("cargo clippy", clippy_run);
        if runs.is_empty() {
            Err(Issue::warn("The clippy hook doesn't run cargo clippy"))
        } else if runs.iter().any(Result::is_ok) {
            Ok(())
        } else {
            Issue::alternatives(runs.into_iter().filter_map(Result::err))
        }
    });
    Issue::combine(issues.filter_map(Result::err))
}

/// Check that some step runs `cargo clippy` as [`clippy_run`] requires. Warnings denied through `RUSTFLAGS` rather than
/// `-D warnings`, in any step's environment, still fail Clippy, but differ from the standard, so they warn instead.
fn clippy_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let runs = workflow.filter_map_commands(clippy_run);
    if runs.is_empty() {
        Err(Issue::fail("No step runs cargo clippy"))
    } else if runs.iter().any(Result::is_ok) {
        Ok(())
    } else if rustflags_deny_warnings(workflow) {
        Err(Issue::warn(
            "cargo clippy denies warnings through RUSTFLAGS rather than -D warnings",
        ))
    } else {
        Issue::alternatives(runs.into_iter().filter_map(Result::err))
    }
}

/// Whether the environment of some step of `workflow` sets `RUSTFLAGS` to deny warnings.
fn rustflags_deny_warnings(workflow: &Workflow) -> bool {
    workflow.jobs.values().any(|job| {
        job.steps.iter().any(|step| {
            workflow
                .env(job, step, "RUSTFLAGS")
                .is_some_and(|flags| denies_warnings(&flags.split_whitespace().collect::<Vec<_>>()))
        })
    })
}

/// The arguments a command's words pass to cargo-deny, if they run it, as [`runs_cargo_deny`] tells, joined with single
/// spaces and without `--manifest-path`, which names the component in a subdirectory rather than changing the checks.
fn cargo_deny_args(words: &[&str]) -> Option<String> {
    if !runs_cargo_deny(words) {
        return None;
    }
    let start = words
        .iter()
        .position(|word| *word == "deny" || is_program(word, "cargo-deny"))?;
    let mut args = Vec::new();
    let mut rest = words.get(start + 1..).unwrap_or_default().iter();
    while let Some(&arg) = rest.next() {
        if arg == "--manifest-path" {
            rest.next();
        } else if !arg.starts_with("--manifest-path=") {
            args.push(arg);
        }
    }
    Some(args.join(" "))
}

/// Check that each `cargo-deny` hook runs cargo-deny with the arguments the standard gives, [`CARGO_DENY_CI_ARGS`],
/// apart from `--manifest-path`. A hook passing others, or running it nowhere norms can see, warns.
fn cargo_deny_hooks(config: &PrecommitConfig) -> Result<(), Issue> {
    let issues = config.commit_hooks("cargo-deny")?.into_iter().map(|hook| {
        let runs = hook.filter_map_commands("cargo deny", cargo_deny_args);
        if runs.is_empty() {
            Err(Issue::warn("The cargo-deny hook doesn't run cargo deny"))
        } else if runs.iter().any(|args| args == CARGO_DENY_CI_ARGS) {
            Ok(())
        } else {
            Issue::alternatives(runs.into_iter().map(|args| {
                let args = if args.is_empty() {
                    "without arguments".to_owned()
                } else {
                    args
                };
                Issue::warn(format!(
                    "The cargo-deny hook runs cargo deny {args} rather than {CARGO_DENY_CI_ARGS}"
                ))
            }))
        }
    });
    Issue::combine(issues.filter_map(Result::err))
}

/// The actions that build no Rust code.
const NON_BUILDING_ACTIONS: [&str; 5] = [
    "actions/checkout",
    RUSTFMT_ACTION,
    "actions-rust-lang/setup-rust-toolchain",
    CARGO_DENY_ACTION,
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

/// Whether cargo-deny's arguments, as [`cargo_deny_action_args`] gives them, run its advisories check: `check`, naming
/// either no checks or `advisories` among them.
fn cargo_deny_checks_advisories(args: &str) -> bool {
    let mut words = args.split_whitespace().skip_while(|word| *word != "check");
    if words.next().is_none() {
        return false;
    }
    let checks: Vec<&str> = words
        .filter(|word| CARGO_DENY_CHECKS.contains(word))
        .collect();
    checks.is_empty() || checks.contains(&"advisories")
}

/// Whether `workflow` runs cargo-deny's advisories check through [`CARGO_DENY_ACTION`] with the arguments the standard
/// gives, [`CARGO_DENY_AUDIT_ARGS`]. A step passing others warns if it still runs the advisories check, and fails if
/// it doesn't.
fn cargo_deny_advisories_in(workflow: &Workflow) -> Result<(), Issue> {
    cargo_deny_action_in(workflow, CARGO_DENY_AUDIT_ARGS, |args| {
        if cargo_deny_checks_advisories(args) {
            cargo_deny_args_differ(args, CARGO_DENY_AUDIT_ARGS)
        } else {
            Issue::fail(format!(
                "cargo-deny-action runs cargo deny {args}, which skips the advisories check"
            ))
        }
    })
}

/// Check that `audit.yaml` runs cargo-deny's advisories check with the standard's arguments.
pub(crate) fn check_audit(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.audit_workflow() else {
        return;
    };

    report.check(
        "rust-audit-cargo-deny",
        "Rust audit: cargo deny advisories",
        cargo_deny_advisories_in(workflow),
    );
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::{Component, Status};
    use crate::report::Severity;

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
    fn deny_toml_is_found_as_cargo_deny_finds_it() {
        // The deny-toml-exists result for a component in `rust/`, given the files in the project.
        let exists = |files: &[&str]| {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("project");
            for file in files {
                let path = root.join(file);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, "[bans]\nmultiple-versions = \"allow\"\n").unwrap();
            }
            let component = Component {
                language: Language::Rust,
                path: PathBuf::from("rust"),
            };
            let project = ProjectContext::new(root.clone());
            let mut report = Report::new(root, Vec::new());
            check_cargo_deny(&ComponentContext::new(&project, &component), &mut report);
            let result = report.results.first().unwrap();
            assert_eq!(result.id, "deny-toml-exists");
            (result.name.clone(), result.status)
        };
        let exists_as = |name: &str, status| (format!("{name}: exists"), status);

        assert_eq!(exists(&[]), exists_as("deny.toml", Status::Fail));
        assert_eq!(
            exists(&["rust/deny.toml", "deny.toml"]),
            exists_as("deny.toml", Status::Pass)
        );
        assert_eq!(
            exists(&["deny.toml"]),
            exists_as("../deny.toml", Status::Pass)
        );
        // cargo-deny tries deny.toml first within a directory, but any name in a nearer directory.
        assert_eq!(
            exists(&["rust/deny.toml", "rust/.deny.toml", "rust/.cargo/deny.toml"]),
            exists_as("deny.toml", Status::Pass)
        );
        assert_eq!(
            exists(&["rust/.deny.toml", "rust/.cargo/deny.toml", "deny.toml"]),
            exists_as(".deny.toml", Status::Warn)
        );
        assert_eq!(
            exists(&["rust/.cargo/deny.toml", "deny.toml"]),
            exists_as(".cargo/deny.toml", Status::Warn)
        );
        assert_eq!(
            exists(&[".cargo/deny.toml"]),
            exists_as("../.cargo/deny.toml", Status::Warn)
        );
        // Neither a file below the component nor one above the project root applies.
        assert_eq!(
            exists(&["rust/sub/deny.toml", "../deny.toml"]),
            exists_as("deny.toml", Status::Fail)
        );
    }

    #[test]
    fn deny_toml_allows_multiple_versions_bans_std_replacements_and_explains_ignores() {
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
            "deny-toml-std-replacements",
            "deny-toml-ignore-reasons",
        ];
        let with = |statuses: [Status; 5]| {
            ids.iter()
                .map(|id| (*id).to_owned())
                .zip(statuses)
                .collect::<Vec<_>>()
        };

        assert_eq!(
            results("[bans]\nmultiple-versions = \"allow\"\nstd-replacements = {}\n"),
            with([Status::Pass; 5])
        );
        assert_eq!(
            results(
                "[advisories]\nignore = [\"RUSTSEC-2024-0001\"]\n\n\
                 [bans]\nmultiple-versions = \"allow\"\nstd-replacements = {}\n"
            ),
            with([
                Status::Pass,
                Status::Pass,
                Status::Pass,
                Status::Pass,
                Status::Warn
            ])
        );
        assert_eq!(
            results("[bans]\nmultiple-versions = \"deny\"\n"),
            with([
                Status::Pass,
                Status::Pass,
                Status::Warn,
                Status::Warn,
                Status::Pass
            ])
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
        ] {
            assert!(!cargo_deny_checks_advisories(args), "{args}");
        }
    }

    #[test]
    fn audit_runs_cargo_deny_advisories() {
        let audit = |steps: &str| {
            let yaml = format!("jobs:\n  audit:\n    steps:\n{steps}");
            cargo_deny_advisories_in(&serde_saphyr::from_str(&yaml).expect("workflow parses"))
        };
        let action = "      - uses: EmbarkStudios/cargo-deny-action@abc # v2.1.1\n";
        let with = |inputs: &str| format!("{action}        with:\n{inputs}");
        for steps in [
            with(
                "          arguments: --workspace --all-features\n          command: check -D warnings advisories\n          manifest-path: rust/Cargo.toml\n",
            ),
            // The action joins its inputs, so the arguments may be split between them.
            with(
                "          arguments: --workspace --all-features\n          command: check\n          command-arguments: -D warnings advisories\n",
            ),
            // One step with the standard's arguments is enough.
            format!(
                "{}{}",
                with("          command: check bans\n"),
                with(
                    "          arguments: --workspace --all-features\n          command: check -D warnings advisories\n"
                )
            ),
        ] {
            assert_eq!(audit(&steps), Ok(()), "{steps}");
        }
        // A step running the advisories check with other arguments works, but differs from the standard.
        for (steps, args) in [
            (action.to_owned(), "--all-features check"),
            (
                with("          command: check advisories\n"),
                "--all-features check advisories",
            ),
            (
                with(
                    "          arguments: --workspace --all-features\n          command: check -D warnings advisories bans\n",
                ),
                "--workspace --all-features check -D warnings advisories bans",
            ),
        ] {
            assert_eq!(
                audit(&steps),
                Err(Issue::warn(format!(
                    "cargo-deny-action runs cargo deny {args} rather than \
                     --workspace --all-features check -D warnings advisories"
                ))),
                "{steps}"
            );
        }
        // The step that runs the advisories check decides.
        assert_eq!(
            audit(&format!(
                "{}{action}",
                with("          command: check\n          command-arguments: bans\n")
            )),
            Err(Issue::warn(
                "cargo-deny-action runs cargo deny --all-features check bans, which skips the advisories check; \
                 cargo-deny-action runs cargo deny --all-features check rather than \
                 --workspace --all-features check -D warnings advisories"
            ))
        );
        assert_eq!(
            audit(&with(
                "          command: check\n          command-arguments: bans licenses\n"
            )),
            Err(Issue::fail(
                "cargo-deny-action runs cargo deny --all-features check bans licenses, \
                 which skips the advisories check"
            ))
        );
        // Running cargo-deny itself works, but isn't the template's form, so its arguments aren't read.
        for run in [
            "cargo deny --workspace check -D warnings advisories",
            "cargo deny check bans",
            "~/.cargo/bin/cargo-deny check",
        ] {
            assert_eq!(
                audit(&format!("      - run: {run}\n")),
                Err(Issue::warn(
                    "A step runs cargo deny rather than using EmbarkStudios/cargo-deny-action"
                )),
                "{run}"
            );
        }
        assert_eq!(
            audit("      - run: cargo build\n"),
            Err(Issue::fail("No step uses EmbarkStudios/cargo-deny-action"))
        );
    }

    #[test]
    fn ci_runs_cargo_deny_with_the_standard_arguments() {
        let ci = |steps: &str| {
            let yaml = format!("jobs:\n  cargo-deny:\n    steps:\n{steps}");
            cargo_deny_in_ci(&serde_saphyr::from_str(&yaml).expect("workflow parses"))
        };
        let action = "      - uses: EmbarkStudios/cargo-deny-action@abc # v2.1.1\n";
        let with = |inputs: &str| format!("{action}        with:\n{inputs}");
        for steps in [
            with(
                "          arguments: --workspace --all-features\n          command: check -D warnings bans licenses sources\n          manifest-path: rust/Cargo.toml\n",
            ),
            with(
                "          arguments: --workspace  --all-features\n          command: |\n            check -D warnings\n            bans licenses sources\n",
            ),
            // The action joins its inputs, so the arguments may be split between them.
            with(
                "          arguments: --workspace --all-features\n          command: check\n          command-arguments: -D warnings bans licenses sources\n",
            ),
            // One step with the standard's arguments is enough.
            format!(
                "{action}{}",
                with(
                    "          arguments: --workspace --all-features\n          command: check -D warnings bans licenses sources\n"
                )
            ),
        ] {
            assert_eq!(ci(&steps), Ok(()), "{steps}");
        }
        for (steps, args) in [
            (action.to_owned(), "--all-features check"),
            (
                with(
                    "          arguments: --workspace --all-features\n          command: check\n          command-arguments: -D warnings bans licenses sources advisories\n",
                ),
                "--workspace --all-features check -D warnings bans licenses sources advisories",
            ),
            (
                with(
                    "          arguments: --all-features\n          command: check -D warnings bans licenses sources\n",
                ),
                "--all-features check -D warnings bans licenses sources",
            ),
        ] {
            assert_eq!(
                ci(&steps),
                Err(Issue::warn(format!(
                    "cargo-deny-action runs cargo deny {args} rather than \
                     --workspace --all-features check -D warnings bans licenses sources"
                ))),
                "{steps}"
            );
        }
        assert_eq!(
            ci(
                "      - run: cargo deny --workspace --all-features check -D warnings bans licenses sources\n"
            ),
            Err(Issue::warn(
                "A step runs cargo deny rather than using EmbarkStudios/cargo-deny-action"
            ))
        );
        // rustfmt's own run doesn't count as cargo-deny's.
        assert_eq!(
            ci("      - run: cargo fmt --all --check\n"),
            Err(Issue::fail("No step uses EmbarkStudios/cargo-deny-action"))
        );
    }

    #[test]
    fn edition_is_the_latest() {
        assert_eq!(check_edition(Some("2024")), Ok(()));
        assert_eq!(
            check_edition(Some("2021")),
            Err("edition is 2021, not 2024".to_owned())
        );
        assert_eq!(check_edition(None), Err("Missing edition".to_owned()));

        // The template's packages use the edition the check requires.
        let template = include_str!("../../template/{{ rust_dir }}/Cargo.toml.jinja");
        assert!(
            template
                .lines()
                .any(|line| line == format!("edition = \"{RUST_EDITION}\"")),
            "{template}"
        );
    }

    #[test]
    fn rustfmt_in_ci_needs_the_action() {
        let ci = |steps: &str| {
            let yaml = format!("jobs:\n  rustfmt:\n    steps:\n{steps}");
            rustfmt_in_ci(&serde_saphyr::from_str(&yaml).expect("workflow parses"))
        };
        assert_eq!(
            ci("      - uses: actions-rust-lang/rustfmt@abc # v1.1.2\n"),
            Ok(())
        );
        // Running rustfmt itself works, but isn't the template's form, so its arguments aren't read: a run that
        // rewrites files rather than failing on them warns too.
        for run in ["cargo fmt --all --check", "cargo fmt --all"] {
            assert_eq!(
                ci(&format!("      - run: {run}\n")),
                Err(Issue::warn(
                    "A step runs rustfmt rather than using actions-rust-lang/rustfmt"
                )),
                "{run}"
            );
        }
        // cargo-deny's own run doesn't count as rustfmt's.
        assert_eq!(
            ci("      - run: cargo deny check\n"),
            Err(Issue::fail("No step uses actions-rust-lang/rustfmt"))
        );
    }

    #[test]
    fn rustfmt_runs_through_cargo_fmt_or_itself() {
        let runs = |line: &str| runs_rustfmt(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "cargo fmt --all --check",
            "cargo +nightly fmt",
            "rustfmt --check src/main.rs",
            "~/.cargo/bin/rustfmt src/main.rs",
            "cargo-fmt --check",
        ] {
            assert!(runs(line), "{line}");
        }
        for line in [
            "cargo clippy --check",
            "echo cargo-fmt --check",
            "rustup component add rustfmt",
        ] {
            assert!(!runs(line), "{line}");
        }
    }

    #[test]
    fn rustfmt_checks_with_the_check_flag() {
        let run = |line: &str| rustfmt_run(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "cargo fmt --all --check",
            "cargo fmt --all -- --check",
            "rustfmt --check src/main.rs",
        ] {
            assert_eq!(run(line), Some(FormatterRun::Checks), "{line}");
        }
        assert_eq!(run("cargo fmt --all"), Some(FormatterRun::Rewrites));
        assert_eq!(run("cargo clippy --check"), None);
    }

    #[test]
    fn cargo_deny_runs_through_cargo_or_itself() {
        let runs = |line: &str| runs_cargo_deny(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "cargo deny check",
            "cargo-deny check bans",
            "/usr/bin/cargo-deny",
        ] {
            assert!(runs(line), "{line}");
        }
        for line in ["cargo install cargo-deny", "cargo denylist"] {
            assert!(!runs(line), "{line}");
        }
    }

    #[test]
    fn clippy_must_deny_warnings_across_the_workspace() {
        let run = |line: &str| {
            let words: Vec<&str> = line.split_whitespace().collect();
            clippy_run(&words)
        };
        for line in [
            "cargo clippy --workspace --all-targets --all-features -- -D warnings",
            "cargo +1.99.1 clippy --manifest-path rust/Cargo.toml --workspace --all-targets --all-features -- -Dwarnings",
            "cargo clippy --all-features --all-targets --workspace -- --deny warnings",
            "cargo clippy --workspace --all-targets --all-features -- --deny=warnings -W clippy::pedantic",
        ] {
            assert_eq!(run(line), Some(Ok(())), "{line}");
        }
        // Without -D warnings Clippy passes on every lint the lints table sets to warn, whatever else it passes.
        for line in [
            "cargo clippy --workspace --all-targets --all-features",
            "cargo clippy --workspace --all-targets --all-features -D warnings",
            "cargo clippy --workspace --all-targets --all-features -- -W warnings",
            "cargo clippy --workspace --all-targets --all-features -- -D clippy::all",
        ] {
            assert_eq!(
                run(line).map(|result| result.map_err(|issue| issue.severity)),
                Some(Err(Severity::Fail)),
                "{line}"
            );
        }
        assert_eq!(
            run("cargo clippy --workspace -- -D warnings"),
            Some(Err(Issue::warn(
                "cargo clippy runs without --all-targets, --all-features"
            )))
        );
        for line in [
            "cargo test --workspace",
            "cargo install clippy",
            "echo clippy",
        ] {
            assert_eq!(run(line), None, "{line}");
        }
    }

    #[test]
    fn clippy_hooks_and_ci_deny_warnings() {
        let hooks = |entries: &[&str]| {
            let hooks = entries
                .iter()
                .map(|entry| format!("      - id: clippy\n        entry: {entry}\n"))
                .collect::<Vec<_>>()
                .concat();
            let yaml = format!(
                "default_stages: [pre-commit]\nrepos:\n  - repo: local\n    hooks:\n{hooks}"
            );
            clippy_hooks(&serde_saphyr::from_str(&yaml).expect("config parses"))
        };
        let standard = "cargo clippy --workspace --all-targets --all-features -- -D warnings";
        assert_eq!(hooks(&[standard]), Ok(()));
        // Every copy of the hook must deny warnings.
        assert_eq!(
            hooks(&[
                standard,
                "cargo clippy --workspace --all-targets --all-features"
            ])
            .map_err(|issue| issue.severity),
            Err(Severity::Fail)
        );
        assert_eq!(
            hooks(&["cargo check"]),
            Err(Issue::warn("The clippy hook doesn't run cargo clippy"))
        );
        // A hook without an entry runs its repository's, whose arguments norms can't see.
        assert_eq!(
            clippy_hooks(
                &serde_saphyr::from_str(
                    "default_stages: [pre-commit]\nrepos:\n  - repo: https://example.com/hooks\n    rev: v1.0.0\n    hooks:\n      - id: clippy\n"
                )
                .expect("config parses")
            ),
            Err(Issue::warn(
                "The clippy hook sets no entry, so norms can't see whether it denies warnings"
            ))
        );

        let ci = |runs: &[&str]| {
            let steps = runs
                .iter()
                .map(|run| format!("      - run: {run}\n"))
                .collect::<Vec<_>>()
                .concat();
            let yaml = format!("jobs:\n  clippy:\n    steps:\n{steps}");
            clippy_in_ci(&serde_saphyr::from_str(&yaml).expect("workflow parses"))
        };
        // A step that denies warnings decides.
        assert_eq!(ci(&["cargo clippy", standard]), Ok(()));
        assert_eq!(
            ci(&["cargo clippy --workspace --all-targets --all-features"])
                .map_err(|issue| issue.severity),
            Err(Severity::Fail)
        );
        assert_eq!(
            ci(&["cargo test"]),
            Err(Issue::fail("No step runs cargo clippy"))
        );
        // A run that only lacks some options warns.
        assert_eq!(
            ci(&["cargo clippy --workspace -- -D warnings"]).map_err(|issue| issue.severity),
            Err(Severity::Warn)
        );
        // Warnings denied through RUSTFLAGS still fail Clippy, but aren't the standard's form.
        let yaml = "jobs:\n  clippy:\n    env:\n      RUSTFLAGS: -Dwarnings\n    steps:\n      - run: cargo clippy --workspace --all-targets --all-features\n";
        assert_eq!(
            clippy_in_ci(&serde_saphyr::from_str(yaml).expect("workflow parses")),
            Err(Issue::warn(
                "cargo clippy denies warnings through RUSTFLAGS rather than -D warnings"
            ))
        );
    }

    #[test]
    fn cargo_deny_hooks_pass_the_standard_arguments() {
        let hook = |entry: &str| {
            let yaml = format!(
                "default_stages: [pre-commit]\nrepos:\n  - repo: local\n    hooks:\n      - id: cargo-deny\n        entry: {entry}\n"
            );
            cargo_deny_hooks(&serde_saphyr::from_str(&yaml).expect("config parses"))
        };
        for entry in [
            "cargo deny --workspace --all-features check -D warnings bans licenses sources",
            "cargo deny --manifest-path rust/Cargo.toml --workspace --all-features check -D warnings bans licenses sources",
            "cargo-deny --manifest-path=rust/Cargo.toml --workspace --all-features check -D warnings bans licenses sources",
        ] {
            assert_eq!(hook(entry), Ok(()), "{entry}");
        }
        assert_eq!(
            hook("cargo deny --workspace --all-features check bans licenses sources"),
            Err(Issue::warn(
                "The cargo-deny hook runs cargo deny --workspace --all-features check bans licenses sources rather than \
                 --workspace --all-features check -D warnings bans licenses sources"
            ))
        );
        assert_eq!(
            hook("cargo deny"),
            Err(Issue::warn(
                "The cargo-deny hook runs cargo deny without arguments rather than \
                 --workspace --all-features check -D warnings bans licenses sources"
            ))
        );
        assert_eq!(
            hook("cargo install cargo-deny"),
            Err(Issue::warn("The cargo-deny hook doesn't run cargo deny"))
        );
    }

    #[test]
    fn lints_are_set_at_least_to_their_level() {
        let lints: HashMap<String, toml::Value> = toml::from_str(
            "pedantic = { level = \"warn\", priority = -1 }\nnursery = \"allow\"\nunsafe_code = \"deny\"\nunused = 1\n",
        )
        .unwrap();
        assert_eq!(lint_at_least(&lints, "pedantic", LintLevel::Warn), Ok(()));
        assert_eq!(
            lint_at_least(&lints, "nursery", LintLevel::Warn),
            Err("nursery is allow, not warn or stricter".to_owned())
        );
        assert_eq!(
            lint_at_least(&lints, "unsafe_code", LintLevel::Forbid),
            Err("unsafe_code is deny, not forbid".to_owned())
        );
        assert_eq!(
            lint_at_least(&lints, "cargo", LintLevel::Warn),
            Err("cargo isn't set".to_owned())
        );

        // A package's lints are reported together, every compiler lint's issue in one result.
        let manifest: CargoToml = toml::from_str(
            "[package]\nname = \"x\"\n\n[lints.clippy]\npedantic = \"warn\"\nnursery = \"allow\"\ncargo = \"deny\"\n\n[lints.rust]\nfuture_incompatible = \"warn\"\nlet_underscore = \"warn\"\nunsafe_code = \"deny\"\nunused_qualifications = \"warn\"\n",
        )
        .unwrap();
        let mut report = Report::new(PathBuf::new(), Vec::new());
        check_package_lints("Cargo.toml", &manifest, None, &mut report);
        let results: Vec<_> = report
            .results
            .iter()
            .map(|result| (result.id.as_str(), result.status, result.message.as_str()))
            .collect();
        assert_eq!(
            results,
            [
                ("clippy-lints", Status::Pass, ""),
                ("clippy-pedantic", Status::Pass, ""),
                (
                    "clippy-nursery",
                    Status::Warn,
                    "nursery is allow, not warn or stricter"
                ),
                ("clippy-cargo", Status::Pass, ""),
                (
                    "rustc-lints",
                    Status::Warn,
                    "unreachable_pub isn't set; unsafe_code is deny, not forbid"
                ),
            ]
        );
        assert_eq!(
            lint_at_least(&lints, "unused", LintLevel::Warn),
            Err("unused has no level norms recognizes".to_owned())
        );

        // The template's lints meet the standard.
        let template: CargoToml = toml::from_str(
            &include_str!("../../template/{{ rust_dir }}/Cargo.toml.jinja")
                .lines()
                .filter(|line| !line.contains("{{") && !line.contains("{%"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        let template_lints = template.lints.unwrap();
        let clippy = template_lints.clippy.unwrap();
        for (_, group) in CLIPPY_GROUPS {
            assert_eq!(
                lint_at_least(&clippy, group, LintLevel::Warn),
                Ok(()),
                "{group}"
            );
        }
        let rust = template_lints.rust.unwrap();
        for (lint, level) in RUSTC_LINTS {
            assert_eq!(lint_at_least(&rust, lint, level), Ok(()), "{lint}");
        }
    }
}
