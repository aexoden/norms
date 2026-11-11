# AGENTS.md

norms is three things in one repository:

- `STANDARDS.md`, the standards for the author's projects. norms follows them
  itself, so it is the reference for conventions here too.
- The `norms` Rust CLI. `norms verify <path> [--json] [--deny-warnings]` checks
  a project against the standards, and `norms commit-msg <file>` checks a commit
  message's line lengths. Other projects run them as the `norms-verify` and
  `norms-commit-msg` hooks from `.pre-commit-hooks.yaml`; norms runs both on
  itself as local hooks, which `tests/hooks.rs` keeps in step with the shipped
  ones.
- A Copier template (`copier.yml`, `template/`) that renders new projects
  following the standards.

When changing a standard, update `STANDARDS.md`, the template and, where
relevant, the checks in `src/checks/` together.

## Commands

The Rust toolchain is pinned in `rust-toolchain.toml`. These match the
pre-commit hooks and CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test <name>                         # a single test, by name substring
NORMS_BLESS=1 cargo test --test fixtures  # rewrite the fixture snapshots
# cargo-deny is installed separately
cargo deny --workspace --all-features check -D warnings bans licenses sources
cargo run -- verify <path>                # run norms against a project
cargo run -- verify --deny-warnings .     # run norms against itself
pre-commit run --all-files                # every hook, incl. reuse, markdownlint, tombi, zizmor
```

Clippy runs `pedantic`, `nursery` and `cargo` with `-D warnings`, plus the
restriction lints listed in `Cargo.toml` (see the Lints section of
`STANDARDS.md`). Among them, `unwrap`/`expect`/`panic` and indexing are flagged
outside tests, so return errors or handle `None` instead, and crate-internal
items are `pub(crate)`.

To render the template locally, much as the CI `template` job does:

```sh
git init /tmp/demo
uvx copier copy --trust --defaults --vcs-ref HEAD \
  --data project_name=demo-project --data "description=A demo." \
  --data primary_language=rust . /tmp/demo
git -C /tmp/demo add --all
(cd /tmp/demo && SKIP=norms-verify uvx pre-commit run --all-files)
cargo run -- verify --deny-warnings /tmp/demo
```

`--vcs-ref HEAD` renders the last commit, so commit template changes first. The
render's `norms-verify` hook would build the released norms the template pins,
so it is skipped and this checkout's norms runs instead. pre-commit still
clones that pinned release, which in a release commit isn't tagged yet, so
there the hooks fail to install; the CI job runs them from a copy of the config
whose norms hooks come from this checkout (see its `sed` step in `ci.yaml`),
leaving the render's own config as it is. The CI job runs
`norms verify --deny-warnings` on each render, so a render must have no
failures or warnings.

## Architecture of the CLI

`cli::verify` builds a `ProjectContext`, runs `checks::run_all_checks`, and
prints a `Report`, exiting 1 if any check failed, or with `--deny-warnings` if
any check failed or warned.

- `context.rs`: `ProjectContext` parses each config file at most once and
  returns a `ConfigStatus<T>` (`NotFound`, `ParseError` or `Ok`). Checks read
  files through it rather than themselves. It holds the repository-wide files
  and the detected components; each component's own files (`Cargo.toml`,
  `package.json`, ...) are in its `ComponentContext`.
- `config/`: serde models of only the fields checks inspect, with
  `#[serde(default)]` so missing keys don't fail parsing. `norms.toml`'s model
  is the exception: it denies unknown fields.
