// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `prek.toml`.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use crate::config::{FormatterRun, program_name};
use crate::report::Issue;

/// The hook types prek can install, from its `HookType`.
const HOOK_TYPES: [&str; 10] = [
    "commit-msg",
    "post-checkout",
    "post-commit",
    "post-merge",
    "post-rewrite",
    "pre-commit",
    "pre-merge-commit",
    "pre-push",
    "pre-rebase",
    "prepare-commit-msg",
];

/// Top-level `prek.toml` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrekConfig {
    /// The hook types a plain `prek install` installs, kept as TOML wrote it, so that a malformed value fails only the
    /// check reading it.
    pub default_install_hook_types: Option<Value>,
    /// The stages a hook runs in when it doesn't list its own, kept as TOML wrote it, so that a malformed value fails
    /// only the check reading it.
    pub default_stages: Option<Value>,
    pub repos: Vec<PrekRepo>,
    /// The file's text, as [`parse`](Self::parse) read it.
    #[serde(skip)]
    pub source: String,
}

/// The repositories prek provides itself, which have no `rev`.
const OWN_REPOS: [&str; 3] = ["local", "builtin", "meta"];

/// A remote repository and its `rev` as the shared Renovate preset's stopgap manager for `prek.toml` finds them: a
/// GitHub URL with the `rev` on the next line. Its regular expression, which Renovate runs on the whole file, matches
/// the depName and leaves the URL's `.git` out of it; this one takes the whole URL.
static RENOVATE_REPO_REV: LazyLock<Regex> = LazyLock::new(|| {
    #[expect(clippy::unwrap_used, reason = "the pattern is a valid constant")]
    Regex::new(r#"repo = "(https://github\.com/[^"]+)"\n\s*rev = "[^"]+""#).unwrap()
});

/// A single repo entry in the prek config.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrekRepo {
    /// The repository URL, or `"local"` for local hooks and `"builtin"` for prek's own.
    pub repo: String,
    /// Hooks defined in this repo.
    pub hooks: Vec<PrekHook>,
}

/// A single hook within a prek repo.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrekHook {
    pub id: String,
    /// Another name that `PREK_SKIP` and `prek run` match; empty means none.
    pub alias: Option<String>,
    /// The hook's command, if it sets one, as a local hook must. A remote hook may leave it to the hook repository.
    pub entry: Option<String>,
    /// The stages the hook lists, if it lists any.
    pub stages: Option<Vec<String>>,
    pub args: Vec<String>,
    /// The language prek runs the hook with, if the configuration sets it, as a local hook must.
    pub language: Option<String>,
    /// The priority group the hook runs in, a name from `[priorities]` or a number, kept as TOML wrote it.
    pub priority: Option<Value>,
    /// Whether prek appends the staged files' names to the hook's command, if the configuration says: `false` for
    /// none, `true` for all of them, or a number for at most that many per run. Kept as TOML wrote it.
    pub pass_filenames: Option<Value>,
}

impl PrekHook {
    /// The hook's alias, if it has one other than its ID.
    pub(crate) fn alias(&self) -> Option<&str> {
        self.alias
            .as_deref()
            .filter(|alias| !alias.is_empty() && *alias != self.id)
    }

    /// The command the hook runs, as its words, or none if the entry has unbalanced quotes. prek splits the hook's
    /// entry, or `default_entry` if it sets none, as a remote hook may leave it to the hook's repository, with the
    /// `shlex` crate, reading a word starting with `#` as a comment ending the entry, and appends the args.
    pub(crate) fn command(&self, default_entry: &str) -> Option<Vec<String>> {
        let mut words = shlex::split(self.entry.as_deref().unwrap_or(default_entry))?;
        words.extend(self.args.iter().cloned());
        Some(words)
    }

    /// The result of `f` for the command the hook runs, as [`command`](Self::command) gives it, if it gives one.
    pub(crate) fn map_command<T>(
        &self,
        default_entry: &str,
        f: impl FnOnce(&[&str]) -> Option<T>,
    ) -> Option<T> {
        let command = self.command(default_entry)?;
        f(&command.iter().map(String::as_str).collect::<Vec<_>>())
    }
}

/// The program a hook's entry runs its tool through, if it runs it through one the standard rules out: Devbox (`devbox
/// run`), or a shell, as in `bash -c "cd web && pnpm exec eslint ."`, where `pnpm --dir web` or `uv --directory api`
/// would run the tool itself. prek runs a `system` hook's entry without a shell, so a `cd` needs one.
///
/// A leading `env`, with any `NAME=value` words after it, is skipped, as in `/usr/bin/env bash -c "..."`.
fn entry_wrapper(hook: &PrekHook) -> Option<String> {
    let command = hook.command("")?;
    let mut words = command.iter().map(String::as_str);
    let mut program = program_name(words.next()?);
    if program == "env" {
        program = program_name(words.find(|word| !word.contains('='))?);
    }
    matches!(program, "devbox" | "bash" | "sh" | "dash" | "zsh").then(|| program.to_owned())
}

impl PrekConfig {
    /// Parse `prek.toml`'s text, `source`, keeping it.
    pub(crate) fn parse(source: String) -> Result<Self, String> {
        let config: Self = toml::from_str(&source).map_err(|e| format!("Invalid TOML: {e}"))?;
        Ok(Self { source, ..config })
    }

