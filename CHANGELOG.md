# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/2.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Warn when a package in a Python library's `src/` has no `py.typed` marker
  (`python-py-typed`), without which type checkers ignore the library's
  annotations, or when a published library doesn't list the `Typing :: Typed`
  classifier (`pyproject-typed-classifier`), as the standards require. Add
  them, as the template does.
- Warn when `audit.yaml` runs `uv audit` without `--locked`
  (`python-audit-uv`), which relocks a stale `uv.lock` rather than failing, and
  audits versions that aren't committed. Run `uv audit --locked`, as the
  template does.
- Warn when a tsconfig doesn't set `"noEmit": true` (`tsconfig-no-emit`), as
  the standards require: without it, the `build` script's `tsc -b` writes
  JavaScript beside the sources. Set it in `tsconfig.app.json` and
  `tsconfig.node.json`, as the template does.
- Warn when `[tool.basedpyright]` turns a rule off (`false`, `"none"`,
  `"information"` or `"hint"`) without a comment on its line saying why
  (`basedpyright-rule-reasons`), as the standards require. Add one, as in
  `reportAny = false  # The JSON API returns Any.` `basedpyright-mode` also
  warns on `failOnWarnings = false`, which lets the warnings most rules report
  pass.
- Warn when a hook that runs before each commit isn't in its priority group
  (`prek-priorities`): the lock file hooks in `priority = "lock"`, the builtin
  hooks and `reuse` in none, and the rest in `priority = "check"`. Otherwise a
  hook runs alongside hooks it shouldn't, such as `reuse` while others create
  caches, and fails only now and then. Set the priorities the template does.
- **Breaking:** `norms commit-msg` fails on a breaking change footer written
  `BREAKING CHANGE:`, with a space, as the Conventional Commits specification
  allows: git doesn't read it as a trailer, and then reads none of the footers
  beside it, such as `Co-Authored-By`. Write `BREAKING-CHANGE:`.
- Warn when the repository has a `.github/dependabot.yml` or
  `.github/dependabot.yaml` (`dependabot-absent`), which has Dependabot open
  version update pull requests beside Renovate's. Remove it. `yaml-extension`
  no longer asks for it to be renamed.
- Warn when the `pnpm-lock` or `pnpm-peer-dependencies` hook of a TypeScript
  component in a subdirectory runs pnpm with `--dir` rather than through
  `exec pnpm` (`typescript-prek-pinned-pnpm`). pnpm switches to the version
  `devEngines` pins only in the directory it starts in, so the hook ran
  whichever pnpm was installed. Write `pnpm --dir <path> exec pnpm install
  --frozen-lockfile`, as the template does.
- Warn when a hook that should only check rewrites files: a `tombi-format`
  hook without `--check` (`prek-tombi`), or a lock file hook that updates a
  stale lock file rather than failing on it, a `cargo-lock` hook without
  `cargo update --workspace --locked` (`rust-prek-cargo-lock`), a `uv-lock`
  hook without `uv lock --check` (`python-prek-uv-lock`) or a `pnpm-lock` hook
  without `pnpm install --frozen-lockfile` (`typescript-prek-pnpm-lock`). Add
  the flags, as the template does.
- **Breaking:** Fail when `ci.yaml`'s gate job isn't named `CI success`
  (`ci-gate-name`), the check branch protection requires: GitHub shows a job's
  check by its `name`, so the required check would never report. Set
  `name: CI success`.
- Warn when `pyproject.toml` has Ruff ignore a rule beyond the template's,
  in `ignore` or `per-file-ignores`, without a comment on the rule's line
  saying why (`ruff-lint-ignore-reasons`), as the standards require. Add one,
  as in `"line-contains-todo",  # Tracked in issues instead.`
- Warn when a `package.json` depends on `eslint-plugin-prettier`
  (`package-json-no-eslint-plugin-prettier`), which runs Prettier again inside
  ESLint. Remove it and its import from `eslint.config.ts`.
- Warn when a CI job that installs Rust with
  `actions-rust-lang/setup-rust-toolchain` adds a `Swatinem/rust-cache` step
  (`rust-ci-separate-cache`), which setup-rust-toolchain already runs. Remove
  the step.
- Warn when a language hook runs its tool through Devbox (`devbox run`) or a
  shell (`bash -c "cd web && ..."`) (`rust-prek-local-hooks`,
  `python-prek-local-hooks`, `typescript-prek-local-hooks`). Run the tool
  directly, reaching a component in a subdirectory with `--manifest-path`,
  `uv --directory` or `pnpm --dir`, as the template does.
