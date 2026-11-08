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

use crate::context::{ComponentContext, ProjectContext};
use crate::models::Language;
use crate::report::Report;

/// Run all applicable checks for the given project context.
///
/// Common checks always run. Language-specific checks run for each detected language, in a stable order: first the
/// checks of each component of that language, then the checks of the language's place in the repository's own files,
/// such as its pre-commit hooks. The project's waivers apply last, once every check has been recorded.
pub(crate) fn run_all_checks(ctx: &ProjectContext, report: &mut Report) {
    // Common checks (apply to all projects)
    common::check_norms_config(ctx, report);
    common::check_readme(ctx, report);
    common::check_licenses(ctx, report);
    common::check_changelog(ctx, report);
    common::check_agent_guidance(ctx, report);
    common::check_gitignore(ctx, report);
    common::check_gitattributes(ctx, report);
    common::check_editorconfig(ctx, report);
    common::check_markdownlint(ctx, report);
    common::check_devbox(ctx, report);
    common::check_precommit(ctx, report);
    common::check_renovate(ctx, report);
    common::check_github_actions(ctx, report);
    common::check_components(ctx, report);

    // Language-specific checks
    for &language in ctx.languages() {
        for component_ctx in ctx
            .component_contexts()
            .iter()
            .filter(|c| c.component().language == language)
        {
            report.with_component(component_ctx.component(), |report| {
                check_component(ctx, component_ctx, language, report);
            });
        }

        match language {
            Language::Cpp => {
                // Placeholder
            }
            Language::Python => {
                python::check_precommit_hooks(ctx, report);
                python::check_ci(ctx, report);
                python::check_audit(ctx, report);
            }
            Language::Rust => {
                rust::check_cargo_config(ctx, report);
                rust::check_precommit_hooks(ctx, report);
                rust::check_ci(ctx, report);
                rust::check_toolchain_cache(ctx, report);
                rust::check_audit(ctx, report);
            }
            Language::TypeScript => {
                typescript::check_precommit_hooks(ctx, report);
                typescript::check_ci(ctx, report);
                typescript::check_audit(ctx, report);
            }
        }
    }

    if let Some(config) = ctx.norms().as_ref_ok() {
        report.apply_waivers(&config.waivers);
    }
}