    /// Whether Renovate can update the `rev` of every remote repository: the shared preset's stopgap manager finds a
    /// `rev` only on the line after a GitHub `repo`, as [`RENOVATE_REPO_REV`] does. Without it, the hooks stay at their
    /// revision until someone updates them by hand.
    pub(crate) fn check_renovate_revs(&self) -> Result<(), String> {
        // Each match finds one entry's rev, so a URL that two entries share needs two.
        let mut found: HashMap<&str, usize> = HashMap::new();
        for url in RENOVATE_REPO_REV
            .captures_iter(&self.source)
            .filter_map(|captures| captures.get(1))
        {
            *found.entry(url.as_str()).or_default() += 1;
        }
        let mut missed: Vec<&str> = Vec::new();
        for repo in self
            .repos
            .iter()
            .map(|repo| repo.repo.as_str())
            .filter(|repo| !OWN_REPOS.contains(repo))
        {
            match found.get_mut(repo) {
                Some(count) if *count > 0 => *count -= 1,
                _ if missed.contains(&repo) => {}
                _ => missed.push(repo),
            }
        }
        if missed.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Renovate can't update the rev of {}: write each as repo = \"https://github.com/...\" with \
                 rev = \"...\" on the next line",
                missed.join(", ")
            ))
        }
    }

    /// Whether `default_stages` is exactly `[pre-commit]`, so that hooks without stages of their own run once per
    /// commit, and not again on its message: a failure if prek would reject it, as anything but a list of the
    /// stages it knows, and a warning if it is missing or lists other stages.
    pub(crate) fn check_default_stages(&self) -> Result<(), Issue> {
        match &self.default_stages {
            None => Err(Issue::warn(
                "No default_stages, so hooks without stages of their own run in every installed stage",
            )),
            Some(Value::Array(stages)) => {
                let unknown: Vec<_> = stages
                    .iter()
                    .filter(|stage| !stage.as_str().is_some_and(is_stage))
                    .map(value_name)
                    .collect();
                if !unknown.is_empty() {
                    Err(Issue::fail(format!(
                        "default_stages lists {}, which prek doesn't know",
                        unknown.join(", ")
                    )))
                } else if matches!(stages.as_slice(), [stage] if stage == "pre-commit") {
                    Ok(())
                } else {
                    Err(Issue::warn(format!(
                        "default_stages is [{}], not [pre-commit]",
                        stages.iter().map(value_name).collect::<Vec<_>>().join(", ")
                    )))
                }
            }
            Some(_) => Err(Issue::fail("default_stages isn't a list")),
        }
    }

    /// Whether `default_install_hook_types` lists both `pre-commit` and `commit-msg`, so that a plain `prek install`
    /// also installs the commit message hook. Any other hook types prek knows may be listed too.
    pub(crate) fn check_default_install_hook_types(&self) -> Result<(), String> {
        const REQUIRED: [&str; 2] = ["pre-commit", "commit-msg"];
        match &self.default_install_hook_types {
            None => Err(
                "No default_install_hook_types, so prek install installs only the pre-commit hook"
                    .to_owned(),
            ),
            Some(Value::Array(types)) => {
                let missing: Vec<_> = REQUIRED
                    .into_iter()
                    .filter(|required| !types.iter().any(|hook_type| hook_type == required))
                    .collect();
                let unknown: Vec<_> = types
                    .iter()
                    .filter(|hook_type| {
                        !hook_type
                            .as_str()
                            .is_some_and(|name| HOOK_TYPES.contains(&name))
                    })
                    .map(value_name)
                    .collect();
                if !unknown.is_empty() {
                    Err(format!(
                        "default_install_hook_types lists {}, which prek doesn't know",
                        unknown.join(", ")
                    ))
                } else if missing.is_empty() {
                    Ok(())
                } else {
                    Err(format!(
                        "default_install_hook_types lacks {}",
                        missing.join(" and ")
                    ))
                }
            }
            Some(_) => Err("default_install_hook_types isn't a list".to_owned()),
        }
    }

    /// Whether `PREK_SKIP` (or `prek run`) can select each hook alone: each hook needs an ID or alias that no other
    /// hook has as its ID or alias. prek matches a hook by either, so the same remote hook can run twice under
    /// one ID if each copy has an alias of its own.
    pub(crate) fn check_unique_hook_ids(&self) -> Result<(), String> {
        fn names(hook: &PrekHook) -> Vec<&str> {
            let mut names = vec![hook.id.as_str()];
            names.extend(hook.alias());
            names
        }

        let hooks: Vec<_> = self.hooks().collect();
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for hook in &hooks {
            for name in names(hook) {
                *counts.entry(name).or_default() += 1;
            }
        }
        let mut ambiguous: Vec<&str> = Vec::new();
        for hook in &hooks {
            let label = if hook.id.is_empty() {
                "(no id)"
            } else {
                &hook.id
            };
            if names(hook)
                .iter()
                .all(|name| counts.get(name).is_some_and(|&count| count > 1))
                && !ambiguous.contains(&label)
            {
                ambiguous.push(label);
            }
        }
        if ambiguous.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "Hooks with no ID or alias of their own: {}",
                ambiguous.join(", ")
            ))
        }
    }

    /// Whether every hook with one of the IDs `ids`, the language hooks of a language, is set up as the standard's
    /// Language Hooks section says: a `local` hook with `language = "system"` and `pass_filenames = false`, so that it
    /// runs the project's own tools on the whole project, and whose entry runs its tool directly rather than through
    /// Devbox or a shell (see [`entry_wrapper`]). A remote hook's language and file names are its repository's
    /// to choose, so only its being remote is reported. Hooks that are missing are left to the checks of each hook.
    pub(crate) fn check_language_hooks(&self, ids: &[&str]) -> Result<(), String> {
        let mut issues: Vec<String> = Vec::new();
        for (repo, hook) in self
            .repo_hooks()
            .filter(|(_, hook)| ids.contains(&hook.id.as_str()))
        {
            let name = hook.alias().unwrap_or(&hook.id);
            if repo.repo != "local" {
                issues.push(format!("{name} isn't a local hook"));
                continue;
            }
            match hook.language.as_deref() {
                Some("system") => {}
                Some(language) => {
                    issues.push(format!("{name} has language {language}, not system"));
                }
                None => issues.push(format!("{name} has no language")),
            }
            if hook.pass_filenames != Some(Value::Bool(false)) {
                issues.push(format!("{name} doesn't set pass_filenames = false"));
            }
            if let Some(wrapper) = entry_wrapper(hook) {
                issues.push(format!(
                    "{name} runs through {wrapper}, not its tool directly"
                ));
            }
        }
        // Copies of a hook without aliases of their own have the same issues, each worth naming once.
        let mut seen = HashSet::new();
        issues.retain(|issue| seen.insert(issue.clone()));
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    }

    /// The stages `hook` runs in, by the names the configuration gives them: those it lists, or, if it lists none or an
    /// empty list, `default_stages`. Without either, prek runs it in every stage its repository gives it, which
    /// is none. A malformed `default_stages` is left to [`check_default_stages`](Self::check_default_stages).
    fn stages_of<'a>(&'a self, hook: &'a PrekHook) -> Option<Vec<&'a str>> {
        match (&hook.stages, &self.default_stages) {
            (Some(stages), _) if !stages.is_empty() => {
                Some(stages.iter().map(String::as_str).collect())
            }
            (_, Some(Value::Array(stages))) => {
                Some(stages.iter().filter_map(Value::as_str).collect())
            }
            (_, _) => None,
        }
    }

    /// Whether `hook` runs in `stage`, as [`stages_of`](Self::stages_of) reads its stages. One that runs in every stage
    /// its repository gives it is taken to run in `stage`, which for the hooks checked is among them. Stages may be
    /// given by their old names, such as `commit`.
    fn runs_in(&self, hook: &PrekHook, stage: &str) -> bool {
        let wanted = current_stage_name(stage);
        self.stages_of(hook).is_none_or(|stages| {
            stages
                .into_iter()
                .any(|listed| current_stage_name(listed) == wanted)
        })
    }

    /// Whether `hook` runs only in the `manual` stage, as [`stages_of`](Self::stages_of) reads its stages, so that it
    /// runs only when `prek run --stage manual` asks for it.
    fn runs_only_manually(&self, hook: &PrekHook) -> bool {
        self.stages_of(hook).is_some_and(|stages| {
            !stages.is_empty() && stages.into_iter().all(|stage| stage == "manual")
        })
    }

    /// Whether `hook` runs before each commit, in the `pre-commit` stage. See [`runs_in`](Self::runs_in).
    pub(crate) fn runs_at_commit(&self, hook: &PrekHook) -> bool {
        self.runs_in(hook, "pre-commit")
    }

    /// Every hook, with the repository that holds it, in the configuration's order.
    pub(crate) fn repo_hooks(&self) -> impl Iterator<Item = (&PrekRepo, &PrekHook)> {
        self.repos
            .iter()
            .flat_map(|repo| repo.hooks.iter().map(move |hook| (repo, hook)))
    }

    /// Every hook, in the configuration's order.
    pub(crate) fn hooks(&self) -> impl Iterator<Item = &PrekHook> {
        self.repos.iter().flat_map(|repo| &repo.hooks)
    }

    /// Every hook that runs in `stage`, as [`runs_in`](Self::runs_in) decides.
    pub(crate) fn hooks_in<'a>(&'a self, stage: &'a str) -> impl Iterator<Item = &'a PrekHook> {
        self.hooks().filter(move |hook| self.runs_in(hook, stage))
    }

    /// The hooks with ID `id` that run before each commit, or, if there are none, why not: a failure if there is no
    /// such hook or it runs only in the `manual` stage, so never on its own, and a warning if it runs only in other
    /// stages.
    pub(crate) fn commit_hooks(&self, id: &str) -> Result<Vec<&PrekHook>, Issue> {
        let hooks: Vec<_> = self.hooks().filter(|hook| hook.id == id).collect();
        if hooks.is_empty() {
            return Err(Issue::fail(format!("No {id} hook")));
        }
        if hooks.iter().all(|hook| self.runs_only_manually(hook)) {
            return Err(Issue::fail(format!(
                "The {id} hook runs only in the manual stage, so it never runs on its own"
            )));
        }

        let hooks: Vec<_> = hooks
            .into_iter()
            .filter(|hook| self.runs_at_commit(hook))
            .collect();
        if hooks.is_empty() {
            Err(Issue::warn(format!(
                "The {id} hook doesn't run in the pre-commit stage"
            )))
        } else {
            Ok(hooks)
        }
    }

    /// Check that each hook with ID `id` that runs before each commit, as [`commit_hooks`](Self::commit_hooks) finds
    /// them, runs a command whose words `checks` accepts, warning with `issue` if one doesn't: a hook that should fail
    /// on a stale lock file, say, but updates it instead. A hook without an entry, as a remote hook may leave it to the
    /// hook's repository, is taken to run `default_entry`, with its args.
    pub(crate) fn check_hook_command(
        &self,
        id: &str,
        default_entry: &str,
        mut checks: impl FnMut(&[&str]) -> bool,
        issue: &str,
    ) -> Result<(), Issue> {
        let hooks = self.commit_hooks(id)?;
        if hooks.iter().all(|hook| {
            hook.map_command(default_entry, |words| checks(words).then_some(()))
                .is_some()
        }) {
            Ok(())
        } else {
            Err(Issue::warn(issue))
        }
    }

    /// Whether a hook with ID `id` runs before each commit. See [`commit_hooks`](Self::commit_hooks).
    pub(crate) fn check_commit_hook(&self, id: &str) -> Result<(), Issue> {
        self.commit_hooks(id).map(|_| ())
    }

    /// Check that a hook with ID `id` runs before each commit, and that each such hook runs the formatter `name` so
    /// that it fails on unformatted files rather than rewriting them, as `runs` tells from a command's words. A hook
    /// without an entry, as a remote hook may leave it to the hook's repository, is taken to run `name`. prek fails
    /// a hook that changes files, so a formatter that rewrites them still stops the commit, but leaves them
    /// changed: a warning, as is a hook that doesn't run the formatter where norms can see it. One that only prints
    /// the formatted files passes every commit, so it fails.
    pub(crate) fn check_formatter_hook(
        &self,
        id: &str,
        name: &str,
        mut runs: impl FnMut(&[&str]) -> Option<FormatterRun>,
    ) -> Result<(), Issue> {
        let runs: Vec<_> = self
            .commit_hooks(id)?
            .into_iter()
            .filter_map(|hook| hook.map_command(name, &mut runs))
            .collect();
        if runs.is_empty() {
            Err(Issue::warn(format!("The {id} hook doesn't run {name}")))
        } else {
            FormatterRun::check_each(
                &runs,
                name,
                Issue::warn(format!(
                    "{name} runs without --check, so it rewrites files rather than only failing on them"
                )),
            )
        }
    }
}