- `checks/`: plain functions `fn(&ProjectContext, &mut Report)`, or
  `fn(&ComponentContext, &mut Report)` for a check of a component's own files
  (also given the `ProjectContext` when a repository-wide file applies to that
  component alone, such as the CI job testing a library's MSRV).
  `common/` runs for every project. For each detected language, the component
  checks run once per component, then the checks of the language's place in
  repository-wide files (pre-commit hooks, CI) run once. Registration is
  explicit: add a new check to the list in `checks/runner.rs`.
- `report.rs`: `report.must(id, name, outcome)` records a check whose issue is
  a failure, `report.should(...)` one whose issue is a warning, and
  `report.check(...)` one whose issues differ in severity; the module's
  Severity section gives the rule for choosing. `require_parsed` unwraps a
  `ConfigStatus`, recording a failure if the file is missing or malformed.
- Every check has a stable kebab-case ID, subject then property
  (`cargo-toml-license`), which waivers in `norms.toml` name. Don't rename an
  ID a release has shipped: that is a breaking change. The name beside it is
  free text.
- `detection.rs`: components come from marker files (`Cargo.toml`,
  `pyproject.toml`, `package.json`, `CMakeLists.txt`) anywhere in the project.
  The walk honors `.gitignore`, and skips hidden directories, build output,
  installed dependencies and the directories `norms.toml` excludes (norms' own
  excludes `tests/fixtures`). A marker nested in a component of the same
  language is folded into it. C++ is detected but has no checks yet.
- `cargo_workspace.rs`: the members of a Cargo workspace aren't components.
  Their results belong to the workspace's component and name the member's
  manifest (`crates/cli/Cargo.toml: ...`).
- `git.rs`: runs `git` for the release tags the changelog check needs. Only a
  project at the top of its repository has any, so the fixtures don't see
  norms' own tags.
- `commit_msg.rs`: `norms commit-msg`, tested in `tests/commit_msg.rs`.

## Tests

Unit tests sit beside the code they test. `tests/verify.rs` checks
`norms verify`'s exit codes on a copy of a fixture. `tests/fixtures.rs` runs
`norms verify --json` on each project in `tests/fixtures/` and compares what it
reports with `tests/snapshots/<name>.txt`, so any change in findings shows up as
a snapshot diff. When the change is intended, rerun with `NORMS_BLESS=1`, review
`git diff tests/snapshots`, and commit the snapshots with the change.

- A fix or new check that depends on a project's layout gets a fixture.
  `tests/README.md` records what each fixture is for and how the language
  fixtures are rendered from the template. Read it before adding or changing
  one, to find the fixture that already covers the area, and update it in the
  same commit.
- Re-render the language fixtures when the template changes in a way the checks
  see.
- Fixtures are kept exactly as written: pre-commit, markdownlint and Tombi
  skip them, and `REUSE.toml` covers them, so one may be deliberately broken or
  lack headers.

## Template

- `copier.yml` holds the questions and post-copy `_tasks` (lock file
  generation). It `!include`s `versions.yml`, so it is excluded from the
  `check-yaml` hook.
- `versions.yml` holds every version the template pins, as hidden variables.
  Each entry's `# renovate:` comment feeds the regex managers in
  `renovate.json`, so keep that comment format when adding entries.
- `versions.yml` pins actions as `"<sha> # <version>"` with a full `vX.Y.Z`
  tag. `tests/versions.rs` fails unless each is pinned identically in
  `.github/workflows/`, so every template action must also be used by norms'
  own workflows.
- A unit test in `src/config/markdownlint.rs` fails when
  `hook_markdownlint_cli2` changes: compare its `RULES` table with the rules of
  the markdownlint the new release depends on, then update
  `RULES_MARKDOWNLINT_CLI2` there. Nothing catches an update of
  `action_markdownlint_cli2` alone, whose markdownlint may differ, so review
  the rules for that too.
- Template paths use Jinja names for conditional files and per-language
  directories (`{{ rust_dir }}`, `{{ python_dir }}`, `{{ typescript_dir }}`),
  since any language can be primary (at the root) or secondary (in a
  subdirectory).

## Conventions

- Every source file starts with SPDX headers
  (`MIT OR Apache-2.0`, `Jason Lynch <jason@aexoden.com>`, year created);
  Markdown and files without comments are covered by `REUSE.toml`.
- Markdown follows `.markdownlint-cli2.jsonc`: dash lists, prose wrapped at 80
  columns.
- Commits use Conventional Commits with the types listed in `STANDARDS.md`;
  the subject is imperative and lowercase, aiming for 50 characters and never
  over 72. A commit that adds a `**Breaking:**` changelog entry, or makes an
  entry breaking, adds `!` before the colon (`feat!: ...`).
- TOML files are formatted by Tombi (the `tombi-format` hook), which also keeps
  dependencies in alphabetical order.
- Dependencies in `Cargo.toml` are pinned exactly (`=x.y.z`). New dependencies
  must have licenses allowed by `deny.toml`.

## Changelog and Releases

`CHANGELOG.md` is maintained by hand in Keep a Changelog format, as the
Changelog section of `STANDARDS.md` describes.

- A change that users of norms or the template would notice gets an entry under
  `Unreleased`, in the commit that makes it and under the type of change it is,
  written for those users rather than as the commit's subject.
- When a later change alters or reverts something still under `Unreleased`,
  update or drop its entry. A released version's entries stay as they are, and
  the later change gets an entry of its own.
- A change that breaks what the version numbers cover (see Versioning in
  `README.md`) starts with `**Breaking:**` and says briefly how to upgrade.

At a release, in one commit:

- Check `Unreleased` against the commits since the last tag for anything it
  missed.
- Rename it to the new version with the date, add an empty `Unreleased` above
  it, and update the link definitions: the new version compares the previous
  tag with the new one, and `Unreleased` the new tag with `HEAD`.
- Set `hook_norms` in `versions.yml` to the new tag, so that projects rendered
  from the release run its hooks (`tests/versions.rs` fails until it matches
  the latest release in `CHANGELOG.md`).
