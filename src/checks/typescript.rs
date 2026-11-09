// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! TypeScript-specific checks.

use std::sync::LazyLock;

use regex::Regex;

use crate::config::{
    FormatterRun, PrecommitHook, TsCompilerOptions, TsConfig, Workflow, check_dev_engine, commands,
    program_args,
};
use crate::context::{ComponentContext, ConfigStatus, ProjectContext};
use crate::report::{Issue, Outcome, Report, file_exists};

//
// Package configuration
//

pub(crate) fn check_package_json(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed("package-json", "package.json", ctx.package_json())
    else {
        return;
    };

    if let Some(license) = &config.license {
        report.check(
            "package-json-license-texts",
            "package.json: license has its texts in LICENSES/",
            license.as_str().map_or_else(
                || Err(Issue::warn("Not an SPDX expression string")),
                |expression| ctx.license_files().check_expression(expression),
            ),
        );
    }

    // ESM module type
    report.should(
        "package-json-esm",
        "package.json: uses ESM (type: module)",
        config.check_module_type(),
    );

    // Every TypeScript component is an application, which npm mustn't publish.
    report.should(
        "package-json-private",
        "package.json: private (application)",
        config.check_private(),
    );

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
    report.must(
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
                    "Found {lockfile}, though pnpm is the package manager"
                )),
            );
        }
    }
}

pub(crate) fn check_pnpm_workspace(ctx: &ComponentContext, report: &mut Report) {
    let Some(config) = report.require_parsed(
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
    report.should(
        "pnpm-workspace-permissions",
        "pnpm: records what dependencies may do (permissions)",
        config.check_permissions(),
    );
    report.should(
        "pnpm-workspace-no-legacy-build-settings",
        "pnpm: no older build settings",
        config.check_no_legacy_build_settings(),
    );
    report.should(
        "pnpm-workspace-builds-restricted",
        "pnpm: builds limited to permissions",
        config.check_builds_restricted(),
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
        check_tsconfig_solution_style(ctx, report);
    } else {
        check_tsconfig_single(config, report);
    }
}

/// Check a single (non-solution-style) tsconfig with inline `compilerOptions`, which differs from the standards'
/// solution-style config, though its options are checked all the same.
fn check_tsconfig_single(config: &TsConfig, report: &mut Report) {
    report.should(
        "tsconfig-project-references",
        "tsconfig.json: uses project references",
        Outcome::fail(
            "tsconfig.json has no references: it should reference tsconfig.app.json and tsconfig.node.json",
        ),
    );

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
fn check_tsconfig_solution_style(ctx: &ComponentContext, report: &mut Report) {
    // A solution-style config has references, which the context resolves, each to a config file.
    let referenced = ctx.typescript_referenced_configs();
    report.should(
        "tsconfig-project-references",
        "tsconfig.json: uses project references",
        check_references(referenced.iter().map(|(name, _)| name.as_str())),
    );

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
            ConfigStatus::ParseError { error } => {
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

/// The configs a solution-style `tsconfig.json` references: the application's, and the tooling files'.
const REFERENCED_CONFIGS: [&str; 2] = ["tsconfig.app.json", "tsconfig.node.json"];

/// Check that a solution-style `tsconfig.json`, referencing the config files named `names`, references each of
/// [`REFERENCED_CONFIGS`], whose roles decide their module settings.
fn check_references<'a>(names: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
    let names: Vec<_> = names.into_iter().collect();
    let missing: Vec<_> = REFERENCED_CONFIGS
        .into_iter()
        .filter(|config| !names.contains(config))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "tsconfig.json doesn't reference {}",
            missing.join(" or ")
        ))
    }
}

