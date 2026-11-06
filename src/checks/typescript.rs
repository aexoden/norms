// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! TypeScript-specific checks.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{
    PrecommitHook, TsCompilerOptions, TsConfig, Workflow, check_dev_engine, commands, program_args,
};
use crate::context::{ComponentContext, ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

//
// Package configuration
//

pub(crate) fn check_package_json(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("package-json", "package.json", ctx.package_json())
    else {
        return;
    };

    // Name field
    report.should(
        "package-json-name",
        "package.json: has name",
        config.name.as_ref().is_some_and(|n| !n.is_empty()),
    );

    if let Some(license) = &config.license {
        report.should(
            "package-json-license-texts",
            "package.json: license has its texts in LICENSES/",
            license.as_str().map_or_else(
                || Err("Not an SPDX expression string".to_owned()),
                |expression| ctx.license_files().check_expression(expression),
            ),
        );
    }

    // ESM module type
    if config.is_esm() {
        report.should(
            "package-json-esm",
            "package.json: uses ESM (type: module)",
            Outcome::Pass,
        );
    } else {
        report.should(
            "package-json-esm",
            "package.json: uses ESM (type: module)",
            Outcome::fail("Consider setting \"type\": \"module\" for ESM support"),
        );
    }

    // Version pinning. Every TypeScript component is an application.
    report.should(
        "package-json-dependencies-exact",
        "package.json: dependencies pinned exactly (application)",
        config.check_dependencies_exact(),
    );

    // Toolchain pins
    report.should(
        "package-json-dev-engines-runtime",
        "package.json: pins Node.js in devEngines",
        check_dev_engine(config, "runtime", "node"),
    );
    report.should(
        "package-json-dev-engines-package-manager",
        "package.json: pins pnpm in devEngines",
        check_dev_engine(config, "packageManager", "pnpm"),
    );
    report.should(
        "package-json-no-corepack-package-manager",
        "package.json: no Corepack packageManager field",
        config.check_no_package_manager(),
    );
}

//
// Package manager
//

pub(crate) fn check_pnpm(ctx: &ComponentContext, report: &mut Report) {
    if ctx.package_json().as_ref_ok().is_none() {
        return;
    }

    let typescript_root = ctx.path();

    // Lock file
    report.should(
        "pnpm-lock-exists",
        "pnpm: pnpm-lock.yaml exists",
        file_exists(&typescript_root.join("pnpm-lock.yaml")),
    );

    // Warn if competing lock files are present
    for (id, lockfile, manager) in [
        ("pnpm-no-npm-lock", "package-lock.json", "npm"),
        ("pnpm-no-yarn-lock", "yarn.lock", "yarn"),
    ] {
        if typescript_root.join(lockfile).exists() {
            report.should(
                id,
                format!("pnpm: no {manager} lock file"),
                Outcome::fail(format!(
                    "Found {lockfile}; consider removing if pnpm is the intended package manager"
                )),
            );
        }
    }
}

pub(crate) fn check_pnpm_workspace(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.recommend_parsed(
        "pnpm-workspace",
        "pnpm-workspace.yaml",
        ctx.pnpm_workspace(),
    ) else {
        return;
    };

    report.should(
        "pnpm-workspace-minimum-release-age",
        "pnpm: release age of 3 days (minimumReleaseAge)",
        config.check_minimum_release_age(),
    );
}

//
// TypeScript configuration
//

pub(crate) fn check_tsconfig(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("tsconfig", "tsconfig.json", ctx.tsconfig()) else {
        return;
    };

    if config.is_solution_style() {
        check_tsconfig_solution_style(ctx, config, report);
    } else {
        check_tsconfig_single(config, report);
    }
}

/// Check a single (non-solution-style) tsconfig with inline `compilerOptions`.
fn check_tsconfig_single(config: &TsConfig, report: &mut Report) {
    let Some(compiler_options) = &config.compiler_options else {
        report.must(
            "tsconfig-compiler-options",
            "tsconfig.json: has compilerOptions",
            Outcome::fail("Missing compilerOptions section"),
        );
        return;
    };

    report.must(
        "tsconfig-compiler-options",
        "tsconfig.json: has compilerOptions",
        Outcome::Pass,
    );
    check_compiler_options("tsconfig.json", compiler_options, report);
}

/// Check a solution-style tsconfig that delegates to referenced sub-projects.
fn check_tsconfig_solution_style(ctx: &ComponentContext, config: &TsConfig, report: &mut Report) {
    report.should(
        "tsconfig-project-references",
        "tsconfig.json: uses project references",
        Outcome::Pass,
    );

    let ref_paths = config.reference_paths();
    if ref_paths.is_empty() {
        report.must(
            "tsconfig-references-non-empty",
            "tsconfig.json: references non-empty",
            Outcome::fail("references array is empty"),
        );
        return;
    }

    report.must(
        "tsconfig-references-non-empty",
        "tsconfig.json: references non-empty",
        Outcome::Pass,
    );

    let referenced = ctx.typescript_referenced_configs();

    if referenced.is_empty() {
        report.must(
            "tsconfig-references-parsed",
            "tsconfig.json: referenced configs parsed",
            Outcome::fail("Could not resolve any referenced configs"),
        );
        return;
    }

    // Validate each referenced config exists and is valid, then check its compilerOptions.
    let mut any_valid = false;
    for (name, status) in referenced {
        match status {
            ConfigStatus::NotFound => {
                report.must(
                    "tsconfig-reference-exists",
                    format!("{name}: exists"),
                    Outcome::fail("Referenced config not found"),
                );
            }
            ConfigStatus::ParseError { error, .. } => {
                report.must(
                    "tsconfig-reference-exists",
                    format!("{name}: exists"),
                    Outcome::Pass,
                );
                report.must(
                    "tsconfig-reference-valid",
                    format!("{name}: valid"),
                    Outcome::fail(error.clone()),
                );
            }
            ConfigStatus::Ok(ref_config) => {
                report.must(
                    "tsconfig-reference-exists",
                    format!("{name}: exists"),
                    Outcome::Pass,
                );
                report.must(
                    "tsconfig-reference-valid",
                    format!("{name}: valid"),
                    Outcome::Pass,
                );

                if let Some(opts) = &ref_config.compiler_options {
                    check_compiler_options(name, opts, report);
                    any_valid = true;
                } else {
                    report.must(
                        "tsconfig-reference-compiler-options",
                        format!("{name}: has compilerOptions"),
                        Outcome::fail("Missing compilerOptions section"),
                    );
                }
            }
        }
    }

    if !any_valid {
        report.must(
            "tsconfig-reference-any-compiler-options",
            "tsconfig: at least one reference has compilerOptions",
            Outcome::fail("No referenced config has a compilerOptions section"),
        );
    }
}

/// Check compiler options common to both single and solution-style configs.
///
/// The `label` is used as a prefix in check names so that solution-style configs produce per-reference results (e.g.
/// "tsconfig.app.json: strict mode enabled").
fn check_compiler_options(label: &str, opts: &TsCompilerOptions, report: &mut Report) {
    // Strict mode
    report.must(
        "tsconfig-strict",
        format!("{label}: strict mode enabled"),
        opts.strict == Some(true),
    );

    // noUncheckedIndexedAccess
    report.should(
        "tsconfig-no-unchecked-indexed-access",
        format!("{label}: noUncheckedIndexedAccess enabled"),
        if opts.no_unchecked_indexed_access == Some(true) {
            Outcome::Pass
        } else {
            Outcome::fail("Consider enabling noUncheckedIndexedAccess for safer index access")
        },
    );

    // Module setting
    report.should(
        "tsconfig-module",
        format!("{label}: module setting configured"),
        opts.module.is_some(),
    );

    // Target setting
    report.should(
        "tsconfig-target",
        format!("{label}: target configured"),
        opts.target.is_some(),
    );

    // verbatimModuleSyntax
    report.should(
        "tsconfig-verbatim-module-syntax",
        format!("{label}: verbatimModuleSyntax enabled"),
        if opts.verbatim_module_syntax == Some(true) {
            Outcome::Pass
        } else {
            Outcome::fail(
                "Consider enabling verbatimModuleSyntax for explicit import/export syntax",
            )
        },
    );

    // isolatedModules (important for bundler compatibility)
    report.should(
        "tsconfig-isolated-modules",
        format!("{label}: isolatedModules enabled"),
        if opts.isolated_modules == Some(true) {
            Outcome::Pass
        } else {
            Outcome::fail("Consider enabling isolatedModules for bundler compatibility")
        },
    );
}

//
// Linting
//

pub(crate) fn check_eslint(ctx: &ComponentContext, report: &mut Report) {
    let ts_root = ctx.path();
    let eslint_config = ts_root.join("eslint.config.ts");

    report.must(
        "eslint-config-exists",
        "ESLint: configured",
        file_exists(&eslint_config),
    );

    // Check for ESLint in dependencies
    if let Some(pkg) = ctx.package_json().as_ref_ok() {
        report.should(
            "eslint-dependency",
            "ESLint: in dependencies",
            pkg.has_dep("eslint"),
        );

        // Check for typescript-eslint
        let has_ts_eslint =
            pkg.has_dep("typescript-eslint") || pkg.has_dep_starting_with("@typescript-eslint/");
        report.should(
            "eslint-typescript-eslint",
            "ESLint: typescript-eslint configured",
            if has_ts_eslint {
                Outcome::Pass
            } else {
                Outcome::fail("Consider adding typescript-eslint for TypeScript-aware linting")
            },
        );
    }
}

//
// Formatting
//

/// The Prettier plugin that formats `package.json`, sorting its dependencies.
const PACKAGEJSON_PLUGIN: &str = "prettier-plugin-packagejson";

pub(crate) fn check_prettier(ctx: &ComponentContext, report: &mut Report) {
    let config = report.require_parsed("prettier-config", ".prettierrc", ctx.prettier());

    // .prettierignore
    report.should(
        "prettier-ignore-exists",
        "Prettier: .prettierignore exists",
        if ctx.path().join(".prettierignore").exists() {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding .prettierignore")
        },
    );

    // Prettier in dependencies
    let package_json = ctx.package_json().as_ref_ok();
    if let Some(pkg) = package_json {
        report.should(
            "prettier-dependency",
            "Prettier: in dependencies",
            pkg.has_dep("prettier"),
        );
    }

    let Some(config) = config else {
        return;
    };

    // package.json formatting, which sorts its dependencies
    report.should(
        "prettier-plugin-packagejson",
        "Prettier: formats package.json with prettier-plugin-packagejson",
        if !config.has_plugin(PACKAGEJSON_PLUGIN) {
            Outcome::fail(format!(
                "{PACKAGEJSON_PLUGIN} isn't in .prettierrc's plugins"
            ))
        } else if package_json.is_some_and(|pkg| !pkg.has_dep(PACKAGEJSON_PLUGIN)) {
            Outcome::fail(format!("{PACKAGEJSON_PLUGIN} isn't in dependencies"))
        } else {
            Outcome::Pass
        },
    );
}

//
// Scripts
//

pub(crate) fn check_scripts(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = ctx.package_json().as_ref_ok() else {
        return;
    };

    if config.scripts.is_none() {
        report.should(
            "package-json-scripts",
            "package.json: has scripts",
            Outcome::fail("No scripts defined in package.json"),
        );
        return;
    }

    report.should(
        "package-json-scripts",
        "package.json: has scripts",
        Outcome::Pass,
    );

    // Build script
    report.should(
        "package-json-script-build",
        "Scripts: has 'build'",
        if config.has_script("build") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'build' script")
        },
    );

    // Lint script
    report.should(
        "package-json-script-lint",
        "Scripts: has 'lint' (eslint with --max-warnings 0)",
        config
            .scripts
            .as_ref()
            .and_then(|scripts| scripts.get("lint"))
            .map_or_else(
                || Err("Consider adding a 'lint' script".to_owned()),
                |script| lint_script_strict(script),
            ),
    );

    // Format script
    let has_format = config.has_any_script(&["format", "fmt", "prettier"]);
    report.should(
        "package-json-script-format",
        "Scripts: has 'format'",
        if has_format {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'format' script")
        },
    );

    // Type check script
    report.should(
        "package-json-script-typecheck",
        "Scripts: has 'typecheck'",
        if config.has_script("typecheck") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'typecheck' script")
        },
    );

    // Test script
    report.should(
        "package-json-script-test",
        "Scripts: has 'test'",
        if config.has_script("test") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'test' script")
        },
    );
}

