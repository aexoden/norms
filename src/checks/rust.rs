// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Rust-specific checks.

use crate::config::{CargoConfig, CargoToml};
use crate::context::{
    ComponentContext, ConfigStatus, LEGACY_RUST_TOOLCHAIN, ProjectContext, RUST_TOOLCHAIN,
};
use crate::models::Language;
use crate::report::{Outcome, Report, file_exists};

/// Whether the component is an application rather than a library: whether it builds a binary, from an explicit
/// `[[bin]]` target or, unless `autobins = false`, `src/main.rs` or `src/bin/`. A package with both a library and a
/// binary counts as an application.
///
/// Only the component's own manifest counts, so the members of a Cargo workspace aren't considered.
fn is_application(ctx: &ComponentContext, config: &CargoToml) -> bool {
    let autobins = config
        .package
        .as_ref()
        .and_then(|p| p.autobins)
        .unwrap_or(true);

    config.has_explicit_bin_targets()
        || (autobins
            && (ctx.path().join("src/main.rs").is_file() || ctx.path().join("src/bin").is_dir()))
}

//
// Project configuration
//

pub(crate) fn check_cargo(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("cargo-toml", "Cargo.toml", ctx.cargo()) else {
        return;
    };

    //  Package metadata
    let package = config.metadata(config.workspace.as_ref());

    report.should(
        "cargo-toml-license",
        "Cargo.toml: has license",
        package.license.is_some(),
    );

    if let Some(license) = package.license {
        report.should(
            "cargo-toml-license-texts",
            "Cargo.toml: license has its texts in LICENSES/",
            ctx.license_files().check_expression(license),
        );
    }

    report.should(
        "cargo-toml-description",
        "Cargo.toml: has description",
        package.description.is_some(),
    );

    report.should(
        "cargo-toml-repository",
        "Cargo.toml: has repository",
        package.repository.is_some(),
    );

    report.should(
        "cargo-toml-edition",
        "Cargo.toml: has edition",
        package.edition.is_some(),
    );
}

//
// Version pinning
//

pub(crate) fn check_cargo_dependencies(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    if is_application(ctx, config) {
        report.should(
            "cargo-toml-dependencies-exact",
            "Cargo.toml: dependencies pinned exactly (application)",
            config.check_dependencies_exact(),
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
    } else if let Some(toolchain) = report.recommend_parsed("rust-toolchain-toml", label, status) {
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

    let has_msrv = ctx.cargo().as_ref_ok().is_some_and(|config| {
        config
            .metadata(config.workspace.as_ref())
            .rust_version
            .is_some()
    });
    report.should(
        "cargo-toml-rust-version",
        "Rust: MSRV defined (rust-version)",
        has_msrv,
    );
}

//
// Clippy lints
//

pub(crate) fn check_clippy_lints(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.cargo().as_ref_ok() else {
        return;
    };

    let lints = match config.clippy_lints(config.workspace.as_ref()) {
        Ok(lints) => lints,
        Err(error) => {
            report.should(
                "clippy-lints",
                "Clippy: lints configured",
                Outcome::fail(error),
            );
            return;
        }
    };

    report.should("clippy-lints", "Clippy: lints configured", Outcome::Pass);
    report.should(
        "clippy-pedantic",
        "Clippy: pedantic enabled",
        lints.contains_key("pedantic"),
    );
}

//
// Dependency auditing
//

pub(crate) fn check_cargo_deny(ctx: &ComponentContext, report: &mut Report) {
    report.should(
        "deny-toml-exists",
        "Rust: deny.toml",
        file_exists(&ctx.path().join("deny.toml")),
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
        report.should(
            "cargo-lock-exists",
            "Rust: Cargo.lock comitted (binary)",
            file_exists(&ctx.path().join("Cargo.lock")),
        );
    }
}

//
// Release age
//

/// Check the release age in the root's `.cargo/config.toml`, which applies wherever Cargo runs in the repository, and
/// in the `.cargo/config.toml` of any Rust component in a subdirectory, which overrides it there.
pub(crate) fn check_cargo_config(ctx: &ProjectContext, report: &mut Report) {
    check_cargo_config_file(ctx.cargo_config(), report);

    for component in ctx
        .components()
        .iter()
        .filter(|c| c.language == Language::Rust && !c.path.as_os_str().is_empty())
    {
        let component_ctx = ComponentContext::new(ctx, component);
        if !matches!(component_ctx.cargo_config(), ConfigStatus::NotFound) {
            report.with_component(component, |report| {
                check_cargo_config_file(component_ctx.cargo_config(), report);
            });
        }
    }
}

fn check_cargo_config_file(status: &ConfigStatus<CargoConfig>, report: &mut Report) {
    let Some(config) = report.recommend_parsed("cargo-config", ".cargo/config.toml", status) else {
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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::Component;

    /// Whether a package with the given manifest and files is an application.
    fn is_application_with(manifest: &str, files: &[&str]) -> bool {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), manifest).unwrap();
        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "fn main() {}\n").unwrap();
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
        assert!(!is_application_with(
            "[workspace]\nmembers = [\"app\"]\n",
            &["app/src/main.rs"]
        ));
    }
}
