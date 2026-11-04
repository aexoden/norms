// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Rust-specific checks.

use crate::context::{ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

//
// Project configuration
//

pub(crate) fn check_cargo(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("cargo-toml", "Cargo.toml", ctx.cargo()) else {
        return;
    };

    //  Package metadata
    let package = config.package.as_ref();

    report.should(
        "cargo-toml-license",
        "Cargo.toml: has license",
        package.and_then(|p| p.license.as_ref()).is_some(),
    );

    report.should(
        "cargo-toml-description",
        "Cargo.toml: has description",
        package.and_then(|p| p.description.as_ref()).is_some(),
    );

    report.should(
        "cargo-toml-repository",
        "Cargo.toml: has repository",
        package.and_then(|p| p.repository.as_ref()).is_some(),
    );

    report.should(
        "cargo-toml-edition",
        "Cargo.toml: has edition",
        package.and_then(|p| p.edition.as_ref()).is_some(),
    );
}

//
// MSRV / Toolchain
//

pub(crate) fn check_rust_toolchain(ctx: &ProjectContext, report: &mut Report) {
    let package = ctx.cargo().as_ref_ok().and_then(|c| c.package.as_ref());

    let has_msrv = package.and_then(|p| p.rust_version.as_ref()).is_some();
    report.should(
        "cargo-toml-rust-version",
        "Rust: MSRV defined (rust-version)",
        has_msrv,
    );
}

//
// Clippy lints
//

pub(crate) fn check_clippy_lints(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    let has_clippy_lints = config.effective_clippy_lints().is_some();

    if !has_clippy_lints {
        report.should(
            "clippy-lints",
            "Clippy: lints configured",
            Outcome::fail("No [lints.clippy] or [workspace.lints.clippy] section"),
        );
        return;
    }

    report.should("clippy-lints", "Clippy: lints configured", Outcome::Pass);
    report.should(
        "clippy-pedantic",
        "Clippy: pedantic enabled",
        config.has_clippy_lint("pedantic"),
    );
}

//
// Dependency auditing
//

pub(crate) fn check_cargo_deny(ctx: &ProjectContext, report: &mut Report) {
    report.should(
        "deny-toml-exists",
        "Rust: deny.toml",
        file_exists(&ctx.path().join("deny.toml")),
    );
}

//
// Lock file
//

pub(crate) fn check_cargo_lock(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    // Determine if the project produces a binary
    let has_binary = config.has_explicit_bin_targets() || ctx.path().join("src/main.rs").exists();

    if has_binary {
        report.should(
            "cargo-lock-exists",
            "Rust: Cargo.lock comitted (binary)",
            file_exists(&ctx.path().join("Cargo.lock")),
        );
    }
}

//
// Pre-commit integration
//

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.should(
        "rust-precommit-rustfmt",
        "Rust pre-commit: rustfmt hook",
        config.has_hook("rustfmt"),
    );

    report.should(
        "rust-precommit-clippy",
        "Rust pre-commit: clippy hook",
        config.has_hook("clippy"),
    );

    report.should(
        "rust-precommit-cargo-deny",
        "Rust pre-commit: cargo-deny hook",
        config.has_hook("cargo-deny"),
    );
}

//
// CI integration
//

pub(crate) fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    report.should(
        "rust-ci-rustfmt",
        "Rust CI: format check",
        workflow.runs_command("cargo fmt") || workflow.uses_action("actions-rust-lang/rustfmt"),
    );
    report.should(
        "rust-ci-clippy",
        "Rust CI: clippy check",
        workflow.runs_command("cargo clippy"),
    );
    report.should(
        "rust-ci-test",
        "Rust CI: test",
        workflow.runs_command("cargo test"),
    );
    report.should(
        "rust-ci-cargo-deny",
        "Rust CI: cargo deny",
        workflow.runs_command("cargo deny")
            || workflow.uses_action("EmbarkStudios/cargo-deny-action"),
    );
}