- Warn when `deny.toml` ignores an advisory that never expires
  (`deny-toml-ignore-expiry`): give it an `expiry`, a date or a duration from
  the advisory's publication such as `"P90D"`, or set a default with
  `[advisories] ignore-expiry`. Both need cargo-deny 0.21.0 or later.
- The template's `norms-verify` job caches prek's hook environments with
  `actions/cache`, keyed on the prek version and `prek.toml`, so that it
  builds norms only when either changes rather than on every run.
- The template's `uv-lock` job and `uv audit` job set `enable-cache: false`,
  and norms warns on a job that runs only `uv lock` or `uv audit` with
  setup-uv's cache on (`python-ci-uv-cache`). setup-uv shares one cache among
  all jobs on the same Python version, in every workflow, so such a job,
  finishing first, saved it without the packages the others install. Add
  `enable-cache: false` to the `astral-sh/setup-uv` step of those jobs.
- Set Cargo's own `pedantic` lint group, new in Rust 1.100, to `warn` in the
  template's `[lints.cargo]`, and warn when a package doesn't
  (`cargo-lints`). Add the table as the template does, or inherit it from
  `[workspace.lints.cargo]`.
- Add `norms github <path>`, which checks the GitHub repository's rulesets
  and settings against `STANDARDS.md`'s GitHub Repository section, reading
  them through the GitHub CLI (`gh`). Its checks' IDs start with `github-`,
  and `norms.toml` waives them as it does the others; `norms verify` ignores
  those waivers.
- `copier copy` points to the section of `STANDARDS.md` on configuring the
  new project's GitHub repository, which the template can't do.
- Add a `SECURITY.md` to the template, which asks for vulnerabilities to be
  reported through the repository's private vulnerability reporting and says
  that only the latest release (or, without releases, `main`) gets fixes.
- **Breaking:** Fail when a project has no `SECURITY.md` at its root
  (`security-md-exists`). Add the one the template renders.
- Add the `norms-commit-msg` and `norms-verify` hooks to the template, with a
  `norms-verify` CI job that runs the pinned hooks: `norms-verify`, and the
  commit message hooks on the message of the commit CI tests, rejecting
  autosquash and merge subjects and only warning about long lines.
- **Breaking:** Check that projects run norms: the `norms-commit-msg` and
  `norms-verify` hooks, the latter with `--deny-warnings`, a CI job running
  `norms verify` with the whole history, and a CI step running the commit
  message hooks (`prek run --stage commit-msg`). Add them as the template
  does.
- Add `--long-lines warn` to `norms commit-msg`, also set by
  `NORMS_COMMIT_MSG_LENGTH=warn`, which reports lines over 72 characters
  without failing, for checking a commit that has already been made. A type
  not in lowercase still fails.
- Add a `build.rs` to the template's Rust applications that sets
  `BUILD_VERSION` to the version `git describe` gives, such as
  `0.2.0-5-gf8a4d2b` between releases, which `--version` reports.
- Set `__APP_VERSION__` in the template's TypeScript components to the version
  `git describe` gives, the same way, through Vite's `define`.
- **Breaking:** Check that every `Cargo.toml` and `package.json`, those of
  Cargo workspace members and pnpm workspace packages included, carries the
  repository's one version: that of the latest release in `CHANGELOG.md` (or,
  without one, the latest release tag), or `0.0.0` before the first release.
  Set `version` to it, and change it in the commit that makes each release.
- **Breaking:** Check the packages of a TypeScript component's pnpm workspace,
  those its `pnpm-workspace.yaml`'s `packages` patterns match, as the
  component's own `package.json` is checked: each must parse, carry the
  release's version, use ESM, be private, pin its dependencies exactly, and
  leave out Corepack's `packageManager`. Fix or waive what each package reports.
- Warn about YAML files named `.yml`, anywhere in the project but `.git`,
  `node_modules`, `target`, `.venv` and the directories `.gitignore` or
  `[components] exclude` leave out. Rename them to
  `.yaml`; `.copier-answers.yml`, `.github/FUNDING.yml` and
  `.github/ISSUE_TEMPLATE/config.yml` keep their names, the only ones Copier
  and GitHub document for them.