/// Run the checks of one component's own files, and of the repository-wide files that apply to it alone, such as the
/// CI job testing a library's oldest supported version.
fn check_component(
    project: &ProjectContext,
    ctx: &ComponentContext,
    language: Language,
    report: &mut Report,
) {
    match language {
        Language::Cpp => {
            // Placeholder
        }
        Language::Python => {
            python::check_pyproject(ctx, report);
            python::check_python_version(ctx, report);
            python::check_requires_python_pin(ctx, report);
            python::check_python_classifiers(ctx, report);
            python::check_versioning(ctx, report);
            python::check_git_archival(project, ctx, report);
            python::check_src_layout(ctx, report);
            python::check_uv_lock(ctx, report);
            python::check_dependencies_exact(ctx, report);
            python::check_uv_exclude_newer(ctx, report);
            python::check_uv_required_version(project, ctx, report);
            python::check_dependency_groups(ctx, report);
            python::check_ruff(ctx, report);
            python::check_ruff_lint(ctx, report);
            python::check_mypy(ctx, report);
            python::check_min_python_job(project, ctx, report);
        }
        Language::Rust => {
            rust::check_cargo(ctx, report);
            rust::check_cargo_workspace(ctx, report);
            rust::check_cargo_dependencies(ctx, report);
            rust::check_rust_toolchain(ctx, report);
            rust::check_rust_version(ctx, report);
            rust::check_clippy_lints(ctx, report);
            rust::check_clippy_toml(ctx, report);
            rust::check_cargo_deny(ctx, report);
            rust::check_cargo_lock(ctx, report);
            rust::check_msrv_job(project, ctx, report);
        }
        Language::TypeScript => {
            typescript::check_package_json(ctx, report);
            typescript::check_pnpm(ctx, report);
            typescript::check_pnpm_workspace(ctx, report);
            typescript::check_tsconfig(ctx, report);
            typescript::check_eslint(ctx, report);
            typescript::check_prettier(ctx, report);
            typescript::check_scripts(ctx, report);
        }
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
            let mut report = Report::new(self.0.clone(), ctx.components().to_vec());
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
    fn invalid_precommit_config_fails() {
        let project = TempProject::new("precommit", None);
        fs::write(project.0.join(".pre-commit-config.yaml"), "repos: [\n").unwrap();
        fs::write(project.0.join("Cargo.toml"), "[package]\nname = \"demo\"\n").unwrap();
        let results = project.verify();

        let valid = find(&results, "precommit-config-valid");
        assert!(matches!(valid.as_slice(), [r] if r.status == Status::Fail));
        assert!(
            !results
                .iter()
                .any(|r| r.id.starts_with("rust-precommit-") || r.id == "precommit-reuse")
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

    #[test]
    fn excluded_directories_hold_no_components() {
        let project = TempProject::new(
            "excluded",
            Some("[components]\nexclude = [\"tests/fixtures\", \"missing\", \"../up\"]\n"),
        );
        fs::create_dir_all(project.0.join("tests/fixtures/web")).unwrap();
        fs::write(project.0.join("tests/fixtures/web/package.json"), "{}").unwrap();
        fs::write(project.0.join("Cargo.toml"), "").unwrap();
        let results = project.verify();

        assert!(
            results
                .iter()
                .all(|r| r.component.as_deref().is_none_or(|c| c == "."))
        );
        assert!(!results.iter().any(|r| r.id.starts_with("package-json-")));

        let paths = find(&results, "norms-toml-exclude-paths");
        assert!(matches!(
            paths.as_slice(),
            [r] if r.status == Status::Fail && r.message == "Not a plain relative path: \"../up\""
        ));
        let exists: Vec<_> = find(&results, "norms-toml-exclude-exists")
            .iter()
            .map(|r| (r.name.as_str(), r.status))
            .collect();
        assert_eq!(
            exists,
            [
                (
                    "norms.toml: excluded directory tests/fixtures exists",
                    Status::Pass
                ),
                (
                    "norms.toml: excluded directory missing exists",
                    Status::Warn
                ),
            ]
        );
    }

    #[test]
    fn component_contexts_are_shared() {
        let project = TempProject::new("contexts", None);
        for component in ["api", "web"] {
            fs::create_dir_all(project.0.join(component)).unwrap();
        }
        fs::write(project.0.join("api/Cargo.toml"), "").unwrap();
        fs::write(project.0.join("web/package.json"), "{}").unwrap();
        let ctx = ProjectContext::new(project.0.clone());

        let contexts = ctx.component_contexts();
        assert!(std::ptr::eq(contexts, ctx.component_contexts()));
        assert_eq!(
            contexts
                .iter()
                .map(ComponentContext::component)
                .collect::<Vec<_>>(),
            ctx.components().iter().collect::<Vec<_>>()
        );
        assert!(std::ptr::eq(
            contexts.first().unwrap().cargo(),
            ctx.component_contexts().first().unwrap().cargo()
        ));
    }

    #[test]
    fn component_cargo_configs_are_checked_too() {
        let project = TempProject::new("cargo-config", None);
        fs::create_dir_all(project.0.join(".cargo")).unwrap();
        fs::write(
            project.0.join(".cargo/config.toml"),
            "[registry]\nglobal-min-publish-age = \"3 days\"\n",
        )
        .unwrap();
        for component in ["backend", "tools"] {
            fs::create_dir_all(project.0.join(component)).unwrap();
            fs::write(project.0.join(component).join("Cargo.toml"), "").unwrap();
        }
        fs::create_dir_all(project.0.join("backend/.cargo")).unwrap();
        fs::write(
            project.0.join("backend/.cargo/config.toml"),
            "[registry]\nglobal-min-publish-age = \"0\"\n",
        )
        .unwrap();
        let results = project.verify();

        let ages: Vec<_> = find(&results, "cargo-config-min-publish-age")
            .iter()
            .map(|r| (r.component.as_deref(), r.status))
            .collect();
        assert_eq!(
            ages,
            [(None, Status::Pass), (Some("backend"), Status::Warn)]
        );
    }

    #[test]
    fn components_use_the_nearest_toolchain_file() {
        let project = TempProject::new("toolchain", None);
        for component in ["legacy/app", "broken/app", "plain"] {
            fs::create_dir_all(project.0.join(component)).unwrap();
            fs::write(project.0.join(component).join("Cargo.toml"), "").unwrap();
        }
        fs::write(project.0.join("legacy/rust-toolchain"), "1.99.1\n").unwrap();
        fs::write(
            project.0.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"1.99.1\"\n",
        )
        .unwrap();
        fs::write(project.0.join("broken/rust-toolchain.toml"), "[toolchain\n").unwrap();
        let results = project.verify();

        let toolchain: Vec<_> = results
            .iter()
            .filter(|r| {
                r.id.starts_with("rust-toolchain-toml-") && r.id != "rust-toolchain-toml-components"
            })
            .map(|r| (r.component.as_deref(), r.name.as_str(), r.status))
            .collect();
        assert_eq!(
            toolchain,
            [
                (
                    Some("broken/app"),
                    "../rust-toolchain.toml: exists",
                    Status::Pass
                ),
                (
                    Some("broken/app"),
                    "../rust-toolchain.toml: valid",
                    Status::Fail
                ),
                (
                    Some("legacy/app"),
                    "../rust-toolchain: exists",
                    Status::Warn
                ),
                (
                    Some("plain"),
                    "../rust-toolchain.toml: exists",
                    Status::Pass
                ),
                (Some("plain"), "../rust-toolchain.toml: valid", Status::Pass),
                (
                    Some("plain"),
                    "../rust-toolchain.toml: exact channel",
                    Status::Pass
                ),
            ]
        );
    }

    #[test]
    fn components_use_the_nearest_clippy_config() {
        let conforming = "absolute-paths-max-segments = 3\nallow-expect-in-tests = true\n\
                          allow-panic-in-tests = true\nallow-unwrap-in-tests = true\n";
        let project = TempProject::new("clippy-toml", None);
        for component in ["both", "nested/app"] {
            fs::create_dir_all(project.0.join(component)).unwrap();
            fs::write(project.0.join(component).join("Cargo.toml"), "").unwrap();
        }
        fs::write(project.0.join("both/.clippy.toml"), conforming).unwrap();
        fs::write(project.0.join("both/clippy.toml"), "[broken\n").unwrap();
        fs::write(
            project.0.join("clippy.toml"),
            "allow-panic-in-tests = true\n",
        )
        .unwrap();
        let clippy = |results: &[CheckResult]| -> Vec<(Option<String>, String, Status)> {
            results
                .iter()
                .filter(|r| r.id.starts_with("clippy-toml-"))
                .map(|r| (r.component.clone(), r.name.clone(), r.status))
                .collect()
        };

        let both = |name: &str| (Some("both".to_owned()), format!(".clippy.toml: {name}"));
        let nested = |name: &str| {
            (
                Some("nested/app".to_owned()),
                format!("../../clippy.toml: {name}"),
            )
        };
        let expected: Vec<_> = [
            (both("exists"), Status::Pass),
            (both("valid"), Status::Pass),
            (
                both("unwrap, expect and panic allowed in tests"),
                Status::Pass,
            ),
            (both("absolute paths limited to 3 segments"), Status::Pass),
            (nested("exists"), Status::Pass),
            (nested("valid"), Status::Pass),
            (
                nested("unwrap, expect and panic allowed in tests"),
                Status::Warn,
            ),
            (nested("absolute paths limited to 3 segments"), Status::Warn),
        ]
        .into_iter()
        .map(|((component, name), status)| (component, name, status))
        .collect();
        assert_eq!(clippy(&project.verify()), expected);

        fs::remove_file(project.0.join("clippy.toml")).unwrap();
        let missing: Vec<_> = clippy(&project.verify())
            .into_iter()
            .filter(|(component, ..)| component.as_deref() == Some("nested/app"))
            .collect();
        assert_eq!(
            missing,
            [(
                Some("nested/app".to_owned()),
                "clippy.toml: exists".to_owned(),
                Status::Fail
            )]
        );
    }

    #[test]
    fn workspace_members_use_their_own_clippy_configs() {
        let conforming = "absolute-paths-max-segments = 3\nallow-expect-in-tests = true\n\
                          allow-panic-in-tests = true\nallow-unwrap-in-tests = true\n";
        let project = TempProject::new("member-clippy-toml", None);
        for member in ["ws/crates/a", "ws/crates/b", "ws/tools/x"] {
            fs::create_dir_all(project.0.join(member)).unwrap();
            fs::write(
                project.0.join(member).join("Cargo.toml"),
                "[package]\nname = \"member\"\n",
            )
            .unwrap();
        }
        fs::write(
            project.0.join("ws/Cargo.toml"),
            "[workspace]\nmembers = [\"crates/*\", \"tools/x\"]\n",
        )
        .unwrap();
        fs::write(project.0.join("clippy.toml"), conforming).unwrap();
        fs::write(
            project.0.join("ws/crates/clippy.toml"),
            "allow-panic-in-tests = true\n",
        )
        .unwrap();
        fs::write(project.0.join("ws/crates/b/.clippy.toml"), conforming).unwrap();

        let clippy: Vec<_> = project
            .verify()
            .into_iter()
            .filter(|r| r.id.starts_with("clippy-toml-"))
            .map(|r| (r.name, r.status))
            .collect();
        let expected: Vec<_> = [
            ("../clippy.toml", Status::Pass),
            ("crates/b/.clippy.toml", Status::Pass),
            ("crates/clippy.toml", Status::Warn),
        ]
        .into_iter()
        .flat_map(|(label, status)| {
            [
                (format!("{label}: exists"), Status::Pass),
                (format!("{label}: valid"), Status::Pass),
                (
                    format!("{label}: unwrap, expect and panic allowed in tests"),
                    status,
                ),
                (
                    format!("{label}: absolute paths limited to 3 segments"),
                    status,
                ),
            ]
        })
        .collect();
        assert_eq!(clippy, expected);
    }

    #[test]
    fn prettier_must_load_and_depend_on_the_packagejson_plugin() {
        let project = TempProject::new("prettier", None);
        for (component, prettierrc) in [
            ("listed", "plugins: [prettier-plugin-packagejson]\n"),
            ("unlisted", "{ \"printWidth\": 120 }\n"),
            ("malformed", "plugins: [\n"),
        ] {
            fs::create_dir_all(project.0.join(component)).unwrap();
            fs::write(
                project.0.join(component).join("package.json"),
                "{ \"devDependencies\": { \"prettier\": \"3.9.10\" } }\n",
            )
            .unwrap();
            fs::write(project.0.join(component).join(".prettierrc"), prettierrc).unwrap();
        }
        let results = project.verify();

        let prettier: Vec<_> = results
            .iter()
            .filter(|r| {
                ["prettier-config-valid", "prettier-plugin-packagejson"].contains(&r.id.as_str())
            })
            .map(|r| {
                (
                    r.component.as_deref(),
                    r.id.as_str(),
                    r.status,
                    r.message.lines().next().unwrap_or_default(),
                )
            })
            .collect();
        assert_eq!(
            prettier,
            [
                (Some("listed"), "prettier-config-valid", Status::Pass, ""),
                (
                    Some("listed"),
                    "prettier-plugin-packagejson",
                    Status::Fail,
                    "prettier-plugin-packagejson isn't in dependencies"
                ),
                (
                    Some("malformed"),
                    "prettier-config-valid",
                    Status::Fail,
                    "Invalid YAML: error: line 1 column 10: unclosed bracket '['"
                ),
                (Some("unlisted"), "prettier-config-valid", Status::Pass, ""),
                (
                    Some("unlisted"),
                    "prettier-plugin-packagejson",
                    Status::Warn,
                    "prettier-plugin-packagejson isn't in .prettierrc's plugins"
                ),
            ]
        );
    }

    #[test]
    fn unpublished_libraries_need_no_changelog() {
        let project = TempProject::new("changelog", None);
        for (component, manifest, file) in [
            (
                "crates",
                "[workspace]\nmembers = [\"core\"]\n\n[workspace.package]\npublish = false\n",
                "Cargo.toml",
            ),
            (
                "crates/core",
                "[package]\nname = \"core\"\npublish.workspace = true\n",
                "Cargo.toml",
            ),
            (
                "py",
                "[project]\nname = \"lib\"\nclassifiers = [\"Private :: Do Not Upload\"]\n",
                "pyproject.toml",
            ),
        ] {
            fs::create_dir_all(project.0.join(component)).unwrap();
            fs::write(project.0.join(component).join(file), manifest).unwrap();
        }
        let unpublished = project.verify();
        assert!(matches!(
            find(&unpublished, "changelog-exists").as_slice(),
            [r] if r.status == Status::Pass
        ));

        fs::write(
            project.0.join("crates/core/Cargo.toml"),
            "[package]\nname = \"core\"\n",
        )
        .unwrap();
        let results = project.verify();
        let changelog = find(&results, "changelog-exists");
        assert!(matches!(
            changelog.as_slice(),
            [r] if r.status == Status::Fail
                && r.message == "Not found: CHANGELOG.md, which the project needs because the Rust component at crates \
                                 is a published library"
        ));
    }

    #[test]
    fn changelog_waivers_in_applications_are_unneeded() {
        let results = TempProject::new(
            "changelog-waiver",
            Some("[[waivers]]\ncheck = \"changelog-exists\"\nreason = \"Deployed from main.\"\n"),
        )
        .verify();

        let needed = find(&results, "norms-toml-waiver-needed");
        assert!(matches!(
            needed.as_slice(),
            [r] if r.status == Status::Warn && r.message == "The check passes without it"
        ));
    }

    #[test]
    fn other_markdownlint_configs_are_reported() {
        let project = TempProject::new("markdownlint-configs", None);
        let config = r#"{
            "config": {
                "MD004": { "style": "dash" },
                "MD013": { "tables": false, "code_blocks": false },
                "MD024": { "siblings_only": true }
            },
            "ignores": ["{vendor,third_party}/**", "**/*.json"]
        }"#;
        fs::write(project.0.join(".markdownlint-cli2.jsonc"), config).unwrap();
        for dir in ["vendor/lib", "third_party", "docs", ".github"] {
            fs::create_dir_all(project.0.join(dir)).unwrap();
        }
        fs::write(project.0.join("vendor/lib/.markdownlint.json"), "{}").unwrap();
        fs::write(project.0.join("third_party/.markdownlint.json"), "{}").unwrap();
        let statuses = |results: &[CheckResult]| -> Vec<(String, Status, String)> {
            results
                .iter()
                .filter(|r| {
                    ["markdownlint-cli2-rules", "markdownlint-cli2-settings"]
                        .contains(&r.id.as_str())
                })
                .map(|r| (r.id.clone(), r.status, r.message.clone()))
                .collect()
        };
        assert!(
            statuses(&project.verify())
                .iter()
                .all(|(_, status, _)| *status == Status::Pass)
        );

        fs::write(project.0.join(".markdownlint.yaml"), "MD013: false\n").unwrap();
        fs::write(project.0.join("docs/.markdownlint-cli2.jsonc"), "{}").unwrap();
        // Ignoring a config file doesn't stop it applying to the other files of its directory.
        fs::write(project.0.join(".github/.markdownlint.json"), "{}").unwrap();
        let message = "Other markdownlint configuration that norms doesn't read: .github/.markdownlint.json, \
                       .markdownlint.yaml, docs/.markdownlint-cli2.jsonc";
        assert_eq!(
            statuses(&project.verify()),
            [
                (
                    "markdownlint-cli2-rules".to_owned(),
                    Status::Warn,
                    message.to_owned()
                ),
                (
                    "markdownlint-cli2-settings".to_owned(),
                    Status::Warn,
                    message.to_owned()
                ),
            ]
        );
    }

    /// Runs the checks against this repository, which exercises the common and Rust checks. Apart from the one result
    /// per waiver, excluded directory or workflow, no check here records more than one result for each component, so
    /// an ID repeated within a component means two checks share it.
    #[test]
    fn check_ids_are_valid_and_unique() {
        let ctx = ProjectContext::new(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
        let mut report = Report::new(ctx.path().to_path_buf(), ctx.components().to_vec());
        run_all_checks(&ctx, &mut report);

        let mut seen = HashSet::new();
        for result in &report.results {
            assert!(is_valid_check_id(&result.id), "invalid ID: {}", result.id);
            assert!(
                [
                    "norms-toml-waiver-needed",
                    "norms-toml-exclude-exists",
                    "workflow-valid",
                    "workflow-job-timeouts",
                    "workflow-concurrency",
                ]
                .contains(&result.id.as_str())
                    || seen.insert((&result.component, &result.id)),
                "duplicate ID: {}",
                result.id
            );
        }
    }
}