//
// Pre-commit integration
//

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(precommit) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.should(
        "typescript-precommit-pnpm-lock",
        "Pre-commit: has pnpm-lock hook",
        precommit.check_commit_hook("pnpm-lock"),
    );
    report.should(
        "typescript-precommit-pnpm-peer-dependencies",
        "Pre-commit: has pnpm-peer-dependencies hook",
        precommit.check_commit_hook("pnpm-peer-dependencies"),
    );
    report.should(
        "typescript-precommit-eslint",
        "Pre-commit: has ESLint hook (--max-warnings 0)",
        precommit.commit_hooks("eslint").and_then(|hooks| {
            if hooks.into_iter().any(eslint_hook_strict) {
                Ok(())
            } else {
                Err("The ESLint hook doesn't pass --max-warnings 0".to_owned())
            }
        }),
    );
    report.should(
        "typescript-precommit-prettier",
        "Pre-commit: has Prettier hook",
        precommit.check_commit_hook("prettier"),
    );
    report.should(
        "typescript-precommit-tsc",
        "Pre-commit: has TypeScript type check hook",
        precommit.check_commit_hook("tsc"),
    );
    report.should(
        "typescript-precommit-vite-build",
        "Pre-commit: has vite-build hook",
        precommit.check_commit_hook("vite-build"),
    );
    report.should(
        "typescript-precommit-vitest",
        "Pre-commit: has vitest hook",
        precommit.check_commit_hook("vitest"),
    );
}

