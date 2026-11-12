# Test Fixtures

Each directory in `tests/fixtures/` is a project for norms to check.
`tests/fixtures.rs` runs `norms verify --json` on each one and compares what it
reports with `tests/snapshots/<name>.txt`. `AGENTS.md` has the rules for
changing them; this file records what each fixture is for. Keep it current when
adding or changing a fixture.

## Template Renders

The language fixtures are renders of the template, made with the `copier copy`
command under Commands in `AGENTS.md`, with `--skip-tasks` added and the
answers below as its `--data`. Each drops `.copier-answers.yml` and has stub
lock files apart from `Cargo.lock`.

- `rust`, `python` and `typescript`: `primary_language=<language>` and
  `project_name=demo-<language>`.
- `rust-typescript`: `project_name=demo`, `primary_language=rust`,
  `secondary_languages=["typescript"]` and `typescript_path=frontend`.
- `python-rust-library`, the only library render: `project_name=demo-library`,
  `primary_language=python`, `project_type=library` and
  `secondary_languages=["rust"]`.

## Hand-Written Fixtures

- `empty` has only a `.gitkeep`.
- `devbox` has a services-only `devbox.json` beside a stub marker file for each
  language.
- `invalid-ci` has a malformed `ci.yaml` beside a stub `Cargo.toml`, with a
  malformed and a valid workflow beside it and files that aren't workflows,
  which norms skips.
- `components` has stub markers laid out to exercise detection: a Cargo
  workspace member and a nested pnpm package to fold, two TypeScript
  components, a Python one, and markers under `node_modules/`, a hidden
  directory and an excluded `vendor/` to skip; its `norms.toml` also excludes a
  missing directory. Of the TypeScript components, one has a single
  `tsconfig.json` and the other one that references only `tsconfig.app.json`.
