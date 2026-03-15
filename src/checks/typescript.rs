// SPDX-License-Identifier: Apache-2.0 OR MIT
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! TypeScript-specific checks.

use crate::config::{TsCompilerOptions, TsConfig};
use crate::context::{ConfigStatus, ProjectContext};
use crate::report::{Outcome, Report, file_exists};

//
// Package configuration
//

pub fn check_package_json(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("package.json", ctx.package_json()) else {
        return;
    };

    // Name field
    report.should(
        "package.json: has name",
        config.name.as_ref().is_some_and(|n| !n.is_empty()),
    );

    // ESM module type
    if config.is_esm() {
        report.should("package.json: uses ESM (type: module)", Outcome::Pass);
    } else {
        report.should(
            "package.json: uses ESM (type: module)",
            Outcome::fail("Consider setting \"type\": \"module\" for ESM support"),
        );
    }

    // Engine constraints
    report.should(
        "package.json: has engines field",
        if config.engines.is_some() {
            Outcome::Pass
        } else {
            Outcome::fail("Consider specifying engine requirements")
        },
    );
}

//
// Package manager
//

pub fn check_pnpm(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.package_json().as_ref_ok() else {
        return;
    };

    let typescript_root = ctx.typescript_root();

    // packageManager field
    if config.uses_pnpm() {
        report.should("pnpm: configured as package manager", Outcome::Pass);
    } else if let Some(pm) = config.package_manager_display() {
        report.should(
            "pnpm: configured as package manager",
            Outcome::fail(format!("Package manager is '{pm}', expected pnpm")),
        );
    } else {
        report.should(
            "pnpm: configured as package manager",
            Outcome::fail("Missing 'packageManager' field"),
        );
    }

    // Lock file
    report.should(
        "pnpm: pnpm-lock.yaml exists",
        file_exists(&typescript_root.join("pnpm-lock.yaml")),
    );

    // Warn if competing lock files are present
    for (lockfile, manager) in [("package-lock.json", "npm"), ("yarn.lock", "yarn")] {
        if typescript_root.join(lockfile).exists() {
            report.should(
                format!("pnpm: no {manager} lock file"),
                Outcome::fail(format!(
                    "Found {lockfile}; consider removing if pnpm is the intended package manager"
                )),
            );
        }
    }
}

//
// TypeScript configuration
//

pub fn check_tsconfig(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = report.require_parsed("tsconfig.json", ctx.tsconfig()) else {
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
            "tsconfig.json: has compilerOptions",
            Outcome::fail("Missing compilerOptions section"),
        );
        return;
    };

    report.must("tsconfig.json: has compilerOptions", Outcome::Pass);
    check_compiler_options("tsconfig.json", compiler_options, report);
}