//
// CI integration
//

/// A Vite build in a CI script: `vite build`, directly or through `pnpm exec`, or the `build` script, which runs it
/// after `tsc -b`. CI sets the job's working directory for a component in a subdirectory, so pnpm options before the
/// script, such as `--dir`, aren't recognized.
static VITE_BUILD: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"(?m)(?:^|[^\w-])(?:vite[ \t]+build|pnpm[ \t]+(?:run[ \t]+)?build)(?:$|[^\w:-])")
        .unwrap()
});

pub(crate) fn check_ci(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.ci_workflow() else {
        return;
    };

    // Peer dependencies
    report.should(
        "typescript-ci-pnpm-peer-dependencies",
        "TypeScript CI: peer dependencies",
        workflow
            .run_lines()
            .any(|line| commands(&line).any(|words| checks_peer_dependencies(&words))),
    );

    // Lint step
    report.should(
        "typescript-ci-lint",
        "TypeScript CI: lint check (--max-warnings 0)",
        eslint_in_ci(workflow),
    );

    // Format check
    let has_format = workflow.runs_command("prettier");
    report.should(
        "typescript-ci-format",
        "TypeScript CI: format check",
        has_format,
    );

    // Type check
    report.should(
        "typescript-ci-tsc",
        "TypeScript CI: type check",
        workflow.runs_command("tsc"),
    );

    // Tests
    report.should(
        "typescript-ci-test",
        "TypeScript CI: tests",
        workflow.runs_command("vitest")
            || workflow.runs_command("pnpm test")
            || workflow.runs_command("pnpm run test"),
    );

    // Build
    report.should(
        "typescript-ci-vite-build",
        "TypeScript CI: Vite build",
        workflow.runs_matching(&VITE_BUILD),
    );
}