/// Check compiler options common to both single and solution-style configs.
///
/// The `label` is used as a prefix in check names so that solution-style configs produce per-reference results (e.g.
/// "tsconfig.app.json: strict mode enabled").
fn check_compiler_options(label: &str, opts: &TsCompilerOptions, report: &mut Report) {
    // Strict mode
    report.should(
        "tsconfig-strict",
        format!("{label}: strict mode enabled"),
        opts.strict == Some(true),
    );

    // noUncheckedIndexedAccess
    check_enabled(
        "tsconfig-no-unchecked-indexed-access",
        label,
        "noUncheckedIndexedAccess",
        opts.no_unchecked_indexed_access,
        report,
    );

    // Module settings, which depend on what loads the config's files
    let role = ConfigRole::of(label);
    report.should(
        "tsconfig-module",
        format!("{label}: module is {}", role.module()),
        option_is("module", opts.module.as_deref(), role.module()),
    );
    report.should(
        "tsconfig-module-resolution",
        format!("{label}: {}", role.module_resolution_name()),
        role.check_module_resolution(opts.module_resolution.as_deref()),
    );
    if role == ConfigRole::Node {
        // Node loads the tooling files' relative imports by the names of the TypeScript files.
        check_enabled(
            "tsconfig-allow-importing-ts-extensions",
            label,
            "allowImportingTsExtensions",
            opts.allow_importing_ts_extensions,
            report,
        );
    }

    // ECMAScript version, which may be left to TypeScript's default, the newest
    report.should(
        "tsconfig-target",
        format!("{label}: target is ES{MIN_ES_YEAR} or newer"),
        check_target(opts.target.as_deref()),
    );
    report.should(
        "tsconfig-lib",
        format!("{label}: lib includes ES{MIN_ES_YEAR} or newer"),
        check_lib(opts.lib.as_deref()),
    );

    // verbatimModuleSyntax
    check_enabled(
        "tsconfig-verbatim-module-syntax",
        label,
        "verbatimModuleSyntax",
        opts.verbatim_module_syntax,
        report,
    );

    // Checks of unused code and unsafe imports and switches
    for (id, option, value) in [
        (
            "tsconfig-no-unchecked-side-effect-imports",
            "noUncheckedSideEffectImports",
            opts.no_unchecked_side_effect_imports,
        ),
        (
            "tsconfig-no-fallthrough-cases-in-switch",
            "noFallthroughCasesInSwitch",
            opts.no_fallthrough_cases_in_switch,
        ),
        (
            "tsconfig-no-unused-locals",
            "noUnusedLocals",
            opts.no_unused_locals,
        ),
        (
            "tsconfig-no-unused-parameters",
            "noUnusedParameters",
            opts.no_unused_parameters,
        ),
    ] {
        check_enabled(id, label, option, value, report);
    }

    report.should(
        "tsconfig-module-detection",
        format!("{label}: moduleDetection is force"),
        check_module_detection(opts.module_detection.as_deref()),
    );

    // erasableSyntaxOnly, which rejects the syntax that Node's type stripping can't remove
    check_enabled(
        "tsconfig-erasable-syntax-only",
        label,
        "erasableSyntaxOnly",
        opts.erasable_syntax_only,
        report,
    );
}

/// Check that the boolean compiler option `option`, whose value in the config labeled `label` is `value`, is enabled.
fn check_enabled(id: &str, label: &str, option: &str, value: Option<bool>, report: &mut Report) {
    report.should(
        id,
        format!("{label}: {option} enabled"),
        option_enabled(option, value),
    );
}

/// Whether the boolean compiler option `option`, whose value is `value`, is enabled.
fn option_enabled(option: &str, value: Option<bool>) -> Result<(), String> {
    match value {
        Some(true) => Ok(()),
        Some(false) => Err(format!("{option} is false")),
        None => Err(format!("Missing {option}")),
    }
}

/// Check that `moduleDetection` is `force`, so that every file is a module.
fn check_module_detection(value: Option<&str>) -> Result<(), String> {
    option_is("moduleDetection", value, "force")
}

/// Whether the string compiler option `option`, whose value is `value`, is `expected`. TypeScript reads these values
/// case-insensitively.
fn option_is(option: &str, value: Option<&str>, expected: &str) -> Result<(), String> {
    match value {
        Some(value) if value.eq_ignore_ascii_case(expected) => Ok(()),
        Some(value) => Err(format!("{option} is {value:?}, not \"{expected}\"")),
        None => Err(format!("Missing {option}")),
    }
}

/// The oldest ECMAScript version, as a year, that a tsconfig's `target` and `lib` may name. The standards choose the
/// newest version the code's runtime fully supports, which norms can't tell, so it only rejects versions older than
/// any such runtime.
const MIN_ES_YEAR: u32 = 2022;

/// The ECMAScript version that a `target` or `lib` value names, as the year of its edition, with `esnext` newer than
/// any: `None` for a value that names none, such as `dom`, or only part of a version's library, such as
/// `es2023.array`. TypeScript reads the values case-insensitively.
fn es_year(value: &str) -> Option<u32> {
    let value = value.to_ascii_lowercase();
    match value.strip_prefix("es")? {
        "next" => Some(u32::MAX),
        "3" => Some(1999),
        "5" => Some(2009),
        "6" => Some(2015),
        "7" => Some(2016),
        year => year.parse().ok().filter(|&year| year >= 2015),
    }
}