/// A TOML value as a message names it: a string as itself, anything else as JSON writes it.
fn value_name(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

/// Whether prek knows `stage` as a stage, by its current or its old name: a hook type, or `manual`.
fn is_stage(stage: &str) -> bool {
    stage == "manual" || HOOK_TYPES.contains(&current_stage_name(stage))
}

/// The current name of a stage, given by its current or its old name, which prek still accepts.
fn current_stage_name(stage: &str) -> &str {
    match stage {
        "commit" => "pre-commit",
        "merge-commit" => "pre-merge-commit",
        "push" => "pre-push",
        stage => stage,
    }
}

/// Configurations for the tests of other modules' checks.
#[cfg(test)]
impl PrekConfig {
    /// A configuration whose hooks run before each commit unless they list stages of their own, with a repository
    /// `repo` holding `hooks`.
    pub(crate) fn with_repo(repo: &str, hooks: Vec<PrekHook>) -> Self {
        Self {
            default_stages: Some(serde_json::json!(["pre-commit"])),
            repos: vec![PrekRepo {
                repo: repo.to_owned(),
                hooks,
            }],
            ..Self::default()
        }
    }

    /// A configuration as [`with_repo`](Self::with_repo) makes it, whose repository is `local`, with a hook for each
    /// ID and entry of `hooks`.
    pub(crate) fn with_local_hooks(hooks: &[(&str, &str)]) -> Self {
        Self::with_repo(
            "local",
            hooks
                .iter()
                .map(|(id, entry)| PrekHook::new(id, Some(entry)))
                .collect(),
        )
    }
}

#[cfg(test)]
impl PrekHook {
    /// A hook with ID `id` and the entry `entry`, if any.
    pub(crate) fn new(id: &str, entry: Option<&str>) -> Self {
        Self {
            id: id.to_owned(),
            entry: entry.map(str::to_owned),
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(toml: &str) -> PrekConfig {
        toml::from_str(toml).unwrap()
    }

    /// A configuration whose only repository is `local`, with `hooks`, an inline array of tables.
    fn local(hooks: &str) -> PrekConfig {
        parse(&format!(
            "repos = [{{repo = \"local\", hooks = {hooks}}}]\n"
        ))
    }

    fn default_stages(toml: &str) -> Result<(), Issue> {
        parse(toml).check_default_stages()
    }

    fn default_install_hook_types(toml: &str) -> Result<(), String> {
        parse(toml).check_default_install_hook_types()
    }

    #[test]
    fn default_install_hook_types_must_include_commit_msg() {
        for toml in [
            "default_install_hook_types = [\"pre-commit\", \"commit-msg\"]\n",
            "default_install_hook_types = [\"commit-msg\", \"pre-commit\"]\n",
            "default_install_hook_types = [\"pre-commit\", \"commit-msg\", \"pre-push\"]\n",
        ] {
            assert_eq!(default_install_hook_types(toml), Ok(()), "{toml}");
        }
        for (toml, error) in [
            (
                "repos = []\n",
                "No default_install_hook_types, so prek install installs only the pre-commit hook",
            ),
            (
                "default_install_hook_types = [\"pre-commit\"]\n",
                "default_install_hook_types lacks commit-msg",
            ),
            (
                "default_install_hook_types = [\"commit-msg\", \"pre-push\"]\n",
                "default_install_hook_types lacks pre-commit",
            ),
            (
                "default_install_hook_types = []\n",
                "default_install_hook_types lacks pre-commit and commit-msg",
            ),
            (
                "default_install_hook_types = \"pre-commit\"\n",
                "default_install_hook_types isn't a list",
            ),
            (
                "default_install_hook_types = [\"pre-commit\", \"commit-msg\", \"pre_push\", 3]\n",
                "default_install_hook_types lists pre_push, 3, which prek doesn't know",
            ),
        ] {
            assert_eq!(
                default_install_hook_types(toml),
                Err(error.to_owned()),
                "{toml}"
            );
        }
    }

    fn unique_hook_ids(hooks: &str) -> Result<(), String> {
        local(hooks).check_unique_hook_ids()
    }

    #[test]
    fn hooks_need_an_id_or_alias_of_their_own() {
        for hooks in [
            "[]",
            r#"[{id = "pytest"}, {id = "cargo-test"}, {id = "vitest"}]"#,
            r#"[{id = "eslint", alias = "eslint-admin"}, {id = "eslint", alias = "eslint-web"}]"#,
            r#"[{id = "test", alias = "test"}, {id = "vitest"}]"#,
            r#"[{id = "test", alias = ""}, {id = "vitest"}]"#,
        ] {
            assert_eq!(unique_hook_ids(hooks), Ok(()), "{hooks}");
        }
        for (hooks, ambiguous) in [
            (r#"[{id = "test"}, {id = "test"}]"#, "test"),
            (r#"[{id = "test"}, {id = "test"}, {id = "test"}]"#, "test"),
            (r#"[{id = "test"}, {id = "test", alias = "test"}]"#, "test"),
            // PREK_SKIP=eslint-admin selects the first hook alone, but nothing selects the second.
            (
                r#"[{id = "eslint", alias = "eslint-admin"}, {id = "eslint"}]"#,
                "eslint",
            ),
            (
                r#"[{id = "a"}, {id = "b"}, {id = "a"}, {id = "b"}]"#,
                "a, b",
            ),
            (r#"[{alias = "a"}, {alias = "a"}]"#, "(no id)"),
            (
                r#"[{id = "eslint", alias = "lint"}, {id = "eslint", alias = "lint"}]"#,
                "eslint",
            ),
            // The first hook's only name is the second's alias; the second has an ID of its own.
            (
                r#"[{id = "vitest"}, {id = "test", alias = "vitest"}]"#,
                "vitest",
            ),
        ] {
            assert_eq!(
                unique_hook_ids(hooks),
                Err(format!(
                    "Hooks with no ID or alias of their own: {ambiguous}"
                )),
                "{hooks}"
            );
        }
        // An empty alias is kept, but means none.
        let hook = PrekHook {
            alias: Some(String::new()),
            ..PrekHook::default()
        };
        assert_eq!(hook.alias(), None);
    }

    fn hook_command(entry: Option<&str>, args: &[&str]) -> Vec<Vec<String>> {
        PrekHook {
            entry: entry.map(str::to_owned),
            args: args.iter().map(|arg| (*arg).to_owned()).collect(),
            ..PrekHook::default()
        }
        .command("default")
        .into_iter()
        .collect()
    }

    #[test]
    fn entries_split_as_prek_splits_them() {
        for (entry, args, expected) in [
            (None, &["--fix"][..], vec![vec!["default", "--fix"]]),
            (
                Some("pnpm exec eslint . --max-warnings 0"),
                &[],
                vec![vec!["pnpm", "exec", "eslint", ".", "--max-warnings", "0"]],
            ),
            (
                Some("python -c 'import sys; sys.exit(0)'"),
                &["a"],
                vec![vec!["python", "-c", "import sys; sys.exit(0)", "a"]],
            ),
            (
                Some("bash -c 'cd web && eslint .'"),
                &["--max-warnings", "0"],
                vec![vec![
                    "bash",
                    "-c",
                    "cd web && eslint .",
                    "--max-warnings",
                    "0",
                ]],
            ),
            // A word starting with # ends the entry, but the args still follow.
            (
                Some("eslint . #--fix"),
                &["--max-warnings", "0"],
                vec![vec!["eslint", ".", "--max-warnings", "0"]],
            ),
            (Some("eslint 'unbalanced"), &[], vec![]),
        ] {
            assert_eq!(hook_command(entry, args), expected, "{entry:?}");
        }
    }

    fn language_hooks(repos: &str) -> Result<(), String> {
        parse(&format!("repos = {repos}\n")).check_language_hooks(&["lint", "test"])
    }

    #[test]
    fn language_hooks_are_local_system_take_no_file_names_and_run_their_tools_directly() {
        for repos in [
            "[]",
            r#"[{repo = "local", hooks = [{id = "lint", language = "system", pass_filenames = false}]}]"#,
            // Hooks with other IDs aren't language hooks.
            r#"[{repo = "local", hooks = [{id = "other", language = "python"}]}, {repo = "https://example.com/a", hooks = [{id = "b"}]}]"#,
            r#"[{repo = "local", hooks = [{id = "lint", alias = "lint-a", language = "system", pass_filenames = false}, {id = "lint", alias = "lint-b", language = "system", pass_filenames = false}]}]"#,
            r#"[{repo = "local", hooks = [{id = "lint", entry = "pnpm --dir web exec eslint . --max-warnings 0", language = "system", pass_filenames = false}]}]"#,
            // Only the program the entry starts with runs the tool: an argument naming a shell is no wrapper.
            r#"[{repo = "local", hooks = [{id = "lint", entry = "cargo run -- sh", language = "system", pass_filenames = false}]}]"#,
            // An entry prek can't split runs nothing, which the checks of each hook report.
            r#"[{repo = "local", hooks = [{id = "lint", entry = "bash -c 'cd web", language = "system", pass_filenames = false}]}]"#,
            r#"[{repo = "local", hooks = [{id = "lint", entry = "env CI=1 cargo test", language = "system", pass_filenames = false}]}]"#,
        ] {
            assert_eq!(language_hooks(repos), Ok(()), "{repos}");
        }
        for (repos, issues) in [
            (
                r#"[{repo = "https://example.com/lint", hooks = [{id = "lint", language = "system", pass_filenames = false}]}]"#,
                "lint isn't a local hook",
            ),
            (
                r#"[{repo = "builtin", hooks = [{id = "lint"}]}]"#,
                "lint isn't a local hook",
            ),
            // prek takes unsupported as another name for system, but the standard names system.
            (
                r#"[{repo = "local", hooks = [{id = "lint", language = "unsupported", pass_filenames = false}]}]"#,
                "lint has language unsupported, not system",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", pass_filenames = false}]}]"#,
                "lint has no language",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", language = "system"}]}]"#,
                "lint doesn't set pass_filenames = false",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", language = "system", pass_filenames = true}]}]"#,
                "lint doesn't set pass_filenames = false",
            ),
            // A number limits how many file names each run takes, but still passes them.
            (
                r#"[{repo = "local", hooks = [{id = "lint", language = "system", pass_filenames = 1}]}]"#,
                "lint doesn't set pass_filenames = false",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", entry = "devbox run -- cargo clippy", language = "system", pass_filenames = false}]}]"#,
                "lint runs through devbox, not its tool directly",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", entry = "bash -c 'cd web && pnpm exec eslint .'", language = "system", pass_filenames = false}]}]"#,
                "lint runs through bash, not its tool directly",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", entry = "/bin/sh -c 'cd api && uv run ruff check'", language = "system", pass_filenames = false}]}]"#,
                "lint runs through sh, not its tool directly",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", entry = "/usr/bin/env FORCE_COLOR=1 zsh -c 'cd web && pnpm exec eslint .'", language = "system", pass_filenames = false}]}]"#,
                "lint runs through zsh, not its tool directly",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", entry = "dash -c 'cd api && uv run ruff check'", language = "system", pass_filenames = false}]}]"#,
                "lint runs through dash, not its tool directly",
            ),
            (
                r#"[{repo = "local", hooks = [{id = "lint", language = "python"}, {id = "test", alias = "test-a", language = "system", pass_filenames = false}, {id = "test", alias = "test-b", language = "system"}]}, {repo = "https://example.com/a", hooks = [{id = "test"}, {id = "test"}]}]"#,
                "lint has language python, not system; lint doesn't set pass_filenames = false; \
                 test-b doesn't set pass_filenames = false; test isn't a local hook",
            ),
        ] {
            assert_eq!(language_hooks(repos), Err(issues.to_owned()), "{repos}");
        }
    }

    #[test]
    fn formatter_hooks_check_rather_than_rewrite() {
        let runs = |words: &[&str]| {
            let (_, args) = words.split_first()?;
            Some(FormatterRun::from_args(args, &["--check"], &["-c"]))
        };
        let hook = |hooks: &str| local(hooks).check_formatter_hook("fmt", "fmt", runs);
        assert_eq!(hook(r#"[{id = "fmt", entry = "fmt --check ."}]"#), Ok(()));
        assert_eq!(hook(r#"[{id = "fmt", args = ["--check"]}]"#), Ok(()));
        assert_eq!(
            hook(
                r#"[{id = "fmt", entry = "fmt --check ."}, {id = "fmt", entry = "fmt --check", alias = "b"}]"#
            ),
            Ok(())
        );
        assert_eq!(
            hook(
                r#"[{id = "fmt", entry = "fmt --check ."}, {id = "fmt", entry = "fmt --write", alias = "b"}]"#
            ),
            Err(Issue::warn(
                "fmt runs without --check, so it rewrites files rather than only failing on them"
            ))
        );
        assert_eq!(
            hook(r#"[{id = "fmt", entry = "fmt -c ."}]"#),
            Err(Issue::warn("fmt runs with -c rather than --check"))
        );
        assert_eq!(
            hook(r#"[{id = "fmt", entry = "fmt --write ."}]"#),
            Err(Issue::warn(
                "fmt runs without --check, so it rewrites files rather than only failing on them"
            ))
        );
        assert_eq!(
            hook(r#"[{id = "fmt", entry = "fmt 'unbalanced"}]"#),
            Err(Issue::warn("The fmt hook doesn't run fmt"))
        );
        assert_eq!(hook("[]"), Err(Issue::fail("No fmt hook")));
    }

    fn commit_hook(toml: &str) -> Result<(), Issue> {
        parse(toml).check_commit_hook("markdownlint-cli2")
    }

    #[test]
    fn hook_commands_are_checked_in_every_copy() {
        let check = |hooks: &str| {
            local(hooks).check_hook_command(
                "uv-lock",
                "uv lock",
                |words| words.contains(&"--check"),
                "stale",
            )
        };
        assert_eq!(
            check(r#"[{id = "uv-lock", entry = "uv lock --check"}]"#),
            Ok(())
        );
        assert_eq!(
            check(r#"[{id = "uv-lock", entry = "uv lock"}]"#),
            Err(Issue::warn("stale"))
        );
        assert_eq!(
            check(
                r#"[{id = "uv-lock", entry = "uv lock --check"}, {id = "uv-lock", alias = "uv-lock-api", entry = "uv lock"}]"#
            ),
            Err(Issue::warn("stale"))
        );
        assert!(check("[]").is_err());
    }

    #[test]
    fn commit_hooks_are_found_by_id() {
        assert_eq!(
            local(r#"[{id = "markdownlint-cli2", entry = "markdownlint"}]"#)
                .check_commit_hook("markdownlint-cli2"),
            Ok(())
        );
        assert_eq!(
            commit_hook("repos = []\n"),
            Err(Issue::fail("No markdownlint-cli2 hook"))
        );
    }

    #[test]
    fn commit_hooks_run_in_the_pre_commit_stage() {
        let hook = |stages: &str, default_stages: &str| {
            commit_hook(&format!(
                "{default_stages}repos = [{{repo = \"local\", hooks = [{{id = \"markdownlint-cli2\"{stages}}}]}}]\n"
            ))
        };
        for (stages, default_stages) in [
            ("", ""),
            ("", "default_stages = [\"pre-commit\"]\n"),
            ("", "default_stages = \"pre-commit\"\n"),
            (
                r#", stages = ["pre-commit", "pre-push"]"#,
                "default_stages = [\"pre-push\"]\n",
            ),
            // An empty list is no list, and commit is the stage's old name.
            (", stages = []", "default_stages = [\"pre-commit\"]\n"),
            (r#", stages = ["commit"]"#, ""),
            ("", "default_stages = [\"commit\"]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Ok(()),
                "{stages} {default_stages}"
            );
        }
        for (stages, default_stages) in [
            (r#", stages = ["pre-push"]"#, ""),
            (", stages = []", "default_stages = [\"pre-push\"]\n"),
            ("", "default_stages = [\"pre-push\", \"manual\"]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Err(Issue::warn(
                    "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
                )),
                "{stages} {default_stages}"
            );
        }
        for (stages, default_stages) in [
            (r#", stages = ["manual"]"#, ""),
            (
                r#", stages = ["manual"]"#,
                "default_stages = [\"pre-commit\"]\n",
            ),
            ("", "default_stages = [\"manual\"]\n"),
            (", stages = []", "default_stages = [\"manual\"]\n"),
        ] {
            assert_eq!(
                hook(stages, default_stages),
                Err(Issue::fail(
                    "The markdownlint-cli2 hook runs only in the manual stage, so it never runs on its own"
                )),
                "{stages} {default_stages}"
            );
        }
    }

    #[test]
    fn renovate_finds_each_rev_on_the_line_after_its_repo() {
        let revs = |toml: &str| {
            PrekConfig::parse(toml.to_owned())
                .unwrap()
                .check_renovate_revs()
        };
        let missed = |repos: &str| {
            Err(format!(
                "Renovate can't update the rev of {repos}: write each as repo = \"https://github.com/...\" with rev = \
                 \"...\" on the next line"
            ))
        };
        // Renovate's \s* lets blank lines through, too.
        assert_eq!(
            revs(
                r#"
[[repos]]
repo = "builtin"

[[repos.hooks]]
id = "check-yaml"

[[repos]]
repo = "https://github.com/fsfe/reuse-tool"
rev = "v6.3.0"

[[repos]]
repo = "https://github.com/zizmorcore/zizmor-pre-commit.git"

rev = "v1.32.0"

[[repos]]
repo = "local"
"#
            ),
            Ok(())
        );
        // The rev before the repo, a comment between them, a comment after the repo, or a literal string all hide it.
        assert_eq!(
            revs(
                r#"
[[repos]]
rev = "v1.11.2"
repo = "https://github.com/tombi-toml/tombi-pre-commit"

[[repos]]
repo = "https://github.com/DavidAnson/markdownlint-cli2"
# Markdown
rev = "v0.24.1"

[[repos]]
repo = "https://github.com/zizmorcore/zizmor-pre-commit" # zizmor
rev = "v1.32.0"

[[repos]]
repo = 'https://github.com/compilerla/conventional-pre-commit'
rev = "v4.5.0"

[[repos]]
repo = "https://github.com/fsfe/reuse-tool"
rev = "v6.3.0"
"#
            ),
            missed(
                "https://github.com/tombi-toml/tombi-pre-commit, https://github.com/DavidAnson/markdownlint-cli2, \
                 https://github.com/zizmorcore/zizmor-pre-commit, https://github.com/compilerla/conventional-pre-commit"
            )
        );
        // So do inline tables and another host.
        assert_eq!(
            revs(
                r#"repos = [{repo = "https://github.com/fsfe/reuse-tool", rev = "v6.3.0", hooks = []}, {repo = "https://gitlab.com/a/b", rev = "v1"}]"#
            ),
            missed("https://github.com/fsfe/reuse-tool, https://gitlab.com/a/b")
        );
        // A repo listed twice needs each rev where Renovate finds it, and is named once however many it misses.
        let twice = |second: &str| {
            revs(&format!(
                "[[repos]]\nrepo = \"https://github.com/a/b\"\nrev = \"v1\"\n\n[[repos]]\n{second}\n\n[[repos]]\n\
                 repo = \"https://github.com/c/d\"\nrev = \"v1\"\n\n[[repos]]\nrev = \"v3\"\nrepo = \"https://github.com/a/b\"\n"
            ))
        };
        assert_eq!(
            twice("repo = \"https://github.com/a/b\"\nrev = \"v2\""),
            missed("https://github.com/a/b")
        );
        assert_eq!(
            twice("rev = \"v2\"\nrepo = \"https://github.com/a/b\""),
            missed("https://github.com/a/b")
        );
    }

    #[test]
    fn hooks_are_read_from_arrays_of_tables() {
        let config = parse(
            r#"
[[repos]]
repo = "local"

[[repos.hooks]]
id = "test"
alias = "unit"
entry = "cargo test"
stages = ["pre-commit"]

[[repos.hooks]]
id = "other"
"#,
        );
        let hooks: Vec<_> = config
            .hooks()
            .map(|hook| {
                (
                    hook.id.as_str(),
                    hook.alias(),
                    hook.entry.as_deref(),
                    hook.stages.clone(),
                )
            })
            .collect();
        assert_eq!(
            hooks,
            [
                (
                    "test",
                    Some("unit"),
                    Some("cargo test"),
                    Some(vec!["pre-commit".to_owned()])
                ),
                ("other", None, None, None),
            ]
        );
    }

    #[test]
    fn a_hook_in_manual_and_another_stage_still_runs() {
        assert_eq!(
            local(r#"[{id = "markdownlint-cli2", stages = ["manual", "pre-push"]}]"#)
                .check_commit_hook("markdownlint-cli2"),
            Err(Issue::warn(
                "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
            ))
        );
        assert_eq!(
            local(
                r#"[{id = "markdownlint-cli2", stages = ["manual"]}, {id = "markdownlint-cli2", stages = ["pre-commit"]}]"#
            )
            .check_commit_hook("markdownlint-cli2"),
            Ok(())
        );
    }

    #[test]
    fn a_manual_hook_beside_one_in_another_stage_still_runs() {
        assert_eq!(
            local(
                r#"[{id = "markdownlint-cli2", stages = ["manual"]}, {id = "markdownlint-cli2", stages = ["pre-push"]}]"#
            )
            .check_commit_hook("markdownlint-cli2"),
            Err(Issue::warn(
                "The markdownlint-cli2 hook doesn't run in the pre-commit stage"
            ))
        );
    }

    #[test]
    fn default_stages_must_be_pre_commit_alone() {
        for toml in [
            "default_stages = [\"pre-commit\"]\nrepos = []\n",
            "default_stages = ['pre-commit']\n",
        ] {
            assert_eq!(default_stages(toml), Ok(()), "{toml}");
        }
        for (toml, issue) in [
            (
                "repos = []\n",
                Issue::warn(
                    "No default_stages, so hooks without stages of their own run in every installed stage",
                ),
            ),
            (
                "default_stages = [\"commit\"]\n",
                Issue::warn("default_stages is [commit], not [pre-commit]"),
            ),
            // prek still knows each stage by its old name.
            (
                "default_stages = [\"merge-commit\", \"push\"]\n",
                Issue::warn("default_stages is [merge-commit, push], not [pre-commit]"),
            ),
            (
                "default_stages = [\"pre-commit\", \"pre-push\"]\n",
                Issue::warn("default_stages is [pre-commit, pre-push], not [pre-commit]"),
            ),
            (
                "default_stages = [\"manual\"]\n",
                Issue::warn("default_stages is [manual], not [pre-commit]"),
            ),
            (
                "default_stages = []\n",
                Issue::warn("default_stages is [], not [pre-commit]"),
            ),
            // prek rejects each of these, and so runs no hooks at all.
            (
                "default_stages = \"pre-commit\"\n",
                Issue::fail("default_stages isn't a list"),
            ),
            (
                "default_stages = [\"pre-commit\", \"pre_push\", 3]\n",
                Issue::fail("default_stages lists pre_push, 3, which prek doesn't know"),
            ),
            (
                "default_stages = [[\"pre-commit\"]]\n",
                Issue::fail("default_stages lists [\"pre-commit\"], which prek doesn't know"),
            ),
        ] {
            assert_eq!(default_stages(toml), Err(issue), "{toml}");
        }
    }
}
