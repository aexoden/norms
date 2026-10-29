# AGENTS.md

norms is two things: `STANDARDS.md`, the standards for the author's projects,
and the `norms` Rust CLI that checks a project against them
(`norms verify <path> [--json]`). The repository also ships a Copier template
(`copier.yml`, `template/`) that renders new projects following the standards.
The repository follows its own standards, so `STANDARDS.md` is the reference for
conventions here too.

## Commands

The Rust toolchain is pinned in `rust-toolchain.toml`. These match the
pre-commit hooks and CI:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test <name>                       # a single test, by name substring
cargo deny check bans licenses sources  # cargo-deny is installed separately
cargo run -- verify <path>              # run norms against a project
pre-commit run --all-files              # every hook, incl. reuse, markdownlint, zizmor
```

Clippy runs `pedantic`, `nursery` and `cargo` with `-D warnings`, plus the
restriction lints listed in `Cargo.toml` (see the Lints section of
`STANDARDS.md`). Among them, `unwrap`/`expect`/`panic` and indexing are flagged
outside tests, so return errors or handle `None` instead, and crate-internal
items are `pub(crate)`.

Rendering the template locally, as the CI `template` job does (it needs
`--vcs-ref HEAD`, so commit first; answers are passed with `--data`):

```sh
git init /tmp/demo
uvx copier copy --trust --defaults --vcs-ref HEAD \
  --data project_name=demo-project --data "description=A demo." \
  --data primary_language=rust . /tmp/demo
git -C /tmp/demo add --all && (cd /tmp/demo && uvx pre-commit run --all-files)
```

## Architecture of the CLI

`cli::verify` builds a `ProjectContext`, runs `checks::run_all_checks`, and
prints a `Report`, exiting 1 if any check failed.

- `context.rs`: `ProjectContext` lazily parses each config file at most once
  (`OnceLock`) and returns a `ConfigStatus<T>` (`NotFound`, `ParseError` with
  the raw text, or `Ok`). Checks share parsed data through it rather than
  reading files themselves.
- `config/`: serde models of only the fields checks inspect, with
  `#[serde(default)]` so missing keys don't fail parsing.
- `checks/`: plain functions `fn(&ProjectContext, &mut Report)`. Registration is
  explicit: a new check must be added to the list in `checks/mod.rs`.
  `common.rs` runs for every project; the language modules run per detected
  language.
- `report.rs`: `report.must(...)` records a failure, `report.should(...)` a
  warning, `report.skip(...)` a skip; `require_parsed`/`recommend_parsed` unwrap
  a `ConfigStatus` while recording the problem if it's missing or malformed.
- `detection.rs`: languages come from marker files at the root (`Cargo.toml`,
  `pyproject.toml`, `package.json`, `CMakeLists.txt`); TypeScript may also live
  in `frontend/`. C++ is detected but has no checks yet.

`norms verify` is not yet gating in CI: it still reports false findings on
projects rendered from the template.

## Template

- `copier.yml` holds the questions and post-copy `_tasks` (lock file
  generation); it `!include`s `versions.yml`, so it is excluded from the
  `check-yaml` hook.
- `versions.yml` holds every version the template pins, as hidden variables.
  Each entry's `# renovate:` comment feeds the regex managers in
  `renovate.json`; keep that comment format when adding entries. Actions are
  pinned as `"<sha> # <version>"` with a full `vX.Y.Z` tag, and use the
  `github-actions` versioning so Renovate keeps that tag in step with the
  workflows' pins instead of picking a floating `vX` tag on the same commit.
  `tests/versions.rs` fails if an action entry doesn't match the actions
  manager's pattern, lacks a full tag, or isn't pinned identically in
  `.github/workflows/`, so every template action must also be used there.
- Template paths use Jinja names for conditional files and per-language
  directories (`{{ rust_dir }}`, `{{ python_dir }}`, `{{ typescript_dir }}`),
  since any language can be primary (at the root) or secondary (in a
  subdirectory).
- When changing a standard, update `STANDARDS.md`, the template and, where
  relevant, the checks in `src/checks/` together.

## Conventions

- Every source file starts with SPDX headers
  (`MIT OR Apache-2.0`, `Jason Lynch <jason@aexoden.com>`, year created);
  Markdown and files without comments are covered by `REUSE.toml`.
- Markdown follows `.markdownlint-cli2.jsonc`: dash lists, prose wrapped at 80
  columns.
- Commits use Conventional Commits with the types listed in `STANDARDS.md`;
  the subject is imperative and lowercase, aiming for 50 characters and never
  over 72.
- Dependencies in `Cargo.toml` are pinned exactly (`=x.y.z`). New dependencies
  must have permissive licenses allowed by `deny.toml`.
- `CHANGELOG.md` is maintained by hand in Keep a Changelog format, with no
  `Unreleased` section.