/// Check that `target`, if set, names ECMAScript [`MIN_ES_YEAR`] or newer. Left unset, it defaults to the newest
/// version TypeScript knows, since TypeScript 6, which the standards use.
fn check_target(target: Option<&str>) -> Result<(), String> {
    match target.and_then(|target| Some((target, es_year(target)?))) {
        Some((target, year)) if year < MIN_ES_YEAR => {
            Err(format!("target is {target:?}, older than ES{MIN_ES_YEAR}"))
        }
        _ => Ok(()),
    }
}

/// Check that `lib`, if set, includes ECMAScript [`MIN_ES_YEAR`] or newer, which decides the APIs that code may use.
/// Left unset, it follows `target`. A `lib` naming no version, which TypeScript can't check code with, isn't judged.
fn check_lib(lib: Option<&[String]>) -> Result<(), String> {
    let newest = lib
        .into_iter()
        .flatten()
        .filter_map(|entry| Some((entry, es_year(entry)?)))
        .max_by_key(|&(_, year)| year);
    match newest {
        Some((entry, year)) if year < MIN_ES_YEAR => Err(format!(
            "lib's newest ECMAScript version is {entry:?}, older than ES{MIN_ES_YEAR}"
        )),
        _ => Ok(()),
    }
}

/// What loads the files a tsconfig covers, which decides how it resolves their imports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigRole {
    /// Code that Vite bundles, such as the application's: `module` is `esnext`, with `bundler` resolution.
    Bundled,
    /// Tooling files, such as `vite.config.ts`, which Node loads itself: `module` is `nodenext`, which sets the
    /// resolution to match, and relative imports name the TypeScript files, which `allowImportingTsExtensions` lets
    /// TypeScript accept.
    Node,
}

impl ConfigRole {
    /// The role of the config file named `name`: `tsconfig.node.json` covers the tooling files, and any other config
    /// code that Vite bundles.
    fn of(name: &str) -> Self {
        if name == "tsconfig.node.json" {
            Self::Node
        } else {
            Self::Bundled
        }
    }

    /// The `module` setting of a config of this role.
    const fn module(self) -> &'static str {
        match self {
            Self::Bundled => "esnext",
            Self::Node => "nodenext",
        }
    }

    /// The name of the `moduleResolution` check of a config of this role.
    const fn module_resolution_name(self) -> &'static str {
        match self {
            Self::Bundled => "moduleResolution is bundler",
            Self::Node => "moduleResolution unset, following module",
        }
    }

    /// Check a config's `moduleResolution`, `value`: `bundler` for bundled code, and for tooling files unset, so that
    /// it follows `module`.
    fn check_module_resolution(self, value: Option<&str>) -> Result<(), String> {
        match (self, value) {
            (Self::Bundled, value) => option_is("moduleResolution", value, "bundler"),
            (Self::Node, None) => Ok(()),
            (Self::Node, Some(value)) => Err(format!(
                "moduleResolution is {value:?}: leave it unset, so that it follows module \"nodenext\""
            )),
        }
    }
}

//
// Linting
//

/// typescript-eslint's `strictTypeChecked` config, as `eslint.config.ts` includes it, such as
/// `tseslint.configs.strictTypeChecked`. The variant ending in `Only`, which leaves out the rules of the configs it
/// builds on, doesn't count.
static STRICT_TYPE_CHECKED: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"\bstrictTypeChecked\b").unwrap()
});

/// typescript-eslint's `stylisticTypeChecked` config, as for [`STRICT_TYPE_CHECKED`].
static STYLISTIC_TYPE_CHECKED: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"\bstylisticTypeChecked\b").unwrap()
});

/// `projectService` enabled in the parser options, as `true` or an object of its options.
static PROJECT_SERVICE: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r"\bprojectService\s*:\s*(?:true\b|\{)").unwrap()
});

/// An import of `eslint-config-prettier`, or of its `flat` entry point.
static CONFIG_PRETTIER_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r#"\bfrom\s*["']eslint-config-prettier(?:/flat)?["']"#).unwrap()
});

/// The text of `eslint.config.ts` without its comments, so that a commented-out config, or one named in a note, doesn't
/// count. Strings are kept whole, so that a `//` in a URL doesn't start a comment; each comment becomes a space, or the
/// line breaks it spans. Regular expression literals aren't recognized, which a configuration rarely has.
fn eslint_config_code(text: &str) -> String {
    let mut code = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '/' if chars.peek() == Some(&'/') => {
                while chars.next_if(|&next| next != '\n').is_some() {}
                code.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = ' ';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    if next == '\n' {
                        code.push('\n');
                    }
                    previous = next;
                }
                code.push(' ');
            }
            '"' | '\'' | '`' => {
                code.push(c);
                while let Some(next) = chars.next() {
                    code.push(next);
                    if next == '\\' {
                        if let Some(escaped) = chars.next() {
                            code.push(escaped);
                        }
                    } else if next == c {
                        break;
                    }
                }
            }
            _ => code.push(c),
        }
    }
    code
}