- Warn about Prettier configuration in a TypeScript component's subdirectories,
  such as a `.prettierrc` there or a `prettier` key in a workspace package's
  `package.json`, which takes the place of the component's `.prettierrc` for the
  files below it. Directories Prettier skips, those its `.gitignore` and
  `.prettierignore` match included, aren't searched, while those `[components]
  exclude` lists are, as Prettier formats them.
- **Breaking:** Settle the Clippy restriction lints in `STANDARDS.md`, and warn
  when a package, or a member of its workspace, leaves out any of them
  (`clippy-restriction-lints`). Set each, as the template does. Setting one to
  `allow` is accepted as a deliberate choice.
- **Breaking:** Type check Python with basedpyright beside mypy: the template
  adds it to the `dev` group, sets `typeCheckingMode = "recommended"` in
  `[tool.basedpyright]`, and runs `uv run basedpyright` in a `basedpyright`
  hook and CI job. Check the hook and the CI step, and warn when
  `[tool.basedpyright]` doesn't set that mode or a `pyrightconfig.json` takes
  its place. Add them as the template does, and fix or turn off, with a
  comment, what basedpyright reports.
- Warn when no hook, or no CI step, runs the doctests of a Rust library
  target, beside a binary or not, as `cargo test` does unless it selects other
  targets.
- Measure Rust test coverage with cargo-llvm-cov, as Python and TypeScript
  already do: the `cargo-test` hook and CI job run `cargo llvm-cov --workspace
  --all-features --show-missing-lines`, `rust-toolchain.toml` adds the
  `llvm-tools` component, and libraries get a `cargo-doctest` hook and job
  running `cargo test --workspace --all-features --doc`, since cargo-llvm-cov
  runs doctests only on nightly. Warn when the `cargo-test` hook or CI tests
  run without coverage, or `rust-toolchain.toml` lacks `llvm-tools`. Install
  [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov#installation) to
  run the hook.
- Warn about a remote repository in `prek.toml` whose `rev` the shared Renovate
  preset can't update: one not written as `repo = "https://github.com/..."`
  with `rev = "..."` on the next line, as with inline tables.
- Warn about a tool version in a workflow that Renovate can't update: one
  written inline, as in `taiki-e/install-action`'s `tool: x@1.2.3`,
  `uvx x@1.2.3`, `uv run --with x==1.2.3`, `cargo install x --version 1.2.3`
  or `pnpm dlx x@1.2.3`, or an environment variable ending in `_VERSION`
  without a `# renovate:` comment above it. Pin each in such a variable under
  such a comment.
- `README.md` documents how to run norms: its hooks and commands, what its
  results and exit codes mean, `norms.toml`, and how it finds components.

### Changed

- `ci-yaml-push` warns when `ci.yaml`'s `push` has no branch filter, which
  runs CI on every branch, rather than accepting it. Name the branches, as in
  `push: branches: [dev, main]`.
- **Breaking:** A Python component that uv doesn't build
  (`[tool.uv] package = false`), such as a service run with `uv run`, is an
  application: its dependencies are pinned exactly, its `requires-python` names
  the pinned version, and it needs no `pytest-min-python` job. Some checks
  treated it as a library. Pin its dependencies with `==`, and set
  `requires-python` to `==` the version in `.python-version`.
- `STANDARDS.md` describes only the standards, more concisely, leaving how
  norms checks them to the README and the code. It's reorganized, with the
  GitHub settings in a GitHub Repository section and each language's
  standards in a section of its own.
- The template's `.editorconfig` indents JSONC files by two spaces, as JSON
  files are, and its `.markdownlint-cli2.jsonc` is indented by two spaces
  rather than four. `editorconfig-indent-size-data` warns unless `*.jsonc` gets
  `indent_size = 2`: add `jsonc` to the `[*.{yml,yaml,json,toml}]` section,
  and reindent `.markdownlint-cli2.jsonc` by two spaces.
- In a project with TypeScript at the root, the template's Prettier formats
  `.markdownlint-cli2.jsonc` too, rather than `.prettierignore` leaving it out,
  and the template writes the file as Prettier does, with trailing commas. To
  follow, remove the file from `.prettierignore` and run
  `pnpm exec prettier --write .markdownlint-cli2.jsonc`.
- `deny-toml-exists` warns on a `.config/deny.toml`, which cargo-deny reads
  since 0.21.0, as it does on `.deny.toml` and `.cargo/deny.toml`: rename it
  `deny.toml`. Before, norms looked past it to a `deny.toml` further up.
- The template's `ci.yaml` lists the `ci-success` gate job last, after the
  jobs it waits for, rather than first, as `STANDARDS.md` now describes it.
  Job order doesn't change how CI runs.