/// Whether a command's words run `eslint`, and if so, whether with `--max-warnings 0`, which fails on warnings as well as
/// errors.
fn eslint_max_warnings_zero(words: &[&str]) -> Option<bool> {
    let mut args = program_args(words, "eslint")?.iter();
    let mut strict = false;
    while let Some(&arg) = args.next() {
        let value = if arg == "--max-warnings" {
            args.next().copied()
        } else {
            arg.strip_prefix("--max-warnings=")
        };
        if let Some(value) = value {
            // ESLint takes the last value given.
            strict = value == "0";
        }
    }
    Some(strict)
}

/// Whether a `lint` script fails on warnings from `eslint`: each command of it that runs `eslint` passes
/// `--max-warnings 0`. A script that doesn't run `eslint` itself, such as one running another script, isn't judged.
fn lint_script_strict(script: &str) -> Result<(), String> {
    if commands(script).any(|words| eslint_max_warnings_zero(&words) == Some(false)) {
        Err("The lint script runs eslint without --max-warnings 0".to_owned())
    } else {
        Ok(())
    }
}

/// Whether an `eslint` hook passes `--max-warnings 0`, in its entry or the args pre-commit appends to it. A hook without
/// an entry, as a remote hook may leave it to the hook's repository, is taken to run `eslint`.
fn eslint_hook_strict(hook: &PrecommitHook) -> bool {
    hook.filter_map_commands("eslint", eslint_max_warnings_zero)
        .contains(&true)
}

/// Check that some step runs `eslint` with `--max-warnings 0`. A step running the `lint` script doesn't count, since the
/// flag would have to be found in the script of the right component.
fn eslint_in_ci(workflow: &Workflow) -> Result<(), String> {
    let runs = workflow.filter_map_commands(eslint_max_warnings_zero);
    if runs.contains(&true) {
        Ok(())
    } else if !runs.is_empty() {
        Err("ESLint runs without --max-warnings 0".to_owned())
    } else if workflow.runs_command("pnpm lint") || workflow.runs_command("pnpm run lint") {
        Err("CI runs the lint script rather than eslint with --max-warnings 0".to_owned())
    } else {
        Err("No step runs eslint".to_owned())
    }
}

/// Whether a command's words check peer dependencies with `pnpm peers check`, which may follow pnpm options such as
/// `--dir`, or a second `pnpm` run through `exec`.
fn checks_peer_dependencies(words: &[&str]) -> bool {
    program_args(words, "pnpm")
        .is_some_and(|rest| rest.windows(2).any(|pair| pair == ["peers", "check"]))
}