/// Check that `eslint.config.ts` imports `eslint-config-prettier`, which is also a dependency, if the
/// dependencies are known.
fn check_config_prettier(code: &str, in_dependencies: Option<bool>) -> Result<(), Issue> {
    if !CONFIG_PRETTIER_IMPORT.is_match(code) {
        Err(Issue::warn(
            "eslint.config.ts doesn't import eslint-config-prettier",
        ))
    } else if in_dependencies == Some(false) {
        // ESLint can't load its configuration then.
        Err(Issue::fail("eslint-config-prettier isn't in dependencies"))
    } else {
        Ok(())
    }
}

pub(crate) fn check_eslint(ctx: &ComponentContext, report: &mut Report) {
    // An eslint.config.ts that can't be read, such as a directory, fails here, rather than leaving out the checks of
    // its contents.
    if let Some(text) =
        report.require_parsed("eslint-config", "eslint.config.ts", ctx.eslint_config())
    {
        let code = eslint_config_code(text);
        for (id, name, pattern, message) in [
            (
                "eslint-strict-type-checked",
                "ESLint: includes typescript-eslint's strictTypeChecked",
                &STRICT_TYPE_CHECKED,
                "Doesn't include strictTypeChecked",
            ),
            (
                "eslint-stylistic-type-checked",
                "ESLint: includes typescript-eslint's stylisticTypeChecked",
                &STYLISTIC_TYPE_CHECKED,
                "Doesn't include stylisticTypeChecked",
            ),
            (
                "eslint-project-service",
                "ESLint: projectService enabled for type-aware rules",
                &PROJECT_SERVICE,
                "Doesn't enable projectService in the parser options",
            ),
        ] {
            report.should(
                id,
                name,
                if pattern.is_match(&code) {
                    Ok(())
                } else {
                    Err(message.to_owned())
                },
            );
        }
        report.check(
            "eslint-config-prettier",
            "ESLint: eslint-config-prettier turns off rules Prettier covers",
            check_config_prettier(
                &code,
                ctx.package_json()
                    .as_ref_ok()
                    .map(|pkg| pkg.has_dep("eslint-config-prettier")),
            ),
        );
    }

    // Check for ESLint in dependencies
    if let Some(pkg) = ctx.package_json().as_ref_ok() {
        report.must(
            "eslint-dependency",
            "ESLint: in dependencies",
            pkg.has_dep("eslint"),
        );

        // Check for typescript-eslint
        let has_ts_eslint =
            pkg.has_dep("typescript-eslint") || pkg.has_dep_starting_with("@typescript-eslint/");
        report.must(
            "eslint-typescript-eslint",
            "ESLint: typescript-eslint configured",
            if has_ts_eslint {
                Outcome::Pass
            } else {
                Outcome::fail("typescript-eslint isn't in dependencies")
            },
        );
    }
}

//
// Formatting
//

/// The Prettier plugin that formats `package.json`, sorting its dependencies.
const PACKAGEJSON_PLUGIN: &str = "prettier-plugin-packagejson";

/// The line length the standards set for Prettier, as for Ruff's `line-length`.
const PRETTIER_PRINT_WIDTH: u64 = 120;