- `STANDARDS.md`'s Urgent Updates section says that a package manager still
  applies its release age to Renovate's vulnerability alert PRs, which show an
  artifact error until their branch updates the package early. For Cargo, it
  says to raise an exact pin before running the override, that the override
  takes the update's new dependencies unaged, and that until the release passes
  the release age, `cargo update` without it, Renovate's lock file maintenance
  included, fails on the pin or downgrades the package again.
- Deny Clippy's warnings with `cargo clippy --config 'build.warnings="deny"'`
  rather than `-- -D warnings`, which leaves Cargo's own lints, new in Rust
  1.100, such as `unused_dependencies`, as warnings that pass. `norms verify`
  warns on a `clippy` hook or CI job that still uses `-D warnings`, or denies
  warnings through `RUSTFLAGS` or `CARGO_BUILD_WARNINGS`. Change the command as
  the template does, on Rust 1.97 or later, since earlier toolchains ignore
  `build.warnings`.
- **Breaking:** Run the Git hooks with [prek](https://prek.j178.dev/) rather
  than pre-commit, configured in `prek.toml` rather than
  `.pre-commit-config.yaml`, which norms no longer reads. Language hooks have
  `language = "system"`. CI installs prek with `taiki-e/install-action` at the
  version in `PREK_VERSION`, and runs `prek run` in a `prek` job, in the
  `norms-verify` job, where it also runs `prek run --stage commit-msg`.
  To upgrade, install prek, run `prek util yaml-to-toml`, lay the result out as
  the template does, with a `[[repos]]` table for each repository and a
  `[[repos.hooks]]` table for each hook, delete `.pre-commit-config.yaml`, run
  `prek install` to replace pre-commit's Git hooks, and update the workflows as
  the template does, setting `PREK_SKIP` rather than `SKIP`. prek builds the
  `norms-verify` hook with `cargo install --locked`, so with the dependency
  versions norms' `Cargo.lock` pins.
- **Breaking:** Run the common hooks, such as `check-yaml` and
  `trailing-whitespace`, from prek's builtin repo (`repo = "builtin"`) rather
  than pre-commit-hooks, so that prek needn't clone it and install Python for
  it, and warn about a common hook from another repo. Replace the
  pre-commit-hooks repo with `repo = "builtin"` and drop its `rev`.
- **Breaking:** Rename the checks of the hook configuration for prek:
  `precommit-*` becomes `prek-*` (`precommit-pre-commit-hooks` becomes
  `prek-builtin-hooks`), `rust-precommit-*`, `python-precommit-*` and
  `typescript-precommit-*` become `rust-prek-*`, `python-prek-*` and
  `typescript-prek-*`, and `ci-pre-commit` and `ci-pre-commit-skip` become
  `ci-prek` and `ci-prek-skip`. Rename any waivers of them in `norms.toml`.
- The template's hooks run concurrently with prek's hook priorities: the lock
  file checks first (`priority = "lock"`), then every other hook outside the
  builtin repository (`priority = "check"`) except `reuse`, which runs alone
  before them so that caches the other hooks create don't fail it. This makes
  `prek run` about three times faster on a project in all three languages. To
  adopt them, copy the template's `[priorities]` table and the `priority` of
  each hook.
- The template's `prek.toml` sets `minimum_prek_version` to 0.4.11, the oldest
  prek that runs it, so that an older prek reports that it is too old rather
  than failing on the priority aliases.
- `norms --version` reports the version `git describe` gives in a build from
  a git checkout, such as `0.2.0-5-gf8a4d2b` between releases.
- The template starts Rust packages at version `0.0.0`, as TypeScript ones
  already were, until the first release.
- `STANDARDS.md` marks a breaking change in a commit message with `!` before
  the colon, matching the changelog's `**Breaking:**` entries, writes commit
  types in lowercase, and gives footers git's trailer format.
- `norms commit-msg` rejects a commit type that isn't in lowercase, such as
  `Feat:`, which `conventional-pre-commit` lets through.
- `STANDARDS.md` keeps GitHub's default squash merge message rather than
  "Pull request title and commit details", so that a pull request with a single
  commit is merged with the commit message CI checks. Set it back to "Default
  message" in the repository's settings.
- `STANDARDS.md` settles the commit types of dependency updates on Renovate's
  defaults: `fix(deps)` for runtime dependencies, `chore(deps)` for the rest.
- `STANDARDS.md` allows an optional `TODO.md` and `ROADMAP.md` for open work.
- `STANDARDS.md` keeps `.gitignore` to what the project generates, leaving
  editor and OS files to each developer's global ignore file.
- `STANDARDS.md` protects a `dev` branch with a ruleset of its own,
  `Development`, which restricts deletions and requires linear history and
  signed commits, but allows force pushes. Add it to a repository that keeps
  `dev`.
- `STANDARDS.md` lists the GitHub settings each repository sets besides its
  rulesets: the Renovate app; Wikis and Projects off unless used; only squash
  merging, always suggesting branch updates and deleting head branches after
  merge, without auto-merge; read-only workflow permissions, without Actions
  approving pull requests, and actions pinned to commit SHAs; and private
  vulnerability reporting, Dependabot alerts without its security updates, and
  secret scanning with push protection. Set them in the repository's settings.
- The template's Rust packages enable Clippy's `unnecessary_rest_pattern` in
  place of `rest_pat_in_fully_bound_structs`, which it covers along with
  struct variants of enums.
- `STANDARDS.md` asks for an operating system matrix in CI only across the
  platforms a project supports.
- The template's `.gitignore` also ignores rustc's internal compiler error
  reports (`rustc-ice-*.txt`), pytest-cov's HTML report (`htmlcov/`), and
  Vite's local environment files (`.env.local`, `.env.*.local`), which hold
  secrets. `STANDARDS.md` counts such local files with what a project
  generates. Add the entries for the project's languages.

### Removed

- `tsconfig-reference-any-compiler-options`, which failed a second time when
  a referenced tsconfig was missing, malformed or without `compilerOptions`,
  each of which `tsconfig-reference-exists`, `tsconfig-reference-valid` or
  `tsconfig-reference-compiler-options` already fails. Remove any waiver of
  it.

### Fixed

- Don't report that a Python component without a `[project]` table, which
  `pyproject-project` already fails, lacks a job testing its oldest Python
  (`python-ci-min-python`): it isn't a library.
- Don't count `ruff check --fix` as checking: it fails `python-ci-ruff-check`
  in CI, where it fixes what it can and passes, and warns in the `ruff-check`
  hook (`python-prek-ruff-check`), where it rewrites files. Run `ruff check`
  without `--fix`, as the template does.
- Check the values of Ruff's settings, not only that they're set: warn unless
  `ignore` lists `missing-trailing-comma` (`ruff-lint-ignores`), pydocstyle's
  `convention` is `google` (`ruff-lint-pydocstyle`), and isort's
  `lines-between-types` is 1 (`ruff-lint-isort`), as the standards give them.
- Fail `rust-ci-rustfmt`, rather than only warning about the step's form, when
  CI runs `cargo fmt` without `--check` in place of
  `actions-rust-lang/rustfmt`: such a step rewrites the files and passes every
  commit. Use the action, as the template does.
- Fail `rust-ci-test` when CI runs only the doctests (`cargo test --doc` or
  `cargo llvm-cov --doc`) or builds the tests without running them
  (`cargo test --no-run`), rather than passing: the unit and integration tests
  never ran. Run them with `cargo llvm-cov`, as the template does.
- Require a committed `Cargo.lock` in Rust libraries too (`cargo-lock-exists`),
  as the standards require of every project, not only in applications. Commit
  the lock file.
- The template's `.gitignore` ignores `.coverage.*`, the files coverage.py
  writes beside `.coverage` while pytest runs. With a TypeScript component at
  the root, Prettier checked them too, and failed when one was gone before it
  was read. Add `.coverage.*` below `.coverage`.
- The template's workflows pin prek and cargo-llvm-cov in `_VERSION`
  environment variables under `# renovate:` comments, which the shared Renovate
  preset now updates, rather than inline in commands, where Renovate didn't.
- Recognize Cargo commands that pass global options before the subcommand,
  such as `cargo --locked test` or `cargo -C rust clippy`, in the Rust hook and
  CI checks.
- Split the commands of workflow `run` scripts and `package.json` scripts,
  which the checks read for the tools they run, as the shell splits them: a
  quoted word stays whole without its quotes, and a shell operator or `#`
  inside quotes doesn't end the command. A command such as
  `echo 'a && uvx x@1.0'` no longer counts as running the quoted one.
- Read workflow `run` scripts as commands, not text, where CI must run
  `ruff check`, `mypy`, `uv audit`, `pnpm audit`, `tombi format --check`, a
  Vite build, or the `lint` or `test` script: a quoted or commented-out
  mention, such as `echo 'ruff check'` or `# uv audit`, no longer counts as
  running the tool, nor does a `--check` given to another command than
  `tombi format`, or a script whose name only starts the same, such as
  `pnpm lint:fix`.
- List `coverage/` and `dist/` in the `.prettierignore` of the template's
  TypeScript component in a subdirectory, since Prettier, run there, doesn't
  read the root `.gitignore`. The `prettier` hook no longer fails after a
  `vite build`.

## [0.2.0] - 2026-11-10

norms 0.2.0 replaces the Python prototype of 0.1.0 with a rewrite in Rust, and
adds the standards it checks, a Copier template for new projects, and pre-commit
hooks. Projects that passed 0.1.0 should expect findings from the new checks:
each says what it expects, `STANDARDS.md` explains the standard behind it, and
`norms.toml` can waive a check that a project departs from on purpose.

### Added

- Add `STANDARDS.md`, the standards `norms verify` checks projects against.
- Add a Copier template that renders new projects following the standards, for
  Rust, Python and TypeScript in any combination, with every version it pins
  kept in `versions.yml`.
- Add the `norms-verify` and `norms-commit-msg` pre-commit hooks.
- Add `norms commit-msg`, which checks a commit message's subject and body line
  lengths.
- Add `norms --version`, which prints the version of norms.
- Add `--deny-warnings` to `norms verify`, which then also fails when a check
  warns.
- Add `norms.toml`, which waives checks with a reason and excludes directories
  from component detection.
- Give every check a stable ID, shown in the text and JSON reports.
- Add the detected components, each result's component and the number of
  skipped checks to `norms verify --json`.
- Detect components anywhere in the project by their marker files, label each
  finding with its component, and check Cargo workspace members with their
  workspace. C++ components are detected but not yet checked.
- **Breaking:** Check the repository's own files: the changelog, `AGENTS.md` and
  `CLAUDE.md`, markdownlint's configuration, and REUSE licensing.
- **Breaking:** Check the pre-commit configuration: the required hooks and how
  they run, their stages, the installed hook types, and unique hook IDs.
- **Breaking:** Check the GitHub Actions workflows: the jobs CI must run and how
  they run them, the gate job, triggers, timeouts, concurrency, runners, zizmor
  and the scheduled audit.
- Check that `renovate.json` uses the shared preset and that `devbox.json`
  installs no toolchains.
- **Breaking:** Require a `devbox.lock` beside `devbox.json`.
- **Breaking:** Check Rust components: the toolchain pin, Cargo manifests and
  lints, exact dependency pins in applications, the release age, the Clippy and
  cargo-deny configuration, and that libraries test their MSRV.
- **Breaking:** Check Python components: the Python and uv pins, classifiers,
  exact dependency pins in applications, the release age, versions from git
  tags, the `src/` layout, the Ruff, mypy, pytest and uv configuration, kept in
  `pyproject.toml` rather than files of their own, and that libraries test their
  oldest Python.
- **Breaking:** Check TypeScript components: `package.json`, the Node.js and
  pnpm pins, exact dependency pins in applications, pnpm's release age and build
  settings, the tsconfig compiler options, the ESLint, Prettier, Vite and Vitest
  setup, and that each tool has one lock or configuration file.

### Changed

- **Breaking:** Rewrite norms in Rust. Run it through the hooks, or install it
  with `cargo install --git https://github.com/aexoden/norms --tag v0.2.0`,
  rather than as a Python package.
- License norms under MIT OR Apache-2.0.
- **Breaking:** Leave out a result's `message` in `norms verify --json` when it
  is empty, rather than give an empty string. Treat a missing `message` as
  empty.
- Check the properties `.editorconfig` gives each file and the line endings and
  diff drivers `.gitattributes` sets, rather than only a few `.editorconfig`
  settings and whether `.gitattributes` exists.
- Check `devbox.json` only when it exists, rather than requiring it.
- **Breaking:** Fail when `renovate.json` is missing, rather than warn.
- **Breaking:** Require license texts in `LICENSES/`, each named by its SPDX
  identifier and with an identical copy at the root, rather than a single
  `LICENSE` file: copy `LICENSE` to `LICENSES/`, under its license's identifier.

### Removed

- Stop warning when norms finds no language it checks.

## [0.1.0] - 2026-01-10

### Added

- Add initial version of the `norms` verification tool.

[Unreleased]: https://github.com/aexoden/norms/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/aexoden/norms/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/aexoden/norms/releases/tag/v0.1.0