/// Check a solution-style tsconfig that delegates to referenced sub-projects.
fn check_tsconfig_solution_style(ctx: &ProjectContext, config: &TsConfig, report: &mut Report) {
    report.should("tsconfig.json: uses project references", Outcome::Pass);

    let ref_paths = config.reference_paths();
    if ref_paths.is_empty() {
        report.must(
            "tsconfig.json: references non-empty",
            Outcome::fail("references array is empty"),
        );
        return;
    }

    report.must("tsconfig.json: references non-empty", Outcome::Pass);

    let referenced = ctx.typescript_referenced_configs();

    if referenced.is_empty() {
        report.must(
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
                    format!("{name}: exists"),
                    Outcome::fail("Referenced config not found"),
                );
            }
            ConfigStatus::ParseError { error, .. } => {
                report.must(format!("{name}: exists"), Outcome::Pass);
                report.must(format!("{name}: valid"), Outcome::fail(error.clone()));
            }
            ConfigStatus::Ok(ref_config) => {
                report.must(format!("{name}: exists"), Outcome::Pass);
                report.must(format!("{name}: valid"), Outcome::Pass);

                if let Some(opts) = &ref_config.compiler_options {
                    check_compiler_options(name, opts, report);
                    any_valid = true;
                } else {
                    report.must(
                        format!("{name}: has compilerOptions"),
                        Outcome::fail("Missing compilerOptions section"),
                    );
                }
            }
        }
    }

    if !any_valid {
        report.must(
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
        format!("{label}: strict mode enabled"),
        opts.strict == Some(true),
    );

    // noUncheckedIndexedAccess
    report.should(
        format!("{label}: noUncheckedIndexedAccess enabled"),
        if opts.no_unchecked_indexed_access == Some(true) {
            Outcome::Pass
        } else {
            Outcome::fail("Consider enabling noUncheckedIndexedAccess for safer index access")
        },
    );

    // Module setting
    report.should(
        format!("{label}: module setting configured"),
        opts.module.is_some(),
    );

    // Target setting
    report.should(format!("{label}: target configured"), opts.target.is_some());

    // verbatimModuleSyntax
    report.should(
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

pub fn check_eslint(ctx: &ProjectContext, report: &mut Report) {
    let ts_root = ctx.typescript_root();
    let eslint_config = ts_root.join("eslint.config.ts");

    report.must("ESLint: configured", file_exists(&eslint_config));

    // Check for ESLint in dependencies
    if let Some(pkg) = ctx.package_json().as_ref_ok() {
        report.should("ESLint: in dependencies", pkg.has_dep("eslint"));

        // Check for typescript-eslint
        let has_ts_eslint =
            pkg.has_dep("typescript-eslint") || pkg.has_dep_starting_with("@typescript-eslint/");
        report.should(
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

pub fn check_prettier(ctx: &ProjectContext, report: &mut Report) {
    let ts_root = ctx.typescript_root();
    let prettierrc = ts_root.join(".prettierrc");
    report.must("Prettier: configured", file_exists(&prettierrc));

    // .prettierignore
    report.should(
        "Prettier: .prettierignore exists",
        if ts_root.join(".prettierignore").exists() {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding .prettierignore")
        },
    );

    // Prettier in dependencies
    if let Some(pkg) = ctx.package_json().as_ref_ok() {
        report.should("Prettier: in dependencies", pkg.has_dep("prettier"));
    }
}

//
// Scripts
//

pub fn check_scripts(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.package_json().as_ref_ok() else {
        return;
    };

    if config.scripts.is_none() {
        report.should(
            "package.json: has scripts",
            Outcome::fail("No scripts defined in package.json"),
        );
        return;
    }

    report.should("package.json: has scripts", Outcome::Pass);

    // Build script
    report.should(
        "Scripts: has 'build'",
        if config.has_script("build") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'build' script")
        },
    );

    // Lint script
    report.should(
        "Scripts: has 'lint'",
        if config.has_script("lint") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'lint' script")
        },
    );

    // Format script
    let has_format = config.has_any_script(&["format", "fmt", "prettier"]);
    report.should(
        "Scripts: has 'format'",
        if has_format {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'format' script")
        },
    );

    // Type check script
    report.should(
        "Scripts: has 'typecheck'",
        if config.has_script("typecheck") {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding a 'typecheck' script")
        },
    );

    // Test script
    report.should(
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

pub fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(precommit) = ctx.precommit().as_ref_ok() else {
        return;
    };

    // pnpm-lock
    if precommit.has_hook("pnpm-lock") {
        report.should("Pre-commit: has pnpm-lock hook", Outcome::Pass);
    } else {
        report.should(
            "Pre-commit: has pnpm-lock hook",
            Outcome::fail("Consider adding a pnpm-lock hook to ensure lockfile is up to date"),
        );
    }

    // ESLint
    if precommit.has_hook("eslint") {
        report.should("Pre-commit: has ESLint hook", Outcome::Pass);
    } else {
        report.should(
            "Pre-commit: has ESLint hook",
            Outcome::fail("Consider adding an ESLint hook to pre-commit configuration"),
        );
    }

    // Prettier
    if precommit.has_hook("prettier") {
        report.should("Pre-commit: has Prettier hook", Outcome::Pass);
    } else {
        report.should(
            "Pre-commit: has Prettier hook",
            Outcome::fail("Consider adding a Prettier hook to pre-commit configuration"),
        );
    }

    // Type check
    if precommit.has_hook("tsc") {
        report.should("Pre-commit: has TypeScript type check hook", Outcome::Pass);
    } else {
        report.should(
            "Pre-commit: has TypeScript type check hook",
            Outcome::fail(
                "Consider adding a TypeScript type check hook to pre-commit configuration",
            ),
        );
    }
}

//
// Devbox integration
//

pub fn check_devbox_pnpm(ctx: &ProjectContext, report: &mut Report) {
    let Some(config) = ctx.devbox().as_ref_ok() else {
        return;
    };

    let corepack_enabled = config.env.as_ref().is_some_and(|env| {
        env.get("DEVBOX_COREPACK_ENABLED")
            .is_some_and(|v| v == "true")
    });
    report.should(
        "TypeScript devbox: enables corepack",
        if corepack_enabled {
            Outcome::Pass
        } else {
            Outcome::fail("Consider setting DEVBOX_COREPACK_ENABLED=true in Devbox configuration")
        },
    );

    let has_node = config.has_package("nodejs");
    report.should(
        "TypeScript devbox: includes nodejs",
        if has_node {
            Outcome::Pass
        } else {
            Outcome::fail("Consider adding nodejs to Devbox packages")
        },
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

    // Lint step
    let has_lint =
        lower.contains("eslint") || lower.contains("pnpm lint") || lower.contains("pnpm run lint");
    report.should("TypeScript CI: lint check", has_lint);

    // Format check
    let has_format = lower.contains("prettier");
    report.should("TypeScript CI: format check", has_format);

    // Type check
    report.should("TypeScript CI: type check", lower.contains("tsc"));

    // Tests
    report.should("TypeScript CI: tests", lower.contains("pnpm test"));
}