pub(crate) fn check_prettier(ctx: &ComponentContext, report: &mut Report) {
    let config = report.require_parsed("prettier-config", ".prettierrc", ctx.prettier());

    // .prettierignore
    report.must(
        "prettier-ignore-exists",
        "Prettier: .prettierignore exists",
        file_exists(&ctx.path().join(".prettierignore")),
    );

    // Prettier in dependencies
    let package_json = ctx.package_json().as_ref_ok();
    if let Some(pkg) = package_json {
        report.must(
            "prettier-dependency",
            "Prettier: in dependencies",
            pkg.has_dep("prettier"),
        );
    }

    let Some(config) = config else {
        return;
    };

    // Line length, as for Ruff
    report.should(
        "prettier-config-print-width",
        format!(".prettierrc: printWidth is {PRETTIER_PRINT_WIDTH}"),
        config.check_print_width(PRETTIER_PRINT_WIDTH),
    );

    // package.json formatting, which sorts its dependencies
    report.check(
        "prettier-plugin-packagejson",
        "Prettier: formats package.json with prettier-plugin-packagejson",
        if !config.has_plugin(PACKAGEJSON_PLUGIN) {
            Err(Issue::warn(format!(
                "{PACKAGEJSON_PLUGIN} isn't in .prettierrc's plugins"
            )))
        } else if package_json.is_some_and(|pkg| !pkg.has_dep(PACKAGEJSON_PLUGIN)) {
            // Prettier can't load its configuration then.
            Err(Issue::fail(format!(
                "{PACKAGEJSON_PLUGIN} isn't in dependencies"
            )))
        } else {
            Ok(())
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
            Outcome::fail("Missing 'build' script")
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
                || Err("Missing 'lint' script".to_owned()),
                |script| lint_script_strict(script),
            ),
    );

    // Format script
    report.check(
        "package-json-script-format",
        "Scripts: has 'format' (prettier --check)",
        config
            .scripts
            .as_ref()
            .and_then(|scripts| scripts.get("format"))
            .map_or_else(
                || Err(Issue::warn("Missing 'format' script")),
                |script| format_script_checks(script),
            ),
    );

    // Type check script
    report.should(
        "package-json-script-typecheck",
        "Scripts: has 'typecheck'",
        if config.has_script("typecheck") {
            Outcome::Pass
        } else {
            Outcome::fail("Missing 'typecheck' script")
        },
    );

    // Test script
    report.should(
        "package-json-script-test",
        "Scripts: has 'test'",
        if config.has_script("test") {
            Outcome::Pass
        } else {
            Outcome::fail("Missing 'test' script")
        },
    );
}

//
// Pre-commit integration
//

/// The IDs of TypeScript's language hooks, which the standard's hook table lists.
const LANGUAGE_HOOKS: [&str; 7] = [
    "pnpm-lock",
    "pnpm-peer-dependencies",
    "prettier",
    "eslint",
    "tsc",
    "vite-build",
    "vitest",
];