/// Check that `audit.yaml` runs `pnpm audit`.
pub(crate) fn check_audit(ctx: &ProjectContext, report: &mut Report) {
    let ConfigStatus::Ok(workflow) = ctx.audit_workflow() else {
        return;
    };

    report.should(
        "typescript-audit-pnpm",
        "TypeScript audit: pnpm audit",
        workflow.runs_command("pnpm audit"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eslint_fails_on_warnings_with_max_warnings_zero() {
        let strict = |line: &str| {
            let words: Vec<&str> = line.split_whitespace().collect();
            eslint_max_warnings_zero(&words)
        };
        for line in [
            "pnpm exec eslint . --max-warnings 0",
            "node_modules/.bin/eslint --max-warnings=0 src",
            "eslint --max-warnings 10 . --max-warnings 0",
        ] {
            assert_eq!(strict(line), Some(true), "{line}");
        }
        for line in [
            "pnpm exec eslint .",
            "eslint . --max-warnings 10",
            "eslint . --max-warnings",
            "eslint --max-warnings 0 . --max-warnings=5",
        ] {
            assert_eq!(strict(line), Some(false), "{line}");
        }
        assert_eq!(strict("pnpm lint --max-warnings 0"), None);

        let hook = |entry: Option<&str>, args: &[&str]| PrecommitHook {
            id: "eslint".to_owned(),
            entry: entry.map(str::to_owned),
            args: args.iter().map(|&arg| arg.to_owned()).collect(),
            ..PrecommitHook::default()
        };
        assert!(eslint_hook_strict(&hook(
            Some("pnpm exec eslint . --max-warnings 0"),
            &[]
        )));
        assert!(eslint_hook_strict(&hook(
            Some("pnpm exec eslint ."),
            &["--max-warnings=0"]
        )));
        assert!(eslint_hook_strict(&hook(None, &["--max-warnings", "0"])));
        assert!(!eslint_hook_strict(&hook(Some("pnpm exec eslint ."), &[])));
        assert!(!eslint_hook_strict(&hook(None, &[])));
        for script in [
            "eslint . --max-warnings 0",
            "eslint . --max-warnings=0 && prettier --check .",
            "pnpm run lint:all",
        ] {
            assert_eq!(lint_script_strict(script), Ok(()), "{script}");
        }
        for script in ["eslint .", "tsc -b && eslint . --max-warnings 5"] {
            assert!(lint_script_strict(script).is_err(), "{script}");
        }
        assert!(!eslint_hook_strict(&hook(
            Some("pnpm exec eslint '--max-warnings 0'"),
            &[]
        )));

        let workflow = |run: &str| -> Workflow {
            serde_saphyr::from_str(&format!("jobs:\n  lint:\n    steps:\n      - run: {run}\n"))
                .unwrap()
        };
        assert_eq!(
            eslint_in_ci(&workflow("pnpm exec eslint . --max-warnings 0")),
            Ok(())
        );
        assert_eq!(
            eslint_in_ci(&workflow("pnpm exec eslint .")),
            Err("ESLint runs without --max-warnings 0".to_owned())
        );
        assert_eq!(
            eslint_in_ci(&workflow("pnpm run lint")),
            Err("CI runs the lint script rather than eslint with --max-warnings 0".to_owned())
        );
        assert_eq!(
            eslint_in_ci(&workflow("pnpm exec tsc -b")),
            Err("No step runs eslint".to_owned())
        );
    }

    #[test]
    fn peer_dependencies_are_checked_by_pnpm_peers_check() {
        let checks = |line: &str| {
            let words: Vec<&str> = line.split_whitespace().collect();
            checks_peer_dependencies(&words)
        };
        for line in [
            "pnpm peers check",
            "pnpm --dir frontend exec pnpm peers check",
            "corepack pnpm peers check --json",
        ] {
            assert!(checks(line), "{line}");
        }
        for line in [
            "pnpm peers",
            "peers check",
            "pnpm install --frozen-lockfile",
        ] {
            assert!(!checks(line), "{line}");
        }
    }

    #[test]
    fn vite_build_runs_vite_or_the_build_script() {
        for run in [
            "pnpm exec vite build",
            "vite build --mode production",
            "pnpm run build",
            "pnpm build\npnpm exec vitest run",
        ] {
            assert!(VITE_BUILD.is_match(run), "{run}");
        }
        for run in [
            "pnpm run build:docs",
            "pnpm build-storybook",
            "vite preview",
            "pnpm exec vite",
            "pnpm exec vite\nbuild",
        ] {
            assert!(!VITE_BUILD.is_match(run), "{run}");
        }
    }
}
