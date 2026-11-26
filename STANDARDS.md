# Project Standards

These are the standards for the author's projects. The `norms` tool checks most
of them, and its template renders new projects that follow them (see the
[README](README.md)). They rest on three principles:

1. Humans shouldn't enforce what tools can check.
2. Be as strict as possible by default, and relax selectively, with a recorded
   reason.
3. Every developer, and CI, builds and runs the project identically.

A project with good reason to depart from a standard records the exception and
its reason as a waiver in `norms.toml` (see the
[README](README.md#configuration)).

## Applications and Libraries

Some standards differ between applications and libraries:

- A Rust component is an application if it builds a binary (`src/main.rs` or
  `src/bin/`, unless `autobins = false`, or a `[[bin]]` target), even alongside
  a library. A Cargo workspace is an application if any of its members is.
- A Python component is an application if it installs a command (a non-empty
  `[project.scripts]` or `[project.gui-scripts]`), or if uv doesn't build it
  (`[tool.uv] package = false`), as for a service run with `uv run`.
- A TypeScript component is always an application.

Other components are libraries. In short, applications pin exact dependency
versions, while libraries declare compatible ranges and test the oldest
toolchain they support.

## Development Environment

Each language's own toolchain manager provides the development environment.
Tool versions are pinned in committed files, so every developer and every CI run
uses the same ones:

| Language   | Required tool | Pinned by                                                             |
|------------|---------------|-----------------------------------------------------------------------|
| Rust       | rustup        | `rust-toolchain.toml` (exact channel and required components)         |
| Python     | uv            | `.python-version`, `requires-python` and `[tool.uv] required-version` |
| TypeScript | pnpm          | `devEngines` in `package.json` (Node.js and pnpm versions)            |

[prek](https://prek.j178.dev/installation/) must also be installed, by any
method.

### Devbox

Use [Devbox](https://www.jetify.com/devbox) only for what the language
toolchains can't provide, which at present means local services, such as a
development PostgreSQL server run with `devbox services`. When a project uses
it:

- `devbox.json` has a `$schema` reference, and `devbox.lock` is committed.
- Its packages are the services and the tools that manage them (for example,
  `postgresql` and `sqlx-cli`), never language toolchains or their managers. It
  doesn't enable Corepack (`DEVBOX_COREPACK_ENABLED`).
- Service setup steps, such as creating databases, are Devbox scripts,
  documented in the README.
- Git hooks and CI run tools directly. Only steps that need a running service
  use Devbox.

## Editor Configuration

The repository root has this `.editorconfig`:

```ini
root = true

[*]
charset = utf-8
end_of_line = lf
indent_style = space
indent_size = 4
insert_final_newline = true
trim_trailing_whitespace = true

[*.{yml,yaml,json,jsonc,toml}]
indent_size = 2
```

A project with TypeScript adds a section for `.prettierrc`, which has no
extension, since Prettier reads `.editorconfig` too:

```ini
[.prettierrc]
indent_size = 2
```

Further sections may set project-specific options.

## YAML Files

YAML files use the `.yaml` extension, except those whose tools document only a
`.yml` name:

- `.copier-answers.yml`, Copier's answers file.
- `.github/FUNDING.yml` and `.github/ISSUE_TEMPLATE/config.yml`. The issue
  templates themselves use `.yaml`.

## Documentation

### README

Every project has a `README.md` that gives an overview of the project. A
project with versioned releases also says what its version numbers cover, such
as a command line, a library's API or a file format, so that readers know what
a breaking change breaks.

### Security Policy

Every project has a `SECURITY.md` at the repository root (not only in `.github/`
or `docs/`), which says:

- Vulnerabilities are reported through the repository's [private vulnerability
  reporting](#repository-settings), linked to its form, not in a public issue.
- Only the latest release receives security fixes, or, in a project without
  versioned releases, the latest commit on `main`.
- Fixes are made on a best-effort basis, with no promised response time.

A private repository keeps the file too, ready for when it becomes public.

### Changelog

A project that publishes versioned releases keeps a hand-written `CHANGELOG.md`
in the [Keep a Changelog](https://keepachangelog.com/en/2.0.0/) format and
follows [Semantic Versioning](https://semver.org/). Releases are tagged
`vX.Y.Z`. Libraries count as published unless they are marked never to be (in
Rust, every package sets `publish = false`, directly or through
`[workspace.package]`; in Python, the `Private :: Do Not Upload` classifier). A
project deployed straight from `main` needs no changelog.

- An `[Unreleased]` section at the top collects notable changes as they land,
  so that anyone who comes across the repository long after a release can see
  what has changed since.
- The changelog is written for its readers, not as a list of commits: a change
  reverted before the release isn't mentioned, and several changes that together
  make one difference are one entry.
- An entry for a breaking change starts with `**Breaking:**`, under the type of
  change it is, and says briefly how to upgrade.

To release:

1. Check the commits since the previous release for notable changes the
   `[Unreleased]` section missed.
2. Rename the section to the new version and date, and add a new, empty
   `[Unreleased]` above it.
3. Update the link definitions at the bottom: each version compares the previous
   tag with its own (the first links to its tag), and `[Unreleased]` compares
   the latest tag with `HEAD`. Before the first release, `[Unreleased]` links to
   the commits on `main`.
4. Set the manifests' versions to the new version (see
   [Versioning](#versioning)).

### Versioning

A repository has one version, that of its latest release, shared by all its
components:

- Rust's `version` in each `Cargo.toml`, and TypeScript's `version` in each
  `package.json`, name the latest release in `CHANGELOG.md` or, without one, the
  latest release tag. The release commit updates them. Before the first release
  they are `0.0.0`, written out explicitly.
- Python's version comes from the release tags (see [Python Package
  Version](#package-version)).

A program reports the version of the build it is, so that a build between
releases names its commit:

| Build                     | Reported version                       |
|---------------------------|----------------------------------------|
| On a release tag          | `0.2.0`                                |
| After a release tag       | `0.2.0-5-gf8a4d2b`                     |
| In a clone without tags   | `0.2.0+gf8a4d2b`                       |
| Without git               | `0.2.0`, the manifest's version        |

After a tag, the version is `git describe`'s without the leading `v`, ending in
`-dirty` if the working tree has changes. Python's hatch-vcs writes its own form
(`0.2.1.dev5+gf8a4d2b`). The template provides this for Rust applications
through `build.rs`, which sets `BUILD_VERSION` for `env!("BUILD_VERSION")`, and
for TypeScript through `vite.config.ts`, which sets `__APP_VERSION__` through
`define`, declared in `src/vite-env.d.ts`.

### Agent Guidance

A project may include an `AGENTS.md` describing its commands and conventions for
AI coding agents. `CLAUDE.md` holds only instructions specific to Claude, and
imports `AGENTS.md` with `@AGENTS.md` on a line of its own rather than repeating
it. The two are separate files, not a symlink, so that `CLAUDE.md` has room for
its own instructions.

### Planning

Open work may be kept in the issue tracker, or in two optional files at the
root:

- `TODO.md`, a list of things to work on, in no particular order.
- `ROADMAP.md`, the milestones ahead, in the order they are expected.

### Markdown

Markdown files are checked by
[markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2), in both
the Git hooks and CI. The standard `.markdownlint-cli2.jsonc` enables every
default rule, each as an error rather than a warning, requires dashes for
unordered lists, wraps prose and headings at 80 columns (tables and code blocks
are exempt, since wrapping would break them) and allows repeated headings under
different parents:

```jsonc
{
  "config": {
    "default": true,

    // MD004 - Unordered list style
    "MD004": {
      "style": "dash",
    },

    // MD013 - Line length
    "MD013": {
      "line_length": 80,
      "tables": false,
      "code_blocks": false,
    },

    // MD024 - No duplicate headings
    "MD024": {
      "siblings_only": true,
    },
  },
}
```

The configuration doesn't use `extends` or `overrides`, and there is no other
markdownlint configuration file, such as a `.markdownlint.*`, outside the
directories its `ignores` matches, since one would replace or override it. CI
runs `DavidAnson/markdownlint-cli2-action` with `globs: '**/*.md'`, since the
action otherwise checks only the files at the root.

## Licensing

Projects use an MIT-style license unless the code's heritage requires another.
Rust projects dual-license, where possible, as `MIT OR Apache-2.0`.

### License Files

Projects follow the [REUSE](https://reuse.software/) specification:

- License texts live in `LICENSES/`, named by SPDX identifier (for example,
  `LICENSES/MIT.txt`).
- The root holds an identical copy of each, where GitHub and readers look for
  it: `LICENSE` for a single license, or `LICENSE-<description>` for several
  (for example, `LICENSE-MIT` and `LICENSE-APACHE-2.0`). Every root copy is one
  of the texts in `LICENSES/`.
- Each manifest's `license` (in `Cargo.toml`, `pyproject.toml` and
  `package.json`) is an SPDX expression, and `LICENSES/` holds a text for every
  license and exception it names. In `pyproject.toml` it is a string, as PEP 639
  requires, not a table.

More complicated licensing is explained in the README.

### File Headers

Every source file starts with SPDX headers, in its comment syntax, giving the
license and the copyright holder with the year the file was created:

<!-- REUSE-IgnoreStart -->
```rust
// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>
```
<!-- REUSE-IgnoreEnd -->

Files that can't carry a comment, such as JSON files, lock files and images, are
annotated in a `REUSE.toml` at the root instead. `reuse lint` checks all of
this, in the Git hooks and in CI (through `fsfe/reuse-action`).

## Version Control

Projects use Git, with a `.gitignore` and a `.gitattributes`.

`.gitignore` lists what the project generates, such as build output, caches,
coverage reports, virtual environments and installed dependencies, in a section
per language, and local files its tools read but that are never committed, such
as Vite's `.env.local`. Files that editors and operating systems leave behind,
such as `.idea/` or `.DS_Store`, belong in each developer's global ignore file
(`core.excludesFile`, by default `~/.config/git/ignore`), for which the
`Global` templates of
[github/gitignore](https://github.com/github/gitignore/tree/main/Global) are a
good start.

`.gitattributes` normalizes line endings, marks binary formats, and enables
Git's built-in diff drivers for the project's languages:

```txt
# Auto-detect text files and normalize line endings to LF
*       text=auto eol=lf

# Binary files
*.gif   binary
*.ico   binary
*.jpg   binary
*.jpeg  binary
*.png   binary
*.webp  binary
*.woff  binary
*.woff2 binary

# Archives
*.bz2   binary
*.gz    binary
*.tar   binary
*.zip   binary

# Python
*.py    text diff=python
*.pyi   text diff=python

# Rust
*.rs    text diff=rust
```

Include only the language blocks that apply. Python projects also mark
`.git_archival.txt` as `export-subst` (see [Python Package
Version](#package-version)).

### Commit Messages

Commit messages follow [Conventional
Commits](https://www.conventionalcommits.org/):

```txt
<type>(<optional scope>)<optional !>: <subject>

[optional body]

[optional footers]
```

The types, always lowercase, are:

- `feat` - adds, changes or removes a feature
- `fix` - fixes one or more bugs
- `perf` - improves performance
- `refactor` - rewrites or restructures code without altering behavior
- `style` - addresses code style without altering behavior
- `test` - adds missing tests or corrects existing ones
- `docs` - adds or updates documentation only
- `build` - affects build-related components
- `ci` - affects CI workflows or other automation configuration
- `revert` - reverts an earlier commit
- `chore` - anything not otherwise mentioned

The [Conventional Commit Types
Cheatsheet](https://www.bavaga.com/blog/2025/01/27/my-ultimate-conventional-commit-types-cheatsheet/)
helps in choosing one. Dependency updates take Renovate's types, whether
Renovate or a person makes them: `fix(deps)` for a package's runtime
dependencies, and `chore(deps)` for everything else, such as development
dependencies, tools, actions, container images and lock file maintenance.
Changelogs are written by hand, so the type doesn't decide whether an update
gets an entry. Scopes are project-specific; a mature project lists its scopes in
its developer documentation.

- **Subject**: imperative mood, lowercase (apart from proper nouns, acronyms
  and the like), no closing punctuation. Aim to keep the first line within 50
  characters, looking for a shorter wording when it runs past that, and never
  exceed 72.
- **Breaking changes** add `!` before the colon, as in `feat!: drop the v1 API`.
  A change is breaking when it breaks what the project's version numbers cover
  (see [README](#readme)) since the last release, so the commit that adds a
  `**Breaking:**` changelog entry, or makes an entry breaking, carries the `!`.
  A `BREAKING-CHANGE:` footer may explain what breaks.
- **Reverts** use the `revert` type with the reverted commit's subject, as in
  `revert: add the history panel`, and the body names the reverted commit and
  says why.
- **Body**: optional. It explains the motivation for the change and how it
  differs from existing behavior, not what the code already makes clear. Wrap
  it at 72 characters, except a line holding a URL.
- **Footers**: Git trailers (`Token: value`), with hyphens for spaces in the
  token, such as `Co-Authored-By` and `Refs`. The breaking change footer is
  always `BREAKING-CHANGE`: Git doesn't read `BREAKING CHANGE`, with a space, as
  a trailer, and then reads none of the footers. Commits written with an AI
  agent carry a `Co-Authored-By` trailer naming it. `Signed-off-by` isn't
  required.

### Branches

`main` is the default branch.

A solo project may also keep a long-lived `dev` branch, to test commits in CI
without the overhead of a pull request. Commits are pushed to `dev`, and once CI
passes, `main` is fast-forwarded to it (`git push origin dev:main`). Work may
still go through a pull request when that is useful. A project with several
contributors documents its own use of `dev`, if any.

Other branches are named `<type>/<short-description>`, with the commit types
above. Branches created by AI agents or bots may use whatever names those tools
choose.

Pull requests are squash-merged, with GitHub's **Default message**: a single
commit's message, or the pull request's title for several. Every pull request
title follows the subject rules above, including the `!` for a breaking change.
Nothing checks the title, so whoever merges does.

## GitHub Repository

These settings live in GitHub rather than in the repository, so the template
can't set them. `norms github` checks them, except the Renovate app, which
GitHub reports only to apps (see the [README](README.md#commands)).

### Branch Protection

A repository ruleset named `Default` protects the default branch. It targets the
default branch, is active, has no bypass actors, and enables:

| Rule                          | Settings                                              |
|-------------------------------|-------------------------------------------------------|
| Restrict deletions            |                                                       |
| Block force pushes            |                                                       |
| Require linear history        |                                                       |
| Require signed commits        |                                                       |
| Require status checks to pass | `CI success` from GitHub Actions; branches up to date |

- The one required check is the CI [gate job](#gate-job). Its source is GitHub
  Actions, so that a commit status of the same name from elsewhere can't
  satisfy it, and requiring branches to be up to date means the tree CI tested
  is the tree that is merged. Never require a check that doesn't report on
  every commit, such as the scheduled audit or Renovate's `renovate/artifacts`:
  one that never reports blocks the merge.
- No pull request is required, so that `main` can be fast-forwarded to `dev`.
  GitHub still accepts a direct push only if the commit has passed CI.
- Commits pushed directly are signed locally. GitHub signs those it creates,
  such as squash merges and Renovate's commits.

A project with a `dev` branch protects it with a second ruleset, `Development`.
It targets `dev`, is active, has no bypass actors, and enables:

| Rule                   | Settings |
|------------------------|----------|
| Restrict deletions     |          |
| Require linear history |          |
| Require signed commits |          |

This refuses at once a push that `main` would later refuse. Force pushes stay
allowed, since `dev` is a working branch, and no checks are required, since
commits are pushed to `dev` to be tested.

Rulesets on private repositories require GitHub Pro. Without it, a private
repository follows the same workflow by convention.

### Repository Settings

Each repository has the [Renovate GitHub App](https://github.com/apps/renovate)
installed, and sets the following, named as in the repository's **Settings**.
Anything not listed keeps GitHub's default.

**General**:

- Issues are on. Wikis and Projects are off unless the project uses them, so
  they don't invite content nobody maintains.
- Only squash merging is allowed, with the **Default message**.
- **Always suggest updating pull request branches** is on, so that a pull
  request into a branch other than `main`, such as one stacked on a feature
  branch, is offered the update too.
- **Allow auto-merge** is off. Renovate merges its own pull requests, since
  platform automerge waits only for required checks, not for statuses such as
  `renovate/artifacts` that Renovate waits for itself.
- **Automatically delete head branches** is on, since GitHub never sees a
  squash-merged branch as merged.

**Actions**, under **General**:

- **Workflow permissions** is **Read repository contents and packages
  permissions**.
- **Allow GitHub Actions to create and approve pull requests** is off.
- **Require actions to be pinned to a full-length commit SHA** is on.

**Advanced Security**:

- **Private vulnerability reporting** is on (public repositories only).
- **Dependabot alerts** are on, since Renovate reads them for its vulnerability
  pull requests.
- **Dependabot security updates** are off, and Dependabot version updates aren't
  configured, since Renovate does both.
- **Secret Protection** and its **Push protection** are on. They are free on
  public repositories; private ones have them only in organizations that buy
  GitHub Secret Protection.

## Git Hooks

Git hooks are run by [prek](https://prek.j178.dev/), configured in `prek.toml`
at the repository root.

### prek Configuration

```toml
minimum_prek_version = "0.4.11"
default_install_hook_types = ["pre-commit", "commit-msg"]
default_stages = ["pre-commit"]

[priorities]
lock = 100
check = 200
```

- `default_install_hook_types` makes a plain `prek install` install the commit
  message hook too. A project with hooks of another type, such as `pre-push`,
  adds it here, and those hooks list their own stages.
- `default_stages` keeps the other hooks from running again on the commit
  message.
- `minimum_prek_version` is the oldest prek that runs the configuration. It
  rises only when the configuration needs a newer prek, not with each prek
  release.
- **Priorities** let hooks run in parallel. Each lock file hook (`cargo-lock`,
  `uv-lock`, `pnpm-lock`) has `priority = "lock"`, so that it finishes before
  the others read the dependencies it installs. Every other `pre-commit` hook
  outside the builtin repository has `priority = "check"`, except `reuse`, which
  must not run while other hooks create caches. The builtin hooks and `reuse`
  have no priority, so they run first. A hook that modifies tracked files never
  shares a priority.
- Each repository is a `[[repos]]` table, and each hook a `[[repos.hooks]]`
  table, not an inline table. A remote repository's `rev` is a release tag, on
  the line after its `repo`, where Renovate's shared preset looks for it:

  ```toml
  [[repos]]
  repo = "https://github.com/fsfe/reuse-tool"
  rev = "v6.3.0"

  [[repos.hooks]]
  id = "reuse"
  ```

- A directory holding configurations that aren't the repository's own, such as
  test fixtures, is listed in `.prekignore`.

### Common Hooks

Every project runs these hooks:

- [prek's builtin hooks](https://prek.j178.dev/built-in-hooks/), from
  `repo = "builtin"`: `check-added-large-files`, `check-case-conflict`,
  `check-illegal-windows-names`, `check-merge-conflict`, `check-symlinks`,
  `check-toml`, `check-vcs-permalinks`, `check-yaml`, `detect-private-key`,
  `end-of-file-fixer` and `trailing-whitespace`. A file with several YAML
  documents, such as `pnpm-lock.yaml`, needs `check-yaml`'s
  `--allow-multiple-documents`.
- [reuse](https://github.com/fsfe/reuse-tool): `reuse` (see
  [Licensing](#licensing)).
- [markdownlint-cli2](https://github.com/DavidAnson/markdownlint-cli2):
  `markdownlint-cli2` (see [Markdown](#markdown)).
- [Tombi](https://github.com/tombi-toml/tombi-pre-commit): `tombi-format`, which
  checks the formatting of every TOML file but the lock files, including the
  order of keys and dependencies in `Cargo.toml` and `pyproject.toml`. Further
  exclusions go in `tombi.toml`'s `[files] exclude`. CI's `tombi` job installs
  it with `tombi-toml/setup-tombi` and runs
  `tombi format --check --diagnostics-format github`, which annotates each
  unformatted file.

  ```toml
  [[repos.hooks]]
  id = "tombi-format"
  args = ["--check"]
  exclude = '(^|/)(Cargo|uv)\.lock$'
  ```

- [zizmor](https://github.com/zizmorcore/zizmor-pre-commit): `zizmor`, with
  `--pedantic` and `--strict-collection`, which fails on a workflow or action
  zizmor can't parse rather than skipping it with a warning.
- [conventional-pre-commit](https://github.com/compilerla/conventional-pre-commit):
  `conventional-pre-commit`, in the `commit-msg` stage, listing the commit types
  itself rather than relying on the hook's defaults:

  ```toml
  [[repos.hooks]]
  id = "conventional-pre-commit"
  stages = ["commit-msg"]
  args = ["feat", "fix", "perf", "refactor", "style", "test", "docs", "build", "ci", "revert", "chore"]
  ```

- [norms](https://github.com/aexoden/norms): `norms-commit-msg`, which checks
  what `conventional-pre-commit` doesn't (a lowercase type, lines within 72
  characters, and a `BREAKING-CHANGE` footer with a hyphen), and `norms-verify`
  with `--deny-warnings`:

  ```toml
  [[repos]]
  repo = "https://github.com/aexoden/norms"
  rev = "vX.Y.Z"

  [[repos.hooks]]
  id = "norms-commit-msg"

  [[repos.hooks]]
  id = "norms-verify"
  args = ["--deny-warnings"]
  ```

### Language Hooks

Each language section lists its required hooks. They are `local` hooks with
`language = "system"` and `pass_filenames = false`, since a remote hook is
pinned by its own `rev`, and one in a language prek manages installs its own
copy of the tool, without the project's dependencies or plugins. They:

- Run the project's own tools directly, at the versions its lock files and
  toolchain pins select, never through Devbox or a shell. A component in a
  subdirectory is reached with the tool's own option (`--manifest-path`,
  `uv --directory`, `pnpm --dir`), not `bash -c "cd <path> && ..."`.
- Check all of the component's files, not only the staged ones, since a change
  to one file can break another.
- Check without modifying files, so the hooks and CI give the same results.
- Have IDs unique across languages (`pytest`, `cargo-test`, `vitest`), so that
  `PREK_SKIP` can target one. A hook run once per component gives each copy an
  `alias` of its own (`eslint-admin`, `eslint-web`).

## Continuous Integration

CI runs on GitHub Actions, in two workflows:

| Workflow                       | Triggers                                        | Purpose                                 |
|--------------------------------|-------------------------------------------------|-----------------------------------------|
| `.github/workflows/ci.yaml`    | Pushes to `main` and `dev`, all pull requests   | Every check the Git hooks run           |
| `.github/workflows/audit.yaml` | Daily schedule, manual dispatch                 | Security advisory audit of dependencies |

### Checks

`ci.yaml` fails in every case where the Git hooks would.

- **Triggers**: `push` names both branches, `branches: [dev, main]`, rather
  than matching them with patterns, whether or not the project keeps `dev`.
  `pull_request` has no `branches` or `paths` filters, nor their `-ignore`
  forms, and any `types` it lists include the default `opened`, `synchronize`
  and `reopened`, since the ruleset waits for the gate job, and a workflow that
  doesn't run blocks the pull request.
- **Jobs**: most hooks have a job of their own, named after the hook ID
  (`markdownlint` and `tombi` for `markdownlint-cli2` and `tombi-format`). A
  `prek` job runs the rest, such as the builtin hooks, with
  `prek run --all-files --show-diff-on-failure`, and lists the hooks that have
  their own jobs in `PREK_SKIP`. It installs prek with `taiki-e/install-action`
  at a pinned version.
- **Actions**: where a section names an action for a check, such as
  `zizmorcore/zizmor-action` or `EmbarkStudios/cargo-deny-action`, CI uses it
  rather than running the tool itself. Toolchains are installed with each
  language's standard setup action, at the versions the repository pins.
- **Libraries** also test the oldest versions they support, which the hooks
  can't: an `msrv` job for Rust and a `pytest-min-python` job for Python (see
  the language sections).
- **zizmor** runs through `zizmorcore/zizmor-action` with
  `persona: pedantic`, `advanced-security: false` and `annotations: true`, so
  that a finding fails the job like any other check, rather than being uploaded
  to code scanning, and with `strict-collection: true`, as the hook passes
  `--strict-collection`.

### norms Job

The `norms-verify` job runs the norms that the hook's `rev` pins:

- It checks out the whole history (`fetch-depth: 0`), since `norms verify` reads
  the release tags.
- It caches prek's hook environments, `~/.cache/prek`, with `actions/cache`,
  keyed on the runner's OS, the prek version and the hash of `prek.toml`, with
  no `restore-keys`.
- It runs `prek run norms-verify --all-files`.
- It then runs the `commit-msg` hooks on the message of the commit being tested
  (the pushed commit, or the pull request's head commit), with
  `prek run --stage commit-msg --commit-msg-filename <file>`, as the template's
  `ci.yaml` does. A merge commit, such as GitHub's **Update branch** makes,
  isn't checked, since linear history keeps it off `main`. Otherwise the step
  first rejects a subject starting with `fixup!`, `squash!`, `amend!` or
  `Merge`, which the hooks let through in local commits. Long lines only warn
  there, since the commit has been made.

### Job Settings

- Split checks into parallel jobs where feasible. Use an operating system matrix
  where, and only where, differences between the platforms the project
  supports could plausibly cause problems.
- Jobs that install nothing and run a single action or shell step besides the
  checkout (the gate job, markdownlint and zizmor) run on `ubuntu-slim`, as
  does the Tombi job, whose setup only downloads a prebuilt binary. Jobs that
  install a toolchain or other tools run on `ubuntu-latest`, as do those using a
  Docker-based action, which `ubuntu-slim` can't run.
- Every job sets `timeout-minutes`: 30 for jobs that compile, type check or run
  tests, and 10 for the rest, raised only for jobs that legitimately need more.
  Jobs on `ubuntu-slim` can't exceed its own 15-minute limit.
- Each workflow sets a `concurrency` group per ref, with
  `cancel-in-progress: true`.
- An `astral-sh/setup-uv` step in a job that runs `uv` itself, such as
  `uv sync`, sets `version` to an exact uv version, since the action otherwise
  installs the newest uv that `required-version` allows. A job that only runs
  tools through `uvx` doesn't install the project, so its uv needn't be pinned.
  A step that pins uv also sets `require-checksums: true`, so that setup-uv
  fails rather than install a uv whose checksum it doesn't know. setup-uv knows
  the checksums of the uv releases before its own, so a Renovate update of uv
  that fails this way waits for the setup-uv release that knows it.
- Tools that no Renovate manager tracks, such as those installed with
  `taiki-e/install-action`, are pinned as described in [Automated
  Updates](#automated-updates).
- Use Devbox only in jobs that need a local service.

### Gate Job

`ci.yaml` ends with a gate job, `ci-success`, which succeeds only if every other
job succeeded. Branch protection requires this single check, so jobs can be
added, renamed or split without changing the repository's settings:

```yaml
ci-success:
  name: CI success
  if: always()
  needs: [lint, test]
  runs-on: ubuntu-slim
  timeout-minutes: 10
  permissions: {}
  steps:
    - name: Fail unless every job succeeded
      if: contains(needs.*.result, 'failure') || contains(needs.*.result, 'cancelled') || contains(needs.*.result, 'skipped')
      run: exit 1
```

- `needs` lists every other job, since `needs.*.result` covers only direct
  dependencies.
- `if: always()` is essential: without it, the gate is skipped when a job
  fails, and a skipped job satisfies a required check.
- Any result other than success fails the gate, including a skipped job.
- No job sets `continue-on-error`, which would let a failed job pass the
  workflow.
- GitHub shows the check by its `name`, not the job ID.

### Hardening

- Pin every action to a full commit SHA, with the version in a trailing comment
  (`uses: actions/checkout@<sha> # v7.0.1`). Renovate keeps these current.
- Set `permissions: {}` at the top level of every workflow, and grant each job
  only what it needs: `contents: read` for a job that checks out the
  repository, and nothing for one that doesn't. Only private repositories need
  it, but granting it everywhere keeps the workflows the same whatever the
  repository's visibility. No job needs more.
- Set `persist-credentials: false` on every `actions/checkout` step.
- Run [zizmor](https://docs.zizmor.sh/) with its pedantic persona, in both the
  Git hooks and CI, which checks these rules.

### Security Audit

`audit.yaml` checks dependencies for security advisories every day, and on
`workflow_dispatch`, so that it can be rerun once a fix lands. It runs apart
from `ci.yaml` because new advisories appear without code changes, and
shouldn't block unrelated pull requests:

- Rust: `cargo deny --workspace --all-features check -D warnings advisories`.
  Since cargo-deny 0.22, its `[advisories] unmaintained` defaults to
  `workspace`, so it reports an unmaintained crate only when a workspace
  member depends on it directly, which the project can replace, and not one
  further down the tree. The template's `deny.toml` leaves it at that default.
- Python: `uv audit --locked`.
- TypeScript: `pnpm audit`.

## Dependency Management

### Automated Updates

[Renovate](https://docs.renovatebot.com/) updates dependencies. The shared
[renovate-config](https://github.com/aexoden/renovate-config) preset sets the
automerge, grouping, pinning and release age policies, so `renovate.json`
normally contains only:

```json
{
  "$schema": "https://docs.renovatebot.com/renovate-schema.json",
  "extends": ["github>aexoden/renovate-config"]
}
```

A project may add rules of its own.

A workflow that installs or runs a tool at a version no Renovate manager reads,
such as through `taiki-e/install-action`'s `tool` input, `uvx`, `cargo install`
or `pnpm dlx`, pins it in an environment variable ending in `_VERSION`, in the
workflow's `env` (or a job's, if only that job uses it), under a `# renovate:`
comment naming its datasource and package:

```yaml
env:
  # renovate: datasource=crate depName=cargo-llvm-cov
  CARGO_LLVM_COV_VERSION: "0.9.1"
  # renovate: datasource=pypi depName=prek
  PREK_VERSION: "0.5.10"
```

Steps use it as `${{ env.PREK_VERSION }}` in an input, or `"$PREK_VERSION"` in a
script, never with the version written inline.

### Version Pinning

Every project commits its lock files. Applications also pin every direct
dependency from a registry, development dependencies included, to an exact
version:

| Ecosystem | Manifest syntax                     | Lock file        |
|-----------|-------------------------------------|------------------|
| Cargo     | `serde = "=1.0.228"`                | `Cargo.lock`     |
| Python    | `"pydantic==2.12.0"`                | `uv.lock`        |
| pnpm      | `"react": "19.2.0"` (no `^` or `~`) | `pnpm-lock.yaml` |

Libraries instead declare compatible version ranges, so that downstream projects
can resolve shared dependencies to a single version.

### Dependency Order

Dependencies are listed in alphabetical order in each table, list or object.
Formatters keep the order: [Tombi](https://tombi-toml.github.io/tombi/) in
`Cargo.toml` and `pyproject.toml`, and Prettier, through
prettier-plugin-packagejson, in `package.json`.

### Release Age

A release must be at least three days old before a project adopts it, giving the
community time to catch compromised or broken releases. It is enforced twice:

1. By Renovate, through `minimumReleaseAge` in the shared preset.
2. By the package manager, which also covers lock file maintenance and manual
   updates:
   - Cargo: `global-min-publish-age = "3 days"` under `[registry]` in
     `.cargo/config.toml` (enforced from Rust 1.100).
   - uv: `exclude-newer = "3 days"` under `[tool.uv]` in `pyproject.toml`.
   - pnpm: `minimumReleaseAge: 4320` (minutes) in `pnpm-workspace.yaml`.

### Urgent Updates

Security fixes may skip the release age. Renovate raises its vulnerability alert
pull requests at once, and for pnpm, since `minimumReleaseAge` is set, exempts
the new version itself, adding `package-name@version` to
`minimumReleaseAgeExclude` in `pnpm-workspace.yaml` with a comment naming the
update. A new dependency of the fix still needs adding there by hand. With
`minimumReleaseAgeExcludePrune: true`, pnpm drops an entry at its next install
or update once the lock file no longer uses that version. Until then, the entry
exempts only a version that soon passes the release age anyway.

uv and Cargo still refuse the new version, so the pull request shows an
artifact error until the package is exempted. Exempt it temporarily, with a
comment naming the advisory, and remove the exemption once the release is old
enough:

- uv: `exclude-newer-package = { package-name = false }` under `[tool.uv]`.
- Cargo: there is no per-package setting. Raise the package's requirement in
  `Cargo.toml` if it pins one, then run
  `CARGO_RESOLVER_INCOMPATIBLE_PUBLISH_AGE=allow cargo update -p package-name`.
  Review the lock file, since any new dependency also skips the release age.
  Until the release is old enough, a plain `cargo update` fails on a raised
  requirement, or otherwise downgrades the package again, so hold Renovate's
  lock file maintenance pull request until then.

## Rust

### Toolchain

`rust-toolchain.toml` pins an exact channel and the components the hooks need:

```toml
[toolchain]
channel = "1.100.0"
components = ["clippy", "llvm-tools", "rustfmt"]
```

A component in a subdirectory uses the root's file unless it has its own.

`.cargo/config.toml` sets the [release age](#release-age):

```toml
[registry]
global-min-publish-age = "3 days"
```

A library sets `rust-version` (its MSRV) in `Cargo.toml`, in each workspace
member or inherited with `rust-version.workspace = true`. Its CI has an `msrv`
job that installs that toolchain with
`actions-rust-lang/setup-rust-toolchain`'s `toolchain` input and runs
`cargo check --workspace --all-features --locked`. It doesn't check the tests,
since development dependencies may need a newer Rust.

### Manifest

`Cargo.toml` sets `version` (see [Versioning](#versioning)), `description`,
`repository`, `license` and `edition` (the latest, currently 2024). A package
that may be published also sets `keywords` and `categories`. Workspace members
may inherit these from `[workspace.package]`.

### Lints

`Cargo.toml` enables Cargo's `pedantic` group, Clippy's `pedantic`, `nursery`
and `cargo` groups, and a selection of Clippy restriction lints:

```toml
[lints.cargo]
pedantic = { level = "warn", priority = -1 }

[lints.clippy]
absolute_paths = "warn"
allow_attributes = "warn"
allow_attributes_without_reason = "warn"
as_pointer_underscore = "warn"
cargo = { level = "warn", priority = -1 }
cfg_not_test = "warn"
clone_on_ref_ptr = "warn"
create_dir = "warn"
dbg_macro = "warn"
decimal_literal_representation = "warn"
default_union_representation = "warn"
definition_in_module_root = "warn"
doc_paragraphs_missing_punctuation = "warn"
empty_enum_variants_with_brackets = "warn"
empty_structs_with_brackets = "warn"
exit = "warn"
expect_used = "warn"
field_scoped_visibility_modifiers = "warn"
filetype_is_file = "warn"
float_cmp_const = "warn"
fn_to_numeric_cast_any = "warn"
if_then_some_else_none = "warn"
indexing_slicing = "warn"
infinite_loop = "warn"
inline_modules = "warn"
lossy_float_literal = "warn"
map_err_ignore = "warn"
map_with_unused_argument_over_ranges = "warn"
mem_forget = "warn"
mixed_read_write_in_expression = "warn"
multiple_crate_versions = "allow"
multiple_inherent_impl = "warn"
multiple_unsafe_ops_per_block = "warn"
needless_raw_strings = "warn"
non_zero_suggestions = "warn"
nursery = { level = "warn", priority = -1 }
panic = "warn"
panic_in_result_fn = "warn"
partial_pub_fields = "warn"
pathbuf_init_then_push = "warn"
pedantic = { level = "warn", priority = -1 }
pointer_format = "warn"
precedence_bits = "warn"
rc_buffer = "warn"
rc_mutex = "warn"
redundant_pub_crate = "allow"
redundant_test_prefix = "warn"
redundant_type_annotations = "warn"
renamed_function_params = "warn"
return_and_then = "warn"
same_name_method = "warn"
self_named_module_files = "warn"
semicolon_inside_block = "warn"
shadow_unrelated = "warn"
str_to_string = "warn"
string_add = "warn"
string_slice = "warn"
suspicious_xor_used_as_pow = "warn"
tests_outside_test_module = "warn"
todo = "warn"
try_err = "warn"
unimplemented = "warn"
unnecessary_rest_pattern = "warn"
unneeded_field_pattern = "warn"
unseparated_literal_suffix = "warn"
unused_result_ok = "warn"
unused_trait_names = "warn"
unwrap_used = "warn"
use_debug = "warn"
verbose_file_reads = "warn"
wildcard_enum_match_arm = "warn"

[lints.rust]
future_incompatible = { level = "warn", priority = -1 }
let_underscore = { level = "warn", priority = -1 }
unreachable_pub = "warn"
unsafe_code = "forbid"
unused_qualifications = "warn"
```

- Groups have priority -1, so that individual lints can override them.
- A workspace may set the lints once in `[workspace.lints.*]`, which each
  package inherits with `[lints] workspace = true`, the package in the root
  manifest included, if there is one.
- The groups and the `[lints.rust]` entries stay at their level or stricter. A
  package that needs to may set one of the restriction lints to `allow`, with a
  comment saying why, such as an application allowing `exit`.
- `multiple_crate_versions` is allowed, since a project can't fix duplicate
  transitive dependencies.
- `redundant_pub_crate` is allowed, since it contradicts `unreachable_pub`.
  Items not reachable from outside the crate are `pub(crate)`, but fields of
  `pub(crate)` types stay `pub`, since `field_scoped_visibility_modifiers`
  forbids scoped visibility on fields.
- Modules use `foo/mod.rs`, which holds only submodule declarations and
  re-exports, and every module but tests has a file of its own.

`clippy.toml` (not `.clippy.toml`) allows `unwrap`, `expect` and `panic` in
tests, and limits absolute paths to three segments:

```toml
absolute-paths-max-segments = 3
allow-expect-in-tests = true
allow-panic-in-tests = true
allow-unwrap-in-tests = true
```

A Rust component in a subdirectory, and a workspace member, shares the root's
`clippy.toml` unless it has its own, and whichever file a package uses meets
these rules.

The `clippy` hook and CI job deny every warning, rustc's, Clippy's and
Cargo's, with `--config 'build.warnings="deny"'`. Clippy's `-- -D warnings`
would leave Cargo's own lints as warnings. A library's `msrv` job doesn't deny
warnings, since Cargo before Rust 1.100 warns about `[lints.cargo]`, which it
doesn't know.

### Dependency Policy

`deny.toml` (not `.deny.toml`, `.cargo/deny.toml` or `.config/deny.toml`)
configures [cargo-deny](https://embarkstudios.github.io/cargo-deny/). A Rust
component in a subdirectory shares the root's file unless it has its own, and
whichever file a component uses meets these rules:

- `[licenses] allow` lists only the licenses cargo-deny encounters, which by
  default excludes development-only dependencies.
- `[bans] multiple-versions = "allow"`, as for the Clippy lint.
- `[bans] std-replacements = {}`, which rejects a direct dependency that `std`
  now replaces, such as `once_cell`. Crates kept anyway go in its `ignore`.
- Every entry in `[advisories] ignore` has a `reason`, and every ignored
  advisory expires, through its `expiry` or `[advisories] ignore-expiry`, so
  that ignores are reviewed. A duration such as `P90D` counts from the
  advisory's publication, so an advisory already older than that needs a date.
  Crates ignored for being yanked have no expiry.

Each developer installs cargo-deny themselves (see its [installation
documentation](https://embarkstudios.github.io/cargo-deny/#install-cargo-deny)).

### Testing

Use Cargo's test harness with
[cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for coverage,
installed like cargo-deny, and in CI with `taiki-e/install-action`. A project
with a library target also runs its doctests separately, since cargo-llvm-cov
runs them only on nightly.

### Hooks and CI

The Git hooks and CI both run:

| Hook ID         | Command                                                                                  |
|-----------------|------------------------------------------------------------------------------------------|
| `cargo-lock`    | `cargo update --workspace --locked`                                                      |
| `rustfmt`       | `cargo fmt --all --check`                                                                |
| `clippy`        | `cargo clippy --workspace --all-targets --all-features --config 'build.warnings="deny"'` |
| `cargo-test`    | `cargo llvm-cov --workspace --all-features --show-missing-lines`                         |
| `cargo-doctest` | `cargo test --workspace --all-features --doc` (library targets only)                     |
| `cargo-deny`    | `cargo deny --workspace --all-features check -D warnings bans licenses sources`          |

`cargo-lock` limits the update to the workspace's own packages, which changes
nothing in a current lock file, and `--locked` makes it fail rather than write a
stale one.

In CI:

- `actions-rust-lang/setup-rust-toolchain` installs the toolchain and caches
  builds, so jobs add no cache step of their own. Jobs that build nothing set
  `cache: false`.
- `actions-rust-lang/rustfmt` checks formatting.
- `EmbarkStudios/cargo-deny-action` runs cargo-deny, with
  `arguments: --workspace --all-features` and
  `command: check -D warnings bans licenses sources` (`advisories` in
  `audit.yaml`).

## Python

### Toolchain

Use [uv](https://docs.astral.sh/uv/) for every Python project.

- `.python-version`, beside `pyproject.toml`, names the exact Python version
  (`x.y.z`) for development and CI.
- Applications set `requires-python` to that version (`==3.15.0`). Libraries set
  the range they support, which includes it.
- The `Programming Language :: Python :: 3.x` classifiers list each minor
  version `requires-python` allows, up to the one in `.python-version`.
- `[tool.uv] required-version` sets the minimum uv version as a single `>=`
  (`>=0.9.5`), so that newer releases of uv keep working, and CI pins an exact
  uv version within it.
- A library's CI also tests the oldest Python it supports, in a
  `pytest-min-python` job whose `astral-sh/setup-uv` step sets it as
  `python-version`.

### Project Layout

Projects use a `src/` layout:

```txt
project/
├── src/
│   └── package_name/
│       ├── __init__.py
│       ├── __about__.py
│       └── ...
├── tests/
│   ├── __init__.py
│   └── ...
├── .git_archival.txt
├── .python-version
├── pyproject.toml
├── uv.lock
└── README.md
```

A library also ships a `py.typed` marker in its package, and lists the
`Typing :: Typed` classifier, so that type checkers read its annotations.

Ruff, mypy, basedpyright, pytest and uv are configured in `pyproject.toml` alone:
there is no `ruff.toml`, `mypy.ini`, `pyrightconfig.json`, `pytest.ini`,
`uv.toml` or similar file, which would replace its settings (for `uv.toml`, the
tool settings such as `exclude-newer` and `required-version`).

Development tools go in the `dev` dependency group, not in optional
dependencies:

```toml
[dependency-groups]
dev = ["basedpyright==...", "mypy==...", "pytest==...", "pytest-cov==...", "ruff==..."]
```

### Package Version

The version comes from Git tags through
[hatch-vcs](https://github.com/ofek/hatch-vcs):

```toml
[project]
dynamic = ["version"]

[build-system]
requires = ["hatch-vcs", "hatchling"]
build-backend = "hatchling.build"

[tool.hatch.version]
source = "vcs"

[tool.uv]
# Reinstall the project when HEAD or tags move, so the reported version stays current.
cache-keys = [{ file = "pyproject.toml" }, { git = { commit = true, tags = true } }]
```

- A component in a subdirectory also sets
  `raw-options = { search_parent_directories = true }` in
  `[tool.hatch.version]`, so that hatch-vcs finds the repository above it.
- A project with `[tool.uv] package = false` isn't built, and needs none of
  this.
- Code reads the version with `importlib.metadata.version()`, in
  `__about__.py`.

`.git_archival.txt`, beside `pyproject.toml` and marked `export-subst` in
`.gitattributes`, lets source archives without Git history report their version:

```txt
node: $Format:%H$
node-date: $Format:%cI$
describe-name: $Format:%(describe:tags=true,match=*[0-9]*)$
```

### Formatting and Linting

Use [Ruff](https://docs.astral.sh/ruff/) for both, with every rule enabled,
preview rules included:

```toml
[tool.ruff]
line-length = 120

[tool.ruff.format]
line-ending = "lf"

[tool.ruff.lint]
preview = true
select = ["ALL"]
ignore = ["missing-trailing-comma"]

[tool.ruff.lint.flake8-copyright]
notice-rgx = "(?i)SPDX-FileCopyrightText: \\d{4}"

[tool.ruff.lint.isort]
lines-between-types = 1

[tool.ruff.lint.per-file-ignores]
"tests/*.py" = ["assert", "private-member-access"]

[tool.ruff.lint.pydocstyle]
convention = "google"
```

`line-length` is 120, as is Prettier's `printWidth`. `missing-trailing-comma`
conflicts with the Ruff formatter. Any further ignore, per-file ones included,
has a comment on its line saying why:

```toml
ignore = [
  "missing-trailing-comma",
  "line-contains-todo",  # Tracked in issues instead.
]
```

### Type Checking

Type check with both [mypy](https://mypy.readthedocs.io/en/stable/) and
[basedpyright](https://docs.basedpyright.com/), since each catches errors the
other misses. Avoid `Any` unless the code can't be written without it.

mypy runs in strict mode, with most of the checks strict mode leaves out:

```toml
[tool.mypy]
strict = true
strict_equality_for_none = true
warn_unreachable = true
enable_error_code = [
  "deprecated",
  "exhaustive-match",
  "explicit-override",
  "mutable-override",
  "possibly-undefined",
  "redundant-expr",
  "redundant-self",
  "truthy-bool",
  "truthy-iterable",
  "unimported-reveal",
  "unused-awaitable",
]
```

`explicit-override` requires `@override` on every overriding method, imported
from `typing`, or from `typing_extensions` in a library that supports Python
before 3.12. The other optional checks are left off: Ruff's
`blanket-type-ignore` already reports what `ignore-without-code` does,
basedpyright's `reportExplicitAny` already rejects what `disallow_any_explicit`
does, and `disallow_any_unimported` matters only to a project that ignores
missing imports.

basedpyright runs in its `recommended` mode, which turns on nearly all of its
rules and fails on warnings:

```toml
[tool.basedpyright]
typeCheckingMode = "recommended"
```

A rule that proves a poor fit may be turned off in `[tool.basedpyright]`, with a
comment saying why.

### Testing

Use [pytest](https://docs.pytest.org/) with
[pytest-cov](https://pytest-cov.readthedocs.io/) for coverage. pytest runs in
strict mode, set in its native TOML table, which pytest reads since version 9:

```toml
[tool.pytest]
strict = true
```

### Hooks and CI

The Git hooks and CI both run:

| Hook ID        | Command                                                          |
|----------------|------------------------------------------------------------------|
| `uv-lock`      | `uv lock --check`                                                |
| `ruff-format`  | `uv run ruff format --check`                                     |
| `ruff-check`   | `uv run ruff check`                                              |
| `mypy`         | `uv run mypy .`                                                  |
| `basedpyright` | `uv run basedpyright`                                            |
| `pytest`       | `uv run pytest --cov=src/package_name --cov-report=term-missing` |

CI installs uv with `astral-sh/setup-uv` and runs `uv sync --locked` before the
checks. Jobs that install nothing, the `uv-lock` job and `audit.yaml`'s
`uv audit` job, set `enable-cache: false`: setup-uv's cache is shared among
jobs on the same Python version, and one saved by a job that installs nothing
would lack the packages the others need.

## TypeScript

A TypeScript project lives at the repository root when it is the primary
language, and otherwise in a named subdirectory, such as `frontend/`. Paths
below are relative to that directory.

### Toolchain

`package.json` pins Node.js and pnpm through `devEngines`, which pnpm downloads
automatically:

```json
"devEngines": {
  "runtime": { "name": "node", "version": "26.13.0", "onFail": "download" },
  "packageManager": { "name": "pnpm", "version": "12.18.0", "onFail": "download" }
}
```

It doesn't set Corepack's `packageManager` field, which would be a second pnpm
pin. pnpm is the only package manager, so there is no `package-lock.json` or
`yarn.lock`.

### Manifest

`package.json` sets `version` (see [Versioning](#versioning)),
`"type": "module"` and, in applications, `"private": true`. It defines these
scripts:

| Script      | Command                     |
|-------------|-----------------------------|
| `build`     | `tsc -b && vite build`      |
| `format`    | `prettier --check .`        |
| `lint`      | `eslint . --max-warnings 0` |
| `test`      | `vitest run`                |
| `typecheck` | `tsc -b --noEmit`           |

In a pnpm workspace, each package's `package.json` follows the same rules, and
sets no `packageManager` either, but the `devEngines` pins and the scripts
belong to the root's alone.

`pnpm-workspace.yaml` sets the release age, drops exemptions from it once they
are no longer used, refuses trust downgrades, and records in `permissions` what
each dependency may do, such as running build scripts:

```yaml
minimumReleaseAge: 4320
minimumReleaseAgeExcludePrune: true
permissions:
  esbuild:
    build: true
trustPolicy: no-downgrade
```

- A new project starts with `permissions: {}`, where `pnpm approve` and
  `pnpm approve-builds` record their decisions. It doesn't use `allowBuilds`,
  so that every decision is in one place, nor `dangerouslyAllowAllBuilds: true`,
  `strictDepBuilds: false`, or the settings pnpm 11 removed, such as
  `onlyBuiltDependencies`.
- `trustPolicy: no-downgrade` rejects a version published with weaker trust
  evidence than an earlier one, as a release published with stolen credentials
  often is. A package that legitimately lost that evidence is listed in
  `trustPolicyExclude`, with a comment saying why.

### Compiler Options

`tsconfig.json` is solution-style, referencing `tsconfig.app.json` for the
application and `tsconfig.node.json` for tooling files such as
`vite.config.ts`. Each referenced config enables at least:

```json
"strict": true,
"noUncheckedIndexedAccess": true,
"noUncheckedSideEffectImports": true,
"noFallthroughCasesInSwitch": true,
"noUnusedLocals": true,
"noUnusedParameters": true,
"verbatimModuleSyntax": true,
"erasableSyntaxOnly": true,
"moduleDetection": "force",
"noEmit": true
```

`noEmit` leaves the output to Vite, so that the `build` script's `tsc -b`
writes no JavaScript beside the sources.

`verbatimModuleSyntax` implies `isolatedModules`, which therefore need not be
set. `erasableSyntaxOnly` rejects TypeScript syntax with runtime effects, such
as enums and namespaces, so that all of the project's TypeScript could run with
Node's type stripping.

The module settings follow what loads each config's files:

- `tsconfig.app.json`, bundled by Vite: `"module": "esnext"` and
  `"moduleResolution": "bundler"`.
- `tsconfig.node.json`, loaded by Node: `"module": "nodenext"`, with
  `moduleResolution` unset to follow it, and
  `"allowImportingTsExtensions": true`, with relative imports naming the `.ts`
  file.

`target` and `lib` name the newest ECMAScript version the code's runtime fully
supports: for the application, what every browser of Vite's default build
target supports (ES2022 for Vite 8), and for the tooling, what the pinned
Node.js supports (ES2025 for Node.js 26).

### Formatting and Linting

Use [Prettier](https://prettier.io/) for formatting, configured in
`.prettierrc` alone, with no `prettier` key in any `package.json` and no
configuration in subdirectories:

```json
{
  "plugins": ["prettier-plugin-packagejson"],
  "printWidth": 120
}
```

[prettier-plugin-packagejson](https://github.com/matzkoh/prettier-plugin-packagejson)
sorts `package.json`'s fields, scripts and dependencies. Every component has a
`.prettierignore`, which lists `pnpm-lock.yaml`, since pnpm writes it. In a
subdirectory, where Prettier doesn't read the root `.gitignore`, it also lists
build output (`dist/`) and coverage reports (`coverage/`).

Use [ESLint](https://eslint.org/) for linting, configured in `eslint.config.ts`
alone, which ESLint loads through [jiti](https://github.com/unjs/jiti), a
development dependency, since it uses Node's own type stripping only behind an
experimental flag. The configuration includes typescript-eslint's
`strictTypeChecked` and `stylisticTypeChecked` configs, with `projectService`
enabled, and `eslint-config-prettier`. Don't use `eslint-plugin-prettier`, which
would run Prettier a second time.

### Testing

Use [Vitest](https://vitest.dev/) with
[@vitest/coverage-v8](https://vitest.dev/guide/coverage.html). The hooks and CI
report coverage, and the `test` script leaves it out for a quick run.
`vite.config.ts` includes every source file in the report and prints it rather
than writing files:

```ts
/// <reference types="vitest/config" />
import { defineConfig } from "vite";

export default defineConfig({
    test: {
        coverage: {
            include: ["src/**"],
            reporter: ["text"],
        },
    },
});
```

### Hooks and CI

The Git hooks and CI both run the following. For a project in a subdirectory,
CI sets the job's working directory, and the hooks replace `pnpm` with
`pnpm --dir <path>`. pnpm's own commands then go through `exec`, as in
`pnpm --dir <path> exec pnpm install --frozen-lockfile`, so that the pinned pnpm
runs them:

| Hook ID                  | Command                               |
|--------------------------|---------------------------------------|
| `pnpm-lock`              | `pnpm install --frozen-lockfile`      |
| `pnpm-peer-dependencies` | `pnpm peers check`                    |
| `prettier`               | `pnpm exec prettier --check .`        |
| `eslint`                 | `pnpm exec eslint . --max-warnings 0` |
| `tsc`                    | `pnpm exec tsc -b --noEmit`           |
| `vite-build`             | `pnpm exec vite build`                |
| `vitest`                 | `pnpm exec vitest run --coverage`     |

- `tsc` runs in build mode (`-b`) here and in the scripts, since otherwise it
  checks nothing in a solution-style `tsconfig.json`.
- `vite-build` catches errors that only bundling reveals, such as a missing
  stylesheet or asset, which `tsc` accepts, or a broken script in `index.html`.
- CI has no `pnpm-lock` job, since pnpm installs with `--frozen-lockfile` in CI
  anyway.
