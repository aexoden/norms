# Project Standards

This file documents the standards intended to be used by my projects. Some of these
standards are enforced by the `norms` tool. These standards are highly opinionated
and may not be appropriate for all projects. In developing these standards, there
were several guiding principles:

1. Humans shouldn't manually enforce things that tools can check.
2. Apply maximum strictness by default, only relaxing selectively with documented
   rationale.
3. Any developer should be able to build and run the project identically.

## Development Environment

Each language's own toolchain manager provides the development environment. Tool
versions are pinned in files committed to the repository, so every developer and every
CI run uses identical versions without any additional environment layer:

| Language   | Required tool | Pinned by                                                             |
|------------|---------------|-----------------------------------------------------------------------|
| Rust       | rustup        | `rust-toolchain.toml` (exact channel and required components)         |
| Python     | uv            | `.python-version`, `requires-python` and `[tool.uv] required-version` |
| TypeScript | pnpm          | `devEngines` in `package.json` (Node.js and pnpm versions)            |

The language-specific sections below describe each of these files in detail.

[pre-commit](https://pre-commit.com/) must also be installed, by whatever method the
developer prefers. See the [pre-commit installation documentation](https://pre-commit.com/#install).

### Devbox

Use [Devbox](https://www.jetify.com/devbox) only where it provides a clear benefit
that the language toolchains cannot. At present, the only such use is running local
services, such as a development PostgreSQL server via `devbox services`.

When Devbox is used:

- `devbox.json` includes a `$schema` reference and `devbox.lock` is committed.
- Packages are limited to the services and the tools needed to manage them (for
  example, `postgresql` and `sqlx-cli`). Language toolchains are not installed through
  Devbox.
- Service setup steps (initializing and creating databases, for example) are provided
  as Devbox scripts and documented in the README.
- Pre-commit hooks and CI steps invoke tools directly. Only steps that need a running
  service may depend on Devbox.

## Editor Configuration

Include an `.editorconfig` file at the repository root. The standard configuration:

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
indent_style = space
indent_size = 4
insert_final_newline = true
trim_trailing_whitespace = true

[*.{yml,yaml,json,toml}]
indent_size = 2
```

Additional project-specific options can be included if required.

## README

Projects should contain a `README.md` file that gives an overview of the project.

## Licensing

Projects should have one or more license files, defaulting to `LICENSE` in the
root of the repository in the case of a single license. More complicated licensing
schemes should be documented in the README and multiple licenses can be suffixed
with `-<description>` to further identify them.

By default, projects should be under an MIT-style license unless another license
is required because of code heritage. Rust projects, if possible, should dual-license
under both MIT and Apache 2.0.

## Version Control

Use `git` as the version control system. Both `.gitattributes` and `.gitignore`
should be present and configured as appropriate for the project.

Additional guidance may be added here at a later date.

### Commit Messages

Commit messages should follow a [Conventional Commits](https://www.conventionalcommits.org/)-
like standard. Exact details and definitions are still under consideration. Some
ideas under consideration can be found at the [Conventional Commits Cheatsheet](https://gist.github.com/qoomon/5dfcdf8eec66a051ecd85625518cfd13).

The standard template for a commit message:

```txt
<type>(<optional scope>): <subject>

[optional body]

[optional footer]
```

The following types are currently recognized, but as stated above, this list is
not set in stone:

- `feat` - adds, changes or removes a feature
- `fix` - fixes one or more bugs
- `perf` - improves performance
- `refactor` - rewrites or restructures code without altering behavior
- `style` - addresses code style without altering behavior
- `test` - adds missing tests or corrects existing ones
- `docs` - adds or updates documentation only
- `build` - affects build-related components
- `ops` - affects devops, CI/CD or infrastructure
- `chore` - anything not otherwise mentioned

Strictly speaking, dependency updates could fall under many of these, intentionally
or not. By default, renovate (discussed later) will put these under `chore(deps)`,
however. It may be desirable to revisit that default.

Scopes are inherently project-specific. A good practice for mature projects would
be to identify allowed scopes in developer documentation.

The subject should be in the imperative mood, lowercase (except proper nouns, acronyms,
or otherwise conventionally capitalized words), with no concluding punctuation.
The entire first line should be no more than 50 characters (though this rule seems
to lead to nearly useless subjects, so this may need to be revisited).

The optional body, if present, should document the motivation for the change and
identify differences from existing behavior. Avoid documenting things that are either
self-explanatory from the code or from comments within the code. The body should
be wrapped at 72 characters (again, I personally find this needlessly anachronistic
so it may be revisited).

### Branch Naming

Branch names should follow the pattern `<type>/<short-description>`. Use the same
types as listed above.

## Continuous Integration

Continuous integration is provided by a GitHub Actions configuration, preferably
stored at `.github/workflows/ci.yaml`.

Recommended jobs include linting and formatting checks, type checking, tests with
coverage and build verification.

## Pre-Commit Hooks

Pre-commit hooks should be provided by [pre-commit](https://pre-commit.com/).

Additional guidance may be added later.

## Dependency Management

### Automated Updates

Use [Renovate](https://docs.renovatebot.com/) for automated dependency updates. The
shared [renovate-config](https://github.com/aexoden/renovate-config) preset builds on
`config:best-practices` and provides the automerge, grouping, pinning and release age
policies, so `renovate.json` normally contains nothing else:

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": ["github>aexoden/renovate-config"]
}
```

Project-specific rules may be added when a project needs them.

### Version Pinning

Every project commits its lock files, as each tool recommends. Applications also pin
every direct dependency, including development dependencies, to an exact version:

| Ecosystem | Manifest syntax                      | Lock file        |
|-----------|--------------------------------------|------------------|
| Cargo     | `serde = "=1.0.228"`                 | `Cargo.lock`     |
| Python    | `"pydantic==2.12.0"`                 | `uv.lock`        |
| pnpm      | `"react": "19.2.0"` (no `^` or `~`)  | `pnpm-lock.yaml` |

Libraries instead declare compatible version ranges, so that downstream projects can
resolve shared dependencies to a single version. Their lock files are still committed
for reproducible development and CI.

### Release Age

A new release must be at least three days old before a project adopts it. The delay
gives the community time to catch and yank compromised or broken releases before they
reach a project. It is enforced at two levels:

1. **Renovate**, through `minimumReleaseAge` in the shared preset.
2. **The package manager**, which covers what Renovate cannot age-check, such as lock
   file maintenance and manual updates:
   - uv: `exclude-newer = "3 days"` under `[tool.uv]` in `pyproject.toml`.
   - pnpm: `minimumReleaseAge: 4320` (in minutes) in `pnpm-workspace.yaml`.
   - Cargo: `global-min-publish-age = "3 days"` under `[registry]` in
     `.cargo/config.toml`. This is stable as of Rust 1.100; earlier toolchains silently
     ignore it, so it takes effect when the pinned toolchain reaches 1.100.

#### Urgent Updates

Security fixes may bypass the release age requirement. Renovate's vulnerability alert
PRs are exempt from `minimumReleaseAge` in the shared preset. To update a single package
early through the package manager, add a temporary per-package exemption, with a
comment naming the advisory:

- uv: `exclude-newer-package = { package-name = false }` under `[tool.uv]`.
- pnpm: `minimumReleaseAgeExclude: [package-name]` in `pnpm-workspace.yaml`.
- Cargo: there is no per-package setting. Run
  `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo update -p package-name`, and the
  lock file keeps the newer version.

Remove a configured exemption once the release has passed the normal release age.

## Language-Specific Standards

### Python

Use [uv](https://github.com/astral-sh/uv) for all Python projects. The basic project
structure should look like the following:

```txt
project/
├── src/
│   └── package_name/
│       ├── __init__.py
│       └── ...
├── tests/
│   ├── __init__.py
│   └── ...
├── pyproject.toml
├── uv.lock
└── README.md
```

For linting and formatting, use [Ruff](https://docs.astral.sh/ruff/). For type
checking, use [mypy](https://mypy.readthedocs.io/en/stable/) in strict mode.

Eventually, standards for testing will likely be added.
