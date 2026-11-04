// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Check modules and runner.
//!
//! Checks are plain functions with the signature:
//!
//! ```ignore
//! fn check_something(ctx: &ProjectContext, report: &mut Report) { ... }
//! ```
//!
//! Registration is explicit - no macros or global side effects. New checks
//! are added by writing the function and listing it in [`run_all_checks`].

pub(crate) mod common;
pub(crate) mod python;
pub(crate) mod rust;
pub(crate) mod typescript;

use crate::context::ProjectContext;
use crate::models::Language;
use crate::report::Report;

/// Run all applicable checks for the given project context.
///
/// Common checks always run. Language-specific checks run for each detected language, in a stable order. The project's
/// waivers apply last, once every check has been recorded.
pub(crate) fn run_all_checks(ctx: &ProjectContext, report: &mut Report) {
    // Common checks (apply to all projects)
    common::check_norms_config(ctx, report);
    common::check_readme(ctx, report);
    common::check_license(ctx, report);
    common::check_changelog(ctx, report);
    common::check_gitignore(ctx, report);
    common::check_gitattributes(ctx, report);
    common::check_editorconfig(ctx, report);
    common::check_devbox(ctx, report);
    common::check_precommit(ctx, report);
    common::check_renovate(ctx, report);
    common::check_github_actions(ctx, report);

    // Language-specific checks
    for &language in ctx.languages() {
        match language {
            Language::Cpp => {
                // Placeholder
            }
            Language::Python => {
                python::check_pyproject(ctx, report);
                python::check_src_layout(ctx, report);
                python::check_uv_lock(ctx, report);
                python::check_dependency_groups(ctx, report);
                python::check_ruff(ctx, report);
                python::check_ruff_lint(ctx, report);
                python::check_mypy(ctx, report);
                python::check_precommit_hooks(ctx, report);
                python::check_ci(ctx, report);
            }
            Language::Rust => {
                rust::check_cargo(ctx, report);
                rust::check_rust_toolchain(ctx, report);
                rust::check_clippy_lints(ctx, report);
                rust::check_cargo_deny(ctx, report);
                rust::check_cargo_lock(ctx, report);
                rust::check_precommit_hooks(ctx, report);
                rust::check_ci(ctx, report);
            }
            Language::TypeScript => {
                typescript::check_package_json(ctx, report);
                typescript::check_pnpm(ctx, report);
                typescript::check_tsconfig(ctx, report);
                typescript::check_eslint(ctx, report);
                typescript::check_prettier(ctx, report);
                typescript::check_scripts(ctx, report);
                typescript::check_precommit_hooks(ctx, report);
                typescript::check_ci(ctx, report);
            }
        }
    }

    if let Some(config) = ctx.norms().as_ref_ok() {
        report.apply_waivers(&config.waivers);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::PathBuf;

    use super::*;
    use crate::models::Status;
    use crate::report::{CheckResult, is_valid_check_id};

    /// A scratch project directory, removed when dropped.
    struct TempProject(PathBuf);

    impl TempProject {
        fn new(name: &str, norms_toml: Option<&str>) -> Self {
            let dir =
                std::env::temp_dir().join(format!("norms-test-{}-{name}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir).unwrap();
            }
            fs::create_dir_all(&dir).unwrap();

            if let Some(content) = norms_toml {
                fs::write(dir.join("norms.toml"), content).unwrap();
            }

            Self(dir)
        }

        fn verify(&self) -> Vec<CheckResult> {
            let ctx = ProjectContext::new(self.0.clone());
            let mut report = Report::new(self.0.clone(), ctx.languages().clone());
            run_all_checks(&ctx, &mut report);
            report.results
        }
    }

    impl Drop for TempProject {
        fn drop(&mut self) {
            drop(fs::remove_dir_all(&self.0));
        }
    }

    fn find<'a>(results: &'a [CheckResult], id: &str) -> Vec<&'a CheckResult> {
        results.iter().filter(|r| r.id == id).collect()
    }

    #[test]
    fn missing_norms_toml_records_nothing() {
        let results = TempProject::new("missing", None).verify();
        assert!(!results.iter().any(|r| r.id.starts_with("norms-toml-")));
    }

    #[test]
    fn invalid_norms_toml_fails_and_waives_nothing() {
        let results = TempProject::new(
            "invalid",
            Some("[components]\ntypescript = \"frontend\"\n\n[[waivers]]\ncheck = \"readme-exists\"\nreason = \"x\"\n"),
        )
        .verify();

        let valid = find(&results, "norms-toml-valid");
        assert!(matches!(valid.as_slice(), [r] if r.status == Status::Fail));
        assert!(find(&results, "norms-toml-waiver-needed").is_empty());
        assert!(
            find(&results, "readme-exists")
                .iter()
                .all(|r| r.status == Status::Fail)
        );
    }

    #[test]
    fn waivers_apply_after_every_check() {
        let results = TempProject::new(
            "waived",
            Some("[[waivers]]\ncheck = \"readme-exists\"\nreason = \"No readme.\"\n"),
        )
        .verify();

        let readme = find(&results, "readme-exists");
        assert!(matches!(
            readme.as_slice(),
            [r] if r.status == Status::Skip && r.message == "Waived: No readme."
        ));
        for id in [
            "norms-toml-exists",
            "norms-toml-valid",
            "norms-toml-waiver-needed",
        ] {
            assert!(
                find(&results, id).iter().all(|r| r.status == Status::Pass),
                "{id}"
            );
        }
    }

    /// Runs the checks against this repository, which exercises the common and Rust checks. Apart from the one result
    /// per waiver, no check here records more than one result, so a repeated ID means two checks share it.
    #[test]
    fn check_ids_are_valid_and_unique() {
        let ctx = ProjectContext::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let mut report = Report::new(ctx.path().to_path_buf(), ctx.languages().clone());
        run_all_checks(&ctx, &mut report);

        let mut seen = HashSet::new();
        for result in &report.results {
            assert!(is_valid_check_id(&result.id), "invalid ID: {}", result.id);
            assert!(
                result.id == "norms-toml-waiver-needed" || seen.insert(&result.id),
                "duplicate ID: {}",
                result.id
            );
        }
    }
}
