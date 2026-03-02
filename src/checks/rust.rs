// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Rust-specific checks.

use crate::context::{ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

//
// Project configuration
//

pub fn check_cargo(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("Cargo.toml", ctx.cargo()) else {
        return;
    };

    //  Package metadata
    let package = config.package.as_ref();

    report.should(
        "Cargo.toml: has license",
        package.and_then(|p| p.license.as_ref()).is_some(),
    );

    report.should(
        "Cargo.toml: has description",
        package.and_then(|p| p.description.as_ref()).is_some(),
    );

    report.should(
        "Cargo.toml: has repository",
        package.and_then(|p| p.repository.as_ref()).is_some(),
    );

    report.should(
        "Cargo.toml: has edition",
        package.and_then(|p| p.edition.as_ref()).is_some(),
    );
}

//
// MSRV / Toolchain
//

pub fn check_rust_toolchain(ctx: &ProjectContext, report: &mut Report) {
    let package = ctx.cargo().as_ref_ok().and_then(|c| c.package.as_ref());

    let has_msrv = package.and_then(|p| p.rust_version.as_ref()).is_some();
    report.should("Rust: MSRV defined (rust-version)", has_msrv);

    // rust-toolchain.toml pins the toolchain for contributors
    report.should(
        "Rust: rust-toolchain.toml",
        file_exists(&ctx.path().join("rust-toolchain.toml")),
    );
}

//
// Clippy lints
//

pub fn check_clippy_lints(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    let has_clippy_lints = config.effective_clippy_lints().is_some();

    if !has_clippy_lints {
        report.should(
            "Clippy: lints configured",
            Outcome::fail("No [lints.clippy] or [workspace.lints.clippy] section"),
        );
        return;
    }

    report.should("Clippy: lints configured", Outcome::Pass);
    report.should(
        "Clippy: pedantic enabled",
        config.has_clippy_lint("pedantic"),
    );
}

//
// Dependency auditing
//

pub fn check_cargo_deny(ctx: &ProjectContext, report: &mut Report) {
    report.should(
        "Rust: deny.toml",
        file_exists(&ctx.path().join("deny.toml")),
    );
}

//
// Lock file
//

pub fn check_cargo_lock(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    // Determine if the project produces a binary
    let has_binary = config.has_explicit_bin_targets() || ctx.path().join("src/main.rs").exists();

    if has_binary {
        report.should(
            "Rust: Cargo.lock comitted (binary)",
            file_exists(&ctx.path().join("Cargo.lock")),
        );
    }
}

//
// Pre-commit integration
//

pub fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.should("Rust pre-commit: rustfmt hook", config.has_hook("rustfmt"));

    report.should("Rust pre-commit: clippy hook", config.has_hook("clippy"));

    report.should(
        "Rust pre-commit: cargo-deny hook",
        config.has_hook("cargo-deny"),
    );
}

//
// Devbox integration
//

pub fn check_devbox_rust(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.devbox().as_ref_ok() else {
        return;
    };

    report.should("Rust devbox: includes rustup", config.has_package("rustup"));

    report.should(
        "Rust devbox: includes cargo-deny",
        config.has_package("cargo-deny"),
    );
}

//
// CI integration
//

pub fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(content) = ctx.ci_workflow() else {
        return;
    };

    let lower = content.to_lowercase();

    report.should("Rust CI: format check", lower.contains("cargo fmt"));
    report.should("Rust CI: clippy check", lower.contains("cargo clippy"));
    report.should("Rust CI: test", lower.contains("cargo test"));
    report.should("Rust CI: cargo deny", lower.contains("cargo deny"));
}