- `unpinned` has applications whose manifests break the pinning standards (one
  also pins pnpm through Corepack's `packageManager`), beside a Python library
  that isn't checked for them, and release ages other than 3 days.
- `reuse` breaks the licensing standards: two license texts whose root copies
  are misnamed or differ, a `REUSE.toml` of the wrong version, hooks and CI
  that don't run `reuse lint` on the whole project, and a Rust, Python and
  TypeScript manifest whose licenses lack a text or aren't SPDX expressions.
- `toolchain` has two Rust components in subdirectories, one with a conforming
  `rust-toolchain.toml` of its own and a `.clippy.toml` and `.deny.toml`, which
  should be named `clippy.toml` and `deny.toml`, and one using the root's
  files, whose toolchain pins no exact channel and lacks clippy and
  llvm-tools, whose `clippy.toml` doesn't allow `panic` in tests and allows four
  path segments, and whose `deny.toml` conforms.
- `classifiers` has Python components whose classifiers are checked against
  `requires-python`: an application and a library with gaps in its range, each
  with its own `.python-version`, one capped by `requires-python` without a
  pin, one open-ended whose only pin is in its parent directory, which uv
  ignores, and one capped whose `.python-version` isn't a plain version.
- `python-pin` breaks the Python pinning standards: one component's
  `.python-version` pins only a minor version, two applications'
  `requires-python` name another version or a range, and a library's range
  excludes its pin.
- `workspace` is a virtual Cargo workspace with a binary and a library member
  matched by a glob, an excluded crate, a malformed member, a member found only
  as a path dependency, and an unrelated crate that isn't a member. The library
  matched by the glob is on the 2021 edition, while the binary and the path
  dependency inherit 2024. Its `CHANGELOG.md` has a release, whose version the
  binary and the path dependency inherit, while the library sets an older one.
- `pnpm-workspace` is a TypeScript component whose `pnpm-workspace.yaml` lists
  packages by a single-level glob, a recursive one, a negated one and one
  leading outside its directory. The packages it finds are one that conforms,
  one that breaks the package standards (an unknown license, no `type` or
  `private`, a range among its dependencies and a Corepack `packageManager`),
  a malformed one, one the recursive glob matches, and one inside it, in a
  `tests` directory, which pnpm doesn't skip. Of the `package.json` files it
  doesn't find, one is excluded by the negated pattern and one is in a
  directory no pattern names. Its `CHANGELOG.md` has a release, whose version
  the conforming packages carry, while the nonconforming one carries an older
  one and the one the recursive glob matches none.
- `msrv` is a virtual Cargo workspace of libraries whose members set
  `rust-version` themselves, inherit it from `[workspace.package]`, or lack it.
- `workflows` breaks the Actions standards, beside stub `Cargo.toml` and
  `pyproject.toml` files:
  - a Rust library target whose doctests no hook or CI step runs;
  - a zizmor hook that doesn't run its pedantic persona, and a CI step running
    zizmor itself beside a zizmor-action step that doesn't fail on findings,
    which decides;
  - a job without `timeout-minutes`, beside a call to a reusable workflow,
    which can't set one;
  - workflows whose `concurrency` is missing or doesn't cancel;
  - a `ci.yaml` that filters pull requests by branch and path and names `main`
    and a glob, but not `dev`, among its push branches;
  - a gate job and a zizmor-action job on `ubuntu-latest`, the gate without
    `if: always()` that misses a job and doesn't fail on every result, and
    that, like another job, sets `continue-on-error`;
  - a `norms-verify` job running the hook from a shallow checkout;
  - a `pr-title.yaml` on the default `pull_request` types, which leave out
    `edited`, that runs the `pre-commit` stage's hooks rather than
    `commit-msg`'s;
  - an unscheduled `audit.yaml` that runs `uv audit` but not cargo-deny's
    advisories check;
  - a `release.yaml` job that only checks formatting but leaves
    setup-rust-toolchain's cache on, beside jobs that build or turn it off.
- `precommit` has only a `.pre-commit-config.yaml`, which breaks the pre-commit
  standards: its `default_install_hook_types` lacks `pre-commit`, it runs only
  some of the pre-commit-hooks set, its `reuse` hook runs in the wrong stage
  and its `tombi-format` hook only in the `manual` one, its
  conventional-pre-commit hook runs in the wrong stage with other commit types
  than the standard's, its `norms-commit-msg` hook runs in the `pre-commit`
  stage and its `norms-verify` hook without `--deny-warnings`, and among remote
  and local hooks whose IDs or aliases collide, some have aliases of their own
  and some can't be selected alone.
- `markdown` breaks the Markdown standards: its `.markdownlint-cli2.jsonc`
  names rules by alias and tag, turns some off and lets others only warn, and
  sets other options than the standard's; its markdownlint-cli2 hook has
  another ID, and its CI action lints only the root's files. Its `CLAUDE.md`
  names `AGENTS.md` beside it, but only in prose and a code span, which don't
  import it.
- `versioning` has Python components in subdirectories, checked against the
  versioning standards: one that conforms but lacks the `raw-options` a
  component below the root needs, one with a static version and another build
  backend, whose `.git_archival.txt` has the older `ref-names` form and isn't
  marked `export-subst`, and one uv doesn't build (`package = false`), which
  the versioning checks skip. Its `ci.yaml` has setup-uv steps that pin uv
  exactly, that pin no version in a job running `uv`, and that pin none in a
  job only running `uvx`, which needn't, and the components' `required-version`
  is a minimum the pin satisfies, an exact version, or a minimum above the pin.
  Its `.gitattributes` sets `diff=python` on `*.py` but not on `*.pyi`.
- `hooks` has language hooks and a `ci.yaml` that come close to the standards'
  hook and CI tables, beside stub `pyproject.toml` and `package.json` files:
  - a `uv-lock` hook and CI step run through uv's global options, and of the
    jobs that `uv run`, one syncs with `--frozen`, one syncs with `--locked`
    only afterwards, and the others sync or run with `--locked`;
  - neither the `pytest` hook nor the job running pytest passes `--cov`, nor
    the `vitest` hook `--coverage`;
  - `pnpm peers check` runs through `pnpm --dir . exec`;
  - the `eslint` hook passes `--max-warnings 10` while CI lints through the
    `lint` script, which runs plain `eslint .`;
  - its `ruff-format` and `prettier` jobs run the formatters without
    `--check`, so they rewrite files rather than fail, as do its `prettier`
    hook and `format` script, while its `ruff-format` hook checks with
    `--diff`;
  - its `ruff-check` hook is remote, its `mypy` hook has `language: system`
    and its `tsc` hook doesn't set `pass_filenames: false`;
  - its `pre-commit` job's `SKIP` lists some of the hooks with jobs of their
    own, but not `pytest`, `eslint`, `mypy` or `ruff-check`.
- `typescript-config` breaks the TypeScript configuration standards:
  - its `package.json` sets `private` to the string `"true"`;
  - its two referenced tsconfigs lack, turn off or set to another value some
    of the required compiler options, one spelling `moduleDetection` in
    another case, which TypeScript accepts;
  - the tooling's config has the application's module settings rather than
    `nodenext`, and the application's `lib` and the tooling's `target` name
    versions older than ES2022;
  - its `pnpm-workspace.yaml` lists `permissions` as a sequence, keeps the
    older `allowBuilds` and `onlyBuiltDependencies`, lets unreviewed
    dependencies' build scripts through, and turns `trustPolicy` off;
  - its `eslint.config.ts` includes typescript-eslint's
    `recommendedTypeChecked` and an `Only` variant of `stylisticTypeChecked`
    rather than the standard's configs, comments out `strictTypeChecked`, sets
    `project` rather than `projectService`, and takes Prettier's config from
    `eslint-plugin-prettier`.
- `layout` breaks the layout standards: a TypeScript component has npm's and
  yarn's lock files beside `pnpm-lock.yaml`, an `eslint.config.js` beside
  `eslint.config.ts`, a `prettier` key in `package.json` beside `.prettierrc`,
  Prettier configuration in two subdirectories (a `prettier` key in a nested
  `package.json` and a `.prettierrc.json`) and in a third that its
  `.prettierignore` skips, and a Python component's `src/` holds a module and a
  namespace package, but no package with an `__init__.py`, while a `ruff.toml`,
  `mypy.ini`, `pytest.ini` and `uv.toml` beside its `pyproject.toml` take the
  place of its tables.

Symlinks and `.gitignore` handling have no fixture, and are covered by the unit
tests in `src/detection.rs` instead: a fixture's `.gitignore` would also apply
to this repository, so the files it ignores couldn't be committed.