pub(crate) fn check_precommit_hooks(ctx: &ProjectContext, report: &mut Report) {
    let Some(precommit) = ctx.precommit().as_ref_ok() else {
        return;
    };

    report.check(
        "typescript-precommit-pnpm-lock",
        "Pre-commit: has pnpm-lock hook",
        precommit.check_commit_hook("pnpm-lock"),
    );
    report.check(
        "typescript-precommit-pnpm-peer-dependencies",
        "Pre-commit: has pnpm-peer-dependencies hook",
        precommit.check_commit_hook("pnpm-peer-dependencies"),
    );
    report.check(
        "typescript-precommit-eslint",
        "Pre-commit: has ESLint hook (--max-warnings 0)",
        precommit.commit_hooks("eslint").and_then(|hooks| {
            if hooks.into_iter().any(eslint_hook_strict) {
                Ok(())
            } else {
                Err(Issue::warn("The ESLint hook doesn't pass --max-warnings 0"))
            }
        }),
    );
    report.check(
        "typescript-precommit-prettier",
        "Pre-commit: has Prettier hook (--check)",
        precommit.check_formatter_hook("prettier", "prettier", prettier_run),
    );
    report.check(
        "typescript-precommit-tsc",
        "Pre-commit: has TypeScript type check hook",
        precommit.check_commit_hook("tsc"),
    );
    report.check(
        "typescript-precommit-vite-build",
        "Pre-commit: has vite-build hook",
        precommit.check_commit_hook("vite-build"),
    );
    report.check(
        "typescript-precommit-vitest",
        "Pre-commit: has vitest hook",
        precommit.check_commit_hook("vitest"),
    );
    report.should(
        "typescript-precommit-local-hooks",
        "TypeScript pre-commit: language hooks are local, unsupported and take no file names",
        precommit.check_language_hooks(&LANGUAGE_HOOKS),
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
    report.must(
        "typescript-ci-pnpm-peer-dependencies",
        "TypeScript CI: peer dependencies",
        workflow
            .run_lines()
            .any(|line| commands(&line).any(|words| checks_peer_dependencies(&words))),
    );

    // Lint step
    report.check(
        "typescript-ci-lint",
        "TypeScript CI: lint check (--max-warnings 0)",
        eslint_in_ci(workflow),
    );

    // Format check
    report.check(
        "typescript-ci-format",
        "TypeScript CI: format check",
        workflow.check_formatter("prettier", prettier_run),
    );

    // Type check
    report.must(
        "typescript-ci-tsc",
        "TypeScript CI: type check",
        workflow.runs_command("tsc"),
    );

    // Tests
    report.must(
        "typescript-ci-test",
        "TypeScript CI: tests",
        workflow.runs_command("vitest")
            || workflow.runs_command("pnpm test")
            || workflow.runs_command("pnpm run test"),
    );

    // Build
    report.must(
        "typescript-ci-vite-build",
        "TypeScript CI: Vite build",
        workflow.runs_matching(&VITE_BUILD),
    );
}

/// Whether a command's words run `prettier`, and if so how: with `--check`, which fails on unformatted files instead
/// of rewriting them, with its short form `-c` or with `--list-different` (or `-l`), which fail on them too, with
/// `--write` (or `-w`), which rewrites them, or with none of them, when Prettier only prints the formatted files.
fn prettier_run(words: &[&str]) -> Option<FormatterRun> {
    let args = program_args(words, "prettier")?;
    let run = FormatterRun::from_args(args, &["--check"], &["-c", "--list-different", "-l"]);
    Some(
        if run == FormatterRun::Rewrites && !args.iter().any(|&arg| arg == "--write" || arg == "-w")
        {
            FormatterRun::Prints
        } else {
            run
        },
    )
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

/// Whether a `format` script checks the formatting rather than rewriting files: each command of it that runs
/// `prettier` does so with `--check`. A script that doesn't run `prettier` itself, such as one running another script,
/// isn't judged.
fn format_script_checks(script: &str) -> Result<(), Issue> {
    let runs: Vec<_> = commands(script)
        .filter_map(|words| prettier_run(&words))
        .collect();
    FormatterRun::check_each(
        &runs,
        "prettier",
        Issue::warn("The format script runs prettier without --check, so it rewrites files"),
    )
}

/// Whether an `eslint` hook passes `--max-warnings 0`, in its entry or the args pre-commit appends to it. A hook without
/// an entry, as a remote hook may leave it to the hook's repository, is taken to run `eslint`.
fn eslint_hook_strict(hook: &PrecommitHook) -> bool {
    hook.filter_map_commands("eslint", eslint_max_warnings_zero)
        .contains(&true)
}

/// Check that some step runs `eslint` with `--max-warnings 0`. A step running the `lint` script doesn't count, since the
/// flag would have to be found in the script of the right component.
fn eslint_in_ci(workflow: &Workflow) -> Result<(), Issue> {
    let runs = workflow.filter_map_commands(eslint_max_warnings_zero);
    if runs.contains(&true) {
        Ok(())
    } else if !runs.is_empty() {
        Err(Issue::warn("ESLint runs without --max-warnings 0"))
    } else if workflow.runs_command("pnpm lint") || workflow.runs_command("pnpm run lint") {
        Err(Issue::warn(
            "CI runs the lint script rather than eslint with --max-warnings 0",
        ))
    } else {
        Err(Issue::fail("No step runs eslint"))
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

    report.must(
        "typescript-audit-pnpm",
        "TypeScript audit: pnpm audit",
        workflow.runs_command("pnpm audit"),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boolean_compiler_options_are_enabled() {
        assert_eq!(option_enabled("erasableSyntaxOnly", Some(true)), Ok(()));
        assert_eq!(
            option_enabled("erasableSyntaxOnly", Some(false)),
            Err("erasableSyntaxOnly is false".to_owned())
        );
        assert_eq!(
            option_enabled("erasableSyntaxOnly", None),
            Err("Missing erasableSyntaxOnly".to_owned())
        );
    }

    #[test]
    fn module_detection_is_force_in_any_case() {
        for value in ["force", "Force"] {
            assert_eq!(check_module_detection(Some(value)), Ok(()), "{value}");
        }
        assert_eq!(
            check_module_detection(Some("auto")),
            Err("moduleDetection is \"auto\", not \"force\"".to_owned())
        );
        assert_eq!(
            check_module_detection(None),
            Err("Missing moduleDetection".to_owned())
        );
    }

    #[test]
    fn single_tsconfig_warns() {
        let config: TsConfig = serde_json::from_str(
            r#"{"compilerOptions": {"strict": true, "module": "esnext", "moduleResolution": "bundler"}}"#,
        )
        .unwrap();
        let mut report = Report::new(std::path::PathBuf::new(), Vec::new());
        check_tsconfig_single(&config, &mut report);
        let warned: Vec<_> = report
            .results
            .iter()
            .filter(|result| result.status == crate::models::Status::Warn)
            .map(|result| result.id.as_str())
            .collect();
        assert!(
            warned.contains(&"tsconfig-project-references"),
            "{warned:?}"
        );
        // The config's own options are still checked.
        assert!(
            report
                .results
                .iter()
                .any(|result| result.id == "tsconfig-module"
                    && result.status == crate::models::Status::Pass),
            "{:?}",
            report.results
        );

        assert_eq!(
            check_references([
                "tsconfig.app.json",
                "tsconfig.node.json",
                "tsconfig.test.json"
            ]),
            Ok(())
        );
        assert_eq!(
            check_references(["tsconfig.app.json"]),
            Err("tsconfig.json doesn't reference tsconfig.node.json".to_owned())
        );
        assert_eq!(
            check_references([]),
            Err(
                "tsconfig.json doesn't reference tsconfig.app.json or tsconfig.node.json"
                    .to_owned()
            )
        );
    }

    #[test]
    fn target_and_lib_are_es2022_or_newer() {
        for (value, year) in [
            ("ES2022", Some(2022)),
            ("es2025", Some(2025)),
            ("ESNext", Some(u32::MAX)),
            ("ES6", Some(2015)),
            ("es7", Some(2016)),
            ("ES5", Some(2009)),
            ("es2023.array", None),
            ("DOM", None),
            ("esnext.disposable", None),
        ] {
            assert_eq!(es_year(value), year, "{value}");
        }

        for target in [None, Some("ES2022"), Some("esnext"), Some("unknown")] {
            assert_eq!(check_target(target), Ok(()), "{target:?}");
        }
        assert_eq!(
            check_target(Some("ES2020")),
            Err("target is \"ES2020\", older than ES2022".to_owned())
        );

        let lib = |entries: &[&str]| {
            check_lib(Some(
                &entries
                    .iter()
                    .map(|&entry| entry.to_owned())
                    .collect::<Vec<_>>(),
            ))
        };
        assert_eq!(check_lib(None), Ok(()));
        assert_eq!(lib(&["ES2022", "DOM", "DOM.Iterable"]), Ok(()));
        assert_eq!(lib(&["ES2020", "ES2025"]), Ok(()));
        assert_eq!(lib(&["DOM"]), Ok(()));
        assert_eq!(
            lib(&["ES2020", "ES2023.Array", "DOM"]),
            Err("lib's newest ECMAScript version is \"ES2020\", older than ES2022".to_owned())
        );
    }

    #[test]
    fn module_settings_follow_the_config_role() {
        assert_eq!(ConfigRole::of("tsconfig.app.json"), ConfigRole::Bundled);
        assert_eq!(ConfigRole::of("tsconfig.json"), ConfigRole::Bundled);
        assert_eq!(ConfigRole::of("tsconfig.node.json"), ConfigRole::Node);

        assert_eq!(option_is("module", Some("ESNext"), "esnext"), Ok(()));
        assert_eq!(
            option_is("module", Some("ES2022"), "esnext"),
            Err("module is \"ES2022\", not \"esnext\"".to_owned())
        );

        let bundled = |value| ConfigRole::Bundled.check_module_resolution(value);
        assert_eq!(bundled(Some("Bundler")), Ok(()));
        assert_eq!(
            bundled(Some("nodenext")),
            Err("moduleResolution is \"nodenext\", not \"bundler\"".to_owned())
        );
        assert_eq!(bundled(None), Err("Missing moduleResolution".to_owned()));

        let node = |value| ConfigRole::Node.check_module_resolution(value);
        assert_eq!(node(None), Ok(()));
        assert_eq!(
            node(Some("NodeNext")),
            Err(
                "moduleResolution is \"NodeNext\": leave it unset, so that it follows module \"nodenext\""
                    .to_owned()
            )
        );
        assert_eq!(
            node(Some("bundler")),
            Err(
                "moduleResolution is \"bundler\": leave it unset, so that it follows module \"nodenext\""
                    .to_owned()
            )
        );
    }

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
            Err(Issue::warn("ESLint runs without --max-warnings 0"))
        );
        assert_eq!(
            eslint_in_ci(&workflow("pnpm run lint")),
            Err(Issue::warn(
                "CI runs the lint script rather than eslint with --max-warnings 0"
            ))
        );
        assert_eq!(
            eslint_in_ci(&workflow("pnpm exec tsc -b")),
            Err(Issue::fail("No step runs eslint"))
        );
    }

    #[test]
    fn eslint_config_includes_the_typed_configs() {
        let code = eslint_config_code(
            "export default defineConfig(\n    ...tseslint.configs.strictTypeChecked,\n    \
             tseslint.configs.stylisticTypeCheckedOnly,\n    // tseslint.configs.stylisticTypeChecked,\n);\n",
        );
        assert!(STRICT_TYPE_CHECKED.is_match(&code));
        assert!(!STYLISTIC_TYPE_CHECKED.is_match(&code));
        assert!(!STRICT_TYPE_CHECKED.is_match("tseslint.configs.strictTypeCheckedOnly"));

        for text in [
            "parserOptions: { projectService: true }",
            "projectService:true,",
            "projectService: {\n    allowDefaultProject: [\"*.js\"],\n}",
        ] {
            assert!(PROJECT_SERVICE.is_match(text), "{text}");
        }
        for text in [
            "parserOptions: { project: true }",
            "projectService: false",
            "// projectService: true",
            "project: true, // not projectService: true yet",
            "/* projectService: true,\n */ project: true",
        ] {
            assert!(
                !PROJECT_SERVICE.is_match(&eslint_config_code(text)),
                "{text}"
            );
        }
    }

    #[test]
    fn eslint_config_code_drops_comments_but_not_strings() {
        assert_eq!(
            eslint_config_code("a, // b\nc, /* d\ne */ f, /**/g"),
            "a,  \nc, \n  f,  g"
        );
        for text in [
            "url: \"https://example.com\", x",
            "glob: '**/*.js', x",
            "s: `a /* b */ c`, x",
            "q: \"a \\\" // b\", x",
        ] {
            assert_eq!(eslint_config_code(text), text, "{text}");
        }
        assert_eq!(eslint_config_code("/* never closed"), " ");
    }

    #[test]
    fn eslint_config_imports_eslint_config_prettier() {
        for code in [
            "import prettier from \"eslint-config-prettier\";",
            "import prettier from 'eslint-config-prettier/flat';",
        ] {
            assert_eq!(check_config_prettier(code, Some(true)), Ok(()), "{code}");
            assert_eq!(check_config_prettier(code, None), Ok(()), "{code}");
        }
        assert_eq!(
            check_config_prettier(
                "import prettier from \"eslint-config-prettier\";",
                Some(false)
            ),
            Err(Issue::fail("eslint-config-prettier isn't in dependencies"))
        );
        for code in [
            "import prettier from \"eslint-plugin-prettier/recommended\";",
            "import prettier from \"eslint-config-prettier-extended\";",
        ] {
            assert_eq!(
                check_config_prettier(code, Some(true)),
                Err(Issue::warn(
                    "eslint.config.ts doesn't import eslint-config-prettier"
                )),
                "{code}"
            );
        }
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

    #[test]
    fn format_script_checks_with_check() {
        for script in [
            "prettier --check .",
            "prettier --check . && prettier --check docs",
            "pnpm run format:all",
        ] {
            assert_eq!(format_script_checks(script), Ok(()), "{script}");
        }
        assert_eq!(
            format_script_checks("prettier --write ."),
            Err(Issue::warn(
                "The format script runs prettier without --check, so it rewrites files"
            ))
        );
        assert_eq!(
            format_script_checks("prettier --check . && prettier -l docs"),
            Err(Issue::warn("prettier runs with -l rather than --check"))
        );
        assert_eq!(
            format_script_checks("prettier ."),
            Err(Issue::fail(
                "prettier runs without --check or --write, so it only prints the formatted files and fails on none"
            ))
        );
    }

    #[test]
    fn prettier_in_ci_checks_rather_than_writes() {
        let checks = |line: &str| prettier_run(&line.split_whitespace().collect::<Vec<_>>());
        for line in [
            "pnpm exec prettier --check .",
            "node_modules/.bin/prettier . --check",
        ] {
            assert_eq!(checks(line), Some(FormatterRun::Checks), "{line}");
        }
        assert_eq!(
            checks("prettier -c src"),
            Some(FormatterRun::ChecksWith("-c"))
        );
        assert_eq!(
            checks("prettier --list-different ."),
            Some(FormatterRun::ChecksWith("--list-different"))
        );
        assert_eq!(
            checks("pnpm exec prettier -l ."),
            Some(FormatterRun::ChecksWith("-l"))
        );
        for line in ["pnpm exec prettier --write .", "prettier -w src"] {
            assert_eq!(checks(line), Some(FormatterRun::Rewrites), "{line}");
        }
        assert_eq!(checks("prettier ."), Some(FormatterRun::Prints));
        assert_eq!(
            FormatterRun::check(&[FormatterRun::Prints], "prettier", Issue::warn("rewrites")),
            Err(Issue::fail(
                "prettier runs without --check or --write, so it only prints the formatted files and fails on none"
            ))
        );
        assert_eq!(
            FormatterRun::check(
                &[FormatterRun::Prints, FormatterRun::Rewrites],
                "prettier",
                Issue::warn("rewrites")
            ),
            Err(Issue::warn("rewrites"))
        );
        for line in [
            "pnpm format",
            "pnpm exec eslint --check .",
            "echo prettier-check",
        ] {
            assert_eq!(checks(line), None, "{line}");
        }
    }
}
