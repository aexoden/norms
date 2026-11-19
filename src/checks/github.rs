// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Checks of a GitHub repository's settings and rulesets, which `norms github` runs, as STANDARDS.md's Branch
//! Protection and Repository Settings sections describe them.
//!
//! Their IDs start with `github-`, so that `norms verify` leaves the waivers of these checks to `norms github`, and
//! `norms github` the others to `norms verify`. A setting the standards make definite fails when it differs; one they
//! leave to the project ("unless the project uses them") warns. A setting the token can't read, or that GitHub offers
//! only to public repositories or with GitHub Pro, is skipped with the reason.

use std::path::PathBuf;

use crate::config::Waiver;
use crate::github::{FeatureStatus, Fetched, Ruleset, Settings, StatusCheckParameters};
use crate::report::{Issue, Outcome, Report};

/// The prefix of every check ID here.
const PREFIX: &str = "github-";

/// The app ID of GitHub Actions, the source the `Default` ruleset requires its status check to come from.
const GITHUB_ACTIONS_APP_ID: u64 = 15368;

/// The status check the `Default` ruleset requires: the CI gate job.
const REQUIRED_CHECK: &str = "CI success";

/// Why a setting reported only to admins is skipped.
const NEEDS_ADMIN: &str = "Reading it needs admin access to the repository";

/// Whether `id` names one of these checks, which `norms github` runs rather than `norms verify`.
pub(crate) fn is_github_check(id: &str) -> bool {
    id.starts_with(PREFIX)
}

/// What a check found: an outcome, or a reason it couldn't check.
enum Verdict {
    Checked(Outcome),
    /// Issues of their own severities, whatever the check's level.
    Issues(Result<(), Issue>),
    Skip(String),
}

impl Verdict {
    fn skip(reason: impl Into<String>) -> Self {
        Self::Skip(reason.into())
    }
}

impl<T: Into<Outcome>> From<T> for Verdict {
    fn from(outcome: T) -> Self {
        Self::Checked(outcome.into())
    }
}

/// Whether an issue fails or warns.
#[derive(Clone, Copy)]
enum Level {
    Must,
    Should,
}

fn record(report: &mut Report, level: Level, id: &str, name: &str, verdict: Verdict) {
    match (verdict, level) {
        (Verdict::Skip(reason), _) => {
            report.skip(id, name, reason);
        }
        (Verdict::Issues(outcome), _) => {
            report.check(id, name, outcome);
        }
        (Verdict::Checked(outcome), Level::Must) => {
            report.must(id, name, outcome);
        }
        (Verdict::Checked(outcome), Level::Should) => {
            report.should(id, name, outcome);
        }
    }
}

/// An issue unless `value` is `expected`, describing the setting with `what`, its name and verb ("Issues are");
/// skipped if it wasn't reported.
fn expect_bool(value: Option<bool>, expected: bool, what: &str) -> Verdict {
    match value {
        None => Verdict::skip(NEEDS_ADMIN),
        Some(value) if value == expected => Outcome::Pass.into(),
        Some(_) => {
            let state = if expected { "off" } else { "on" };
            Outcome::issue(format!("{what} {state}")).into()
        }
    }
}

/// The report of `norms github` on the settings of the repository `name`, for the project at `path`: every check, then
/// the project's waivers of them.
pub(crate) fn github_report(
    path: PathBuf,
    name: String,
    settings: &Settings,
    waivers: &[Waiver],
) -> Report {
    let mut report = Report::for_repository(path, name);
    run_github_checks(settings, &mut report);
    report.apply_waivers(waivers.iter().filter(|w| is_github_check(&w.check)));
    report
}

/// Run every check of the repository's settings.
fn run_github_checks(settings: &Settings, report: &mut Report) {
    check_features(settings, report);
    check_pull_requests(settings, report);
    check_actions(settings, report);
    check_security(settings, report);
    check_rulesets(settings, report);
}

fn check_features(settings: &Settings, report: &mut Report) {
    let repository = &settings.repository;
    record(
        report,
        Level::Must,
        "github-issues",
        "GitHub: issues are on",
        expect_bool(repository.has_issues, true, "Issues are"),
    );
    record(
        report,
        Level::Should,
        "github-wiki",
        "GitHub: the wiki is off",
        expect_bool(repository.has_wiki, false, "The wiki is"),
    );
    record(
        report,
        Level::Should,
        "github-projects",
        "GitHub: projects are off",
        expect_bool(repository.has_projects, false, "Projects are"),
    );
}

fn check_pull_requests(settings: &Settings, report: &mut Report) {
    let repository = &settings.repository;

    let merge_methods = match (
        repository.allow_squash_merge,
        repository.allow_merge_commit,
        repository.allow_rebase_merge,
    ) {
        (Some(squash), Some(merge), Some(rebase)) => {
            let mut issues = Vec::new();
            if !squash {
                issues.push("squash merging is not allowed");
            }
            if merge {
                issues.push("merge commits are allowed");
            }
            if rebase {
                issues.push("rebase merging is allowed");
            }
            Verdict::from(if issues.is_empty() {
                Ok(())
            } else {
                Err(capitalize(&issues.join("; ")))
            })
        }
        _ => Verdict::skip(NEEDS_ADMIN),
    };
    record(
        report,
        Level::Must,
        "github-merge-methods",
        "GitHub: only squash merging is allowed",
        merge_methods,
    );

    let squash_message = match (
        repository.squash_merge_commit_title.as_deref(),
        repository.squash_merge_commit_message.as_deref(),
    ) {
        (Some("COMMIT_OR_PR_TITLE"), Some("COMMIT_MESSAGES")) => Outcome::Pass.into(),
        (Some(title), Some(message)) => Outcome::issue(format!(
            "The squash commit takes {title} and {message}, not GitHub's default message (COMMIT_OR_PR_TITLE and \
             COMMIT_MESSAGES)"
        ))
        .into(),
        _ => Verdict::skip(NEEDS_ADMIN),
    };
    record(
        report,
        Level::Must,
        "github-squash-message",
        "GitHub: squash merges use the default commit message",
        squash_message,
    );

    record(
        report,
        Level::Must,
        "github-update-branch",
        "GitHub: updating pull request branches is always suggested",
        expect_bool(
            repository.allow_update_branch,
            true,
            "Always suggest updating pull request branches is",
        ),
    );
    record(
        report,
        Level::Must,
        "github-auto-merge",
        "GitHub: auto-merge is off",
        expect_bool(repository.allow_auto_merge, false, "Allow auto-merge is"),
    );
    record(
        report,
        Level::Must,
        "github-delete-branch-on-merge",
        "GitHub: head branches are deleted on merge",
        expect_bool(
            repository.delete_branch_on_merge,
            true,
            "Automatically delete head branches is",
        ),
    );
}

fn check_actions(settings: &Settings, report: &mut Report) {
    let workflow = match &settings.workflow {
        Fetched::Ok(workflow) => match (
            workflow.default_workflow_permissions.as_deref(),
            workflow.can_approve_pull_request_reviews,
        ) {
            (Some(permissions), Some(approve)) => {
                let mut issues = Vec::new();
                if permissions != "read" {
                    issues.push(format!("Workflow permissions are {permissions}, not read"));
                }
                if approve {
                    issues.push("GitHub Actions may create and approve pull requests".to_owned());
                }
                Verdict::from(if issues.is_empty() {
                    Ok(())
                } else {
                    Err(issues.join("; "))
                })
            }
            _ => Verdict::skip(NEEDS_ADMIN),
        },
        Fetched::Unavailable(reason) => Verdict::skip(reason),
    };
    record(
        report,
        Level::Must,
        "github-workflow-permissions",
        "GitHub: workflows default to read permissions and can't approve pull requests",
        workflow,
    );

    let sha_pinning = match &settings.actions {
        Fetched::Ok(actions) => expect_bool(
            actions.sha_pinning_required,
            true,
            "Require actions to be pinned to a full-length commit SHA is",
        ),
        Fetched::Unavailable(reason) => Verdict::skip(reason),
    };
    record(
        report,
        Level::Must,
        "github-sha-pinning",
        "GitHub: actions must be pinned to a full-length commit SHA",
        sha_pinning,
    );
}

fn check_security(settings: &Settings, report: &mut Report) {
    let private = settings.repository.private;

    let reporting = match &settings.private_vulnerability_reporting {
        Fetched::Ok(true) => Outcome::Pass.into(),
        Fetched::Ok(false) if private => Verdict::skip(
            "GitHub offers private vulnerability reporting only for public repositories",
        ),
        Fetched::Ok(false) => Outcome::issue("Private vulnerability reporting is off").into(),
        Fetched::Unavailable(reason) => Verdict::skip(reason),
    };
    record(
        report,
        Level::Must,
        "github-private-vulnerability-reporting",
        "GitHub: private vulnerability reporting is on",
        reporting,
    );

    let alerts = match &settings.vulnerability_alerts {
        Fetched::Ok(true) => Outcome::Pass.into(),
        Fetched::Ok(false) => Outcome::issue("Dependabot alerts are off").into(),
        Fetched::Unavailable(reason) => Verdict::skip(reason),
    };
    record(
        report,
        Level::Must,
        "github-dependabot-alerts",
        "GitHub: Dependabot alerts are on",
        alerts,
    );

    let security = settings.repository.security_and_analysis.as_ref();

    let security_updates = match security.and_then(|s| s.dependabot_security_updates.as_ref()) {
        None => Verdict::skip(NEEDS_ADMIN),
        Some(status) if status.enabled() => Outcome::issue(
            "Dependabot security updates are on, though Renovate opens those pull requests",
        )
        .into(),
        Some(_) => Outcome::Pass.into(),
    };
    record(
        report,
        Level::Must,
        "github-dependabot-security-updates",
        "GitHub: Dependabot security updates are off",
        security_updates,
    );

    let scanning = security.map(|s| {
        (
            s.secret_scanning
                .as_ref()
                .is_some_and(FeatureStatus::enabled),
            s.secret_scanning_push_protection
                .as_ref()
                .is_some_and(FeatureStatus::enabled),
        )
    });
    let scanning = match scanning {
        None => Verdict::skip(NEEDS_ADMIN),
        Some((true, true)) => Outcome::Pass.into(),
        Some(_) if private => Verdict::skip(
            "A private repository has secret scanning only with GitHub Secret Protection",
        ),
        Some((false, _)) => Outcome::issue("Secret scanning is off").into(),
        Some((true, false)) => Outcome::issue("Push protection is off").into(),
    };
    record(
        report,
        Level::Must,
        "github-secret-scanning",
        "GitHub: secret scanning and push protection are on",
        scanning,
    );
}

/// What a ruleset the standards describe must be.
struct ExpectedRuleset {
    name: &'static str,
    /// The branch its `ref_name` condition includes, as the API writes it.
    branch: &'static str,
    /// The types of the rules it enables, in the API's order of the standards' table.
    rules: &'static [&'static str],
}

const DEFAULT_RULESET: ExpectedRuleset = ExpectedRuleset {
    name: "Default",
    branch: "~DEFAULT_BRANCH",
    rules: &[
        "deletion",
        "non_fast_forward",
        "required_linear_history",
        "required_signatures",
        "required_status_checks",
    ],
};

const DEVELOPMENT_RULESET: ExpectedRuleset = ExpectedRuleset {
    name: "Development",
    branch: "refs/heads/dev",
    rules: &["deletion", "required_linear_history", "required_signatures"],
};

fn check_rulesets(settings: &Settings, report: &mut Report) {
    let default = match settings.ruleset(DEFAULT_RULESET.name) {
        Fetched::Ok(ruleset) => Verdict::Issues(ruleset_outcome(&DEFAULT_RULESET, ruleset)),
        Fetched::Unavailable(reason) => Verdict::skip(reason),
    };
    record(
        report,
        Level::Must,
        "github-ruleset-default",
        "GitHub: the Default ruleset protects the default branch",
        default,
    );

    let development = match (
        &settings.dev_branch,
        &settings.ruleset(DEVELOPMENT_RULESET.name),
    ) {
        (Fetched::Unavailable(reason), _) | (_, Fetched::Unavailable(reason)) => {
            Verdict::skip(reason.clone())
        }
        (Fetched::Ok(false), _) => Verdict::skip("The repository has no dev branch"),
        (Fetched::Ok(true), Fetched::Ok(ruleset)) => {
            Verdict::Issues(ruleset_outcome(&DEVELOPMENT_RULESET, *ruleset))
        }
    };
    record(
        report,
        Level::Must,
        "github-ruleset-development",
        "GitHub: the Development ruleset protects dev",
        development,
    );
}

/// Whether `ruleset`, the repository's ruleset with the expected name if it has one, is as `expected` describes. Each
/// difference fails, but bypass actors the token isn't shown only warn, since norms can't be sure there are any.
fn ruleset_outcome(expected: &ExpectedRuleset, ruleset: Option<&Ruleset>) -> Result<(), Issue> {
    let Some(ruleset) = ruleset else {
        return Err(Issue::fail(format!(
            "No ruleset is named {}",
            expected.name
        )));
    };

    let mut issues = Vec::new();

    if ruleset.target != "branch" {
        issues.push(format!("It targets {}, not branches", ruleset.target));
    }
    if ruleset.enforcement != "active" {
        issues.push(format!(
            "Its enforcement is {}, not active",
            ruleset.enforcement
        ));
    }

    let ref_name = ruleset
        .conditions
        .as_ref()
        .and_then(|c| c.ref_name.as_ref());
    let targets_branch = ref_name.is_some_and(|r| {
        r.include.iter().eq(std::iter::once(&expected.branch)) && r.exclude.is_empty()
    });
    if !targets_branch {
        let include = ref_name.map_or_else(String::new, |r| r.include.join(", "));
        let exclude = ref_name
            .filter(|r| !r.exclude.is_empty())
            .map_or_else(String::new, |r| {
                format!(", excluding {}", r.exclude.join(", "))
            });
        issues.push(format!(
            "It targets [{include}]{exclude}, not {} alone",
            expected.branch
        ));
    }

    if ruleset
        .bypass_actors
        .as_ref()
        .is_some_and(|actors| !actors.is_empty())
    {
        issues.push("It has bypass actors".to_owned());
    }
    let unseen_bypass_actors = ruleset.bypass_actors.is_none();

    let missing: Vec<_> = expected
        .rules
        .iter()
        .filter(|&&kind| !ruleset.rules.iter().any(|rule| rule.kind == kind))
        .copied()
        .collect();
    if !missing.is_empty() {
        issues.push(format!("It doesn't enable {}", missing.join(", ")));
    }
    let extra: Vec<_> = ruleset
        .rules
        .iter()
        .map(|rule| rule.kind.as_str())
        .filter(|kind| !expected.rules.contains(kind))
        .collect();
    if !extra.is_empty() {
        issues.push(format!("It also enables {}", extra.join(", ")));
    }

    if let Some(rule) = ruleset
        .rules
        .iter()
        .find(|r| r.kind == "required_status_checks")
    {
        issues.extend(status_check_issues(rule.parameters.as_ref()));
    }

    let unseen = unseen_bypass_actors.then(|| {
        Issue::warn(
            "Its bypass actors are shown only to those who can edit it, so they weren't checked",
        )
    });
    Issue::combine(issues.into_iter().map(Issue::fail).chain(unseen))
}

/// What is wrong with a `required_status_checks` rule's parameters: it must require `CI success` from GitHub Actions
/// alone, with branches up to date.
fn status_check_issues(parameters: Option<&serde_json::Value>) -> Vec<String> {
    let parameters: StatusCheckParameters = parameters
        .and_then(|p| serde_json::from_value(p.clone()).ok())
        .unwrap_or_default();
    let mut issues = Vec::new();

    let checks: Vec<_> = parameters
        .required_status_checks
        .iter()
        .map(|check| (check.context.as_str(), check.integration_id))
        .collect();
    if checks != [(REQUIRED_CHECK, Some(GITHUB_ACTIONS_APP_ID))] {
        let required: Vec<_> = checks
            .iter()
            .map(|(context, app)| match app {
                Some(GITHUB_ACTIONS_APP_ID) => format!("{context} from GitHub Actions"),
                Some(app) => format!("{context} from app {app}"),
                None => format!("{context} from any source"),
            })
            .collect();
        issues.push(format!(
            "It requires [{}], not {REQUIRED_CHECK} from GitHub Actions alone",
            required.join(", ")
        ));
    }
    if !parameters.strict_required_status_checks_policy {
        issues.push("It doesn't require branches to be up to date".to_owned());
    }

    issues
}

/// `text` with its first letter in uppercase.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use serde_json::{Value, json};

    use super::*;
    use crate::github::{Api, Response};
    use crate::models::Status;
    use crate::report::CheckResult;

    const REPO: &str = "owner/name";

    /// Recorded responses, by endpoint, and the endpoints requested.
    struct FakeApi {
        responses: HashMap<String, Response>,
        requested: RefCell<Vec<String>>,
    }

    impl Api for FakeApi {
        fn get(&self, endpoint: &str) -> anyhow::Result<Response> {
            self.requested.borrow_mut().push(endpoint.to_owned());
            Ok(self
                .responses
                .get(endpoint)
                .cloned()
                .unwrap_or_else(|| not_found("Not Found")))
        }
    }

    fn not_found(message: &str) -> Response {
        Response::Http {
            status: 404,
            message: message.to_owned(),
        }
    }

    fn forbidden(message: &str) -> Response {
        Response::Http {
            status: 403,
            message: message.to_owned(),
        }
    }

    fn default_ruleset() -> Value {
        json!({
            "id": 1,
            "name": "Default",
            "target": "branch",
            "enforcement": "active",
            "conditions": {"ref_name": {"exclude": [], "include": ["~DEFAULT_BRANCH"]}},
            "rules": [
                {"type": "required_linear_history"},
                {"type": "required_signatures"},
                {"type": "deletion"},
                {"type": "required_status_checks", "parameters": {
                    "strict_required_status_checks_policy": true,
                    "do_not_enforce_on_create": false,
                    "required_status_checks": [{"context": "CI success", "integration_id": 15368}],
                }},
                {"type": "non_fast_forward"},
            ],
            "bypass_actors": [],
        })
    }

    fn development_ruleset() -> Value {
        json!({
            "id": 2,
            "name": "Development",
            "target": "branch",
            "enforcement": "active",
            "conditions": {"ref_name": {"exclude": [], "include": ["refs/heads/dev"]}},
            "rules": [
                {"type": "deletion"},
                {"type": "required_linear_history"},
                {"type": "required_signatures"},
            ],
            "bypass_actors": [],
        })
    }

    /// The responses of a public repository that follows the standards, with a `dev` branch, as norms' own were
    /// recorded.
    fn conforming() -> HashMap<String, Response> {
        let ok = |body: Value| Response::Ok(body);
        HashMap::from([
            (
                format!("repos/{REPO}"),
                ok(json!({
                    "private": false,
                    "has_issues": true,
                    "has_wiki": false,
                    "has_projects": false,
                    "allow_squash_merge": true,
                    "allow_merge_commit": false,
                    "allow_rebase_merge": false,
                    "squash_merge_commit_title": "COMMIT_OR_PR_TITLE",
                    "squash_merge_commit_message": "COMMIT_MESSAGES",
                    "allow_update_branch": true,
                    "allow_auto_merge": false,
                    "delete_branch_on_merge": true,
                    "permissions": {"admin": true, "push": true, "pull": true},
                    "security_and_analysis": {
                        "dependabot_security_updates": {"status": "disabled"},
                        "secret_scanning": {"status": "enabled"},
                        "secret_scanning_push_protection": {"status": "enabled"},
                    },
                })),
            ),
            (
                format!("repos/{REPO}/actions/permissions"),
                ok(
                    json!({"enabled": true, "allowed_actions": "all", "sha_pinning_required": true}),
                ),
            ),
            (
                format!("repos/{REPO}/actions/permissions/workflow"),
                ok(
                    json!({"default_workflow_permissions": "read", "can_approve_pull_request_reviews": false}),
                ),
            ),
            (
                format!("repos/{REPO}/private-vulnerability-reporting"),
                ok(json!({"enabled": true})),
            ),
            (
                format!("repos/{REPO}/vulnerability-alerts"),
                ok(Value::Null),
            ),
            (
                format!("repos/{REPO}/rulesets?per_page=100"),
                ok(json!([
                    {"id": 4, "name": "Default", "source_type": "Organization"},
                    {"id": 1, "name": "Default", "source_type": "Repository"},
                    {"id": 3, "name": "Tags", "source_type": "Repository"},
                    {"id": 2, "name": "Development", "source_type": "Repository"},
                ])),
            ),
            (format!("repos/{REPO}/rulesets/1"), ok(default_ruleset())),
            (
                format!("repos/{REPO}/rulesets/2"),
                ok(development_ruleset()),
            ),
            (
                format!("repos/{REPO}/branches/dev"),
                ok(json!({"name": "dev"})),
            ),
        ])
    }

    fn api(responses: HashMap<String, Response>) -> FakeApi {
        FakeApi {
            responses,
            requested: RefCell::new(Vec::new()),
        }
    }

    /// The responses of [`conforming`], with `endpoint`'s JSON changed by `change`.
    fn changed(endpoint: &str, change: impl FnOnce(&mut Value)) -> HashMap<String, Response> {
        let mut responses = conforming();
        let Some(Response::Ok(body)) = responses.get_mut(&format!("repos/{REPO}{endpoint}")) else {
            panic!("no JSON response for {endpoint}");
        };
        change(body);
        responses
    }

    fn run_with(api: &FakeApi, waivers: &[Waiver]) -> Vec<CheckResult> {
        let settings = Settings::fetch(api, REPO).unwrap();
        github_report(PathBuf::from("."), REPO.to_owned(), &settings, waivers).results
    }

    fn run(responses: HashMap<String, Response>) -> Vec<CheckResult> {
        run_with(&api(responses), &[])
    }

    fn result<'a>(results: &'a [CheckResult], id: &str) -> &'a CheckResult {
        let mut matching = results.iter().filter(|r| r.id == id);
        let found = matching
            .next()
            .unwrap_or_else(|| panic!("{id} not recorded"));
        assert!(matching.next().is_none(), "{id} recorded twice");
        found
    }

    fn status<'a>(results: &'a [CheckResult], id: &str) -> (Status, &'a str) {
        let found = result(results, id);
        (found.status, found.message.as_str())
    }

    #[test]
    fn a_conforming_repository_passes_every_check() {
        let results = run(conforming());
        assert_eq!(results.len(), 16);
        for result in &results {
            assert!(is_github_check(&result.id), "{}", result.id);
            assert_eq!(result.status, Status::Pass, "{result:?}");
        }
    }

    #[test]
    fn only_the_standards_rulesets_of_the_repository_are_read_in_full() {
        let api = api(conforming());
        run_with(&api, &[]);
        let requested = api.requested.borrow();
        assert!(requested.contains(&format!("repos/{REPO}/rulesets/1")));
        assert!(requested.contains(&format!("repos/{REPO}/rulesets/2")));
        assert!(!requested.contains(&format!("repos/{REPO}/rulesets/3")));
        assert!(!requested.contains(&format!("repos/{REPO}/rulesets/4")));
    }

    #[test]
    fn other_merge_settings_fail() {
        let results = run(changed("", |repo| {
            repo["allow_merge_commit"] = json!(true);
            repo["allow_rebase_merge"] = json!(true);
            repo["squash_merge_commit_title"] = json!("PR_TITLE");
            repo["squash_merge_commit_message"] = json!("PR_BODY");
            repo["allow_auto_merge"] = json!(true);
            repo["delete_branch_on_merge"] = json!(false);
            repo["allow_update_branch"] = json!(false);
        }));

        assert_eq!(
            status(&results, "github-merge-methods"),
            (
                Status::Fail,
                "Merge commits are allowed; rebase merging is allowed"
            )
        );
        assert_eq!(
            status(&results, "github-squash-message"),
            (
                Status::Fail,
                "The squash commit takes PR_TITLE and PR_BODY, not GitHub's default message (COMMIT_OR_PR_TITLE \
                 and COMMIT_MESSAGES)"
            )
        );
        assert_eq!(
            status(&results, "github-auto-merge"),
            (Status::Fail, "Allow auto-merge is on")
        );
        assert_eq!(
            status(&results, "github-delete-branch-on-merge"),
            (Status::Fail, "Automatically delete head branches is off")
        );
        assert_eq!(status(&results, "github-update-branch").0, Status::Fail);
    }

    #[test]
    fn squash_merging_turned_off_fails() {
        let results = run(changed("", |repo| {
            repo["allow_squash_merge"] = json!(false);
        }));
        assert_eq!(
            status(&results, "github-merge-methods"),
            (Status::Fail, "Squash merging is not allowed")
        );
    }

    #[test]
    fn a_wiki_or_projects_only_warn_and_issues_off_fails() {
        let results = run(changed("", |repo| {
            repo["has_wiki"] = json!(true);
            repo["has_projects"] = json!(true);
            repo["has_issues"] = json!(false);
        }));
        assert_eq!(
            status(&results, "github-wiki"),
            (Status::Warn, "The wiki is on")
        );
        assert_eq!(
            status(&results, "github-projects"),
            (Status::Warn, "Projects are on")
        );
        assert_eq!(
            status(&results, "github-issues"),
            (Status::Fail, "Issues are off")
        );
    }

    #[test]
    fn waivers_of_these_checks_apply() {
        let waiver = |check: &str| Waiver {
            check: check.to_owned(),
            reason: "The project uses its wiki.".to_owned(),
        };
        let api = api(changed("", |repo| repo["has_wiki"] = json!(true)));
        let results = run_with(&api, &[waiver("github-wiki"), waiver("readme-exists")]);

        assert_eq!(
            status(&results, "github-wiki"),
            (Status::Skip, "Waived: The project uses its wiki.")
        );
        // A waiver of one of `norms verify`'s checks is left to it.
        assert!(
            results
                .iter()
                .all(|r| r.id != "norms-toml-waiver-needed" || r.status == Status::Pass)
        );
        assert_eq!(
            results
                .iter()
                .filter(|r| r.id == "norms-toml-waiver-needed")
                .count(),
            1
        );
    }

    #[test]
    fn a_waiver_of_a_check_skipped_for_want_of_access_is_needed() {
        let mut responses = conforming();
        responses.insert(
            format!("repos/{REPO}/actions/permissions"),
            forbidden("Must have admin rights to Repository."),
        );
        let waiver = Waiver {
            check: "github-sha-pinning".to_owned(),
            reason: "Test.".to_owned(),
        };
        let results = run_with(&api(responses), &[waiver]);
        assert_eq!(status(&results, "norms-toml-waiver-needed").0, Status::Pass);
    }

    #[test]
    fn other_actions_permissions_fail() {
        let mut responses = changed("/actions/permissions/workflow", |workflow| {
            workflow["default_workflow_permissions"] = json!("write");
            workflow["can_approve_pull_request_reviews"] = json!(true);
        });
        responses.insert(
            format!("repos/{REPO}/actions/permissions"),
            Response::Ok(json!({"enabled": true, "sha_pinning_required": false})),
        );
        let results = run(responses);

        assert_eq!(
            status(&results, "github-workflow-permissions"),
            (
                Status::Fail,
                "Workflow permissions are write, not read; GitHub Actions may create and approve pull requests"
            )
        );
        assert_eq!(
            status(&results, "github-sha-pinning"),
            (
                Status::Fail,
                "Require actions to be pinned to a full-length commit SHA is off"
            )
        );
    }

    #[test]
    fn security_features_off_fail_on_a_public_repository() {
        let mut responses = changed("", |repo| {
            repo["security_and_analysis"] = json!({
                "dependabot_security_updates": {"status": "enabled"},
                "secret_scanning": {"status": "enabled"},
                "secret_scanning_push_protection": {"status": "disabled"},
            });
        });
        responses.insert(
            format!("repos/{REPO}/private-vulnerability-reporting"),
            Response::Ok(json!({"enabled": false})),
        );
        responses.insert(
            format!("repos/{REPO}/vulnerability-alerts"),
            not_found("Vulnerability alerts are disabled."),
        );
        let results = run(responses);

        for (id, message) in [
            (
                "github-private-vulnerability-reporting",
                "Private vulnerability reporting is off",
            ),
            ("github-dependabot-alerts", "Dependabot alerts are off"),
            (
                "github-dependabot-security-updates",
                "Dependabot security updates are on, though Renovate opens those pull requests",
            ),
            ("github-secret-scanning", "Push protection is off"),
        ] {
            assert_eq!(status(&results, id), (Status::Fail, message), "{id}");
        }
    }

    #[test]
    fn a_private_repository_without_pro_skips_what_github_doesnt_offer_it() {
        let mut responses = changed("", |repo| {
            repo["private"] = json!(true);
            repo["security_and_analysis"] = json!({
                "dependabot_security_updates": {"status": "disabled"},
                "secret_scanning": {"status": "disabled"},
                "secret_scanning_push_protection": {"status": "disabled"},
            });
        });
        responses.insert(
            format!("repos/{REPO}/private-vulnerability-reporting"),
            Response::Ok(json!({"enabled": false})),
        );
        responses.insert(
            format!("repos/{REPO}/rulesets?per_page=100"),
            forbidden(
                "Upgrade to GitHub Pro or make this repository public to enable this feature.",
            ),
        );
        let results = run(responses);

        for (id, reason) in [
            (
                "github-private-vulnerability-reporting",
                "GitHub offers private vulnerability reporting only for public repositories",
            ),
            (
                "github-secret-scanning",
                "A private repository has secret scanning only with GitHub Secret Protection",
            ),
            (
                "github-ruleset-default",
                "Rulesets on a private repository need GitHub Pro",
            ),
            (
                "github-ruleset-development",
                "Rulesets on a private repository need GitHub Pro",
            ),
        ] {
            assert_eq!(status(&results, id), (Status::Skip, reason), "{id}");
        }
    }

    #[test]
    fn a_token_without_admin_access_skips_what_it_cant_read() {
        let mut responses = changed("", |repo| {
            let repo = repo.as_object_mut().unwrap();
            repo.retain(|key, _| {
                ["private", "has_issues", "has_wiki", "has_projects"].contains(&key.as_str())
            });
            repo.insert(
                "permissions".to_owned(),
                json!({"admin": false, "pull": true}),
            );
        });
        let message = "Must have admin rights to Repository.";
        responses.insert(
            format!("repos/{REPO}/actions/permissions/workflow"),
            forbidden(message),
        );
        responses.insert(
            format!("repos/{REPO}/actions/permissions"),
            forbidden(message),
        );
        let api = api(responses);
        let results = run_with(&api, &[]);

        assert!(
            !api.requested
                .borrow()
                .contains(&format!("repos/{REPO}/vulnerability-alerts"))
        );
        for id in [
            "github-merge-methods",
            "github-squash-message",
            "github-update-branch",
            "github-auto-merge",
            "github-delete-branch-on-merge",
            "github-dependabot-security-updates",
            "github-secret-scanning",
        ] {
            assert_eq!(status(&results, id), (Status::Skip, NEEDS_ADMIN), "{id}");
        }
        assert_eq!(
            status(&results, "github-workflow-permissions"),
            (
                Status::Skip,
                "Couldn't read it: Must have admin rights to Repository. (HTTP 403)"
            )
        );
        assert_eq!(
            status(&results, "github-dependabot-alerts"),
            (
                Status::Skip,
                "Reading Dependabot alerts needs admin access to the repository"
            )
        );
        assert_eq!(status(&results, "github-issues").0, Status::Pass);
    }

    #[test]
    fn the_development_ruleset_is_checked_only_with_a_dev_branch() {
        let mut responses = conforming();
        responses.remove(&format!("repos/{REPO}/branches/dev"));
        responses.insert(
            format!("repos/{REPO}/rulesets?per_page=100"),
            Response::Ok(json!([{"id": 1, "name": "Default", "source_type": "Repository"}])),
        );
        let results = run(responses);
        assert_eq!(
            status(&results, "github-ruleset-development"),
            (Status::Skip, "The repository has no dev branch")
        );

        let mut with_dev = conforming();
        with_dev.insert(
            format!("repos/{REPO}/rulesets?per_page=100"),
            Response::Ok(json!([{"id": 1, "name": "Default", "source_type": "Repository"}])),
        );
        let with_dev = run(with_dev);
        assert_eq!(
            status(&with_dev, "github-ruleset-development"),
            (Status::Fail, "No ruleset is named Development")
        );
    }

    #[test]
    fn a_missing_default_ruleset_fails() {
        let mut responses = conforming();
        responses.insert(
            format!("repos/{REPO}/rulesets?per_page=100"),
            Response::Ok(json!([])),
        );
        let results = run(responses);
        assert_eq!(
            status(&results, "github-ruleset-default"),
            (Status::Fail, "No ruleset is named Default")
        );
    }

    #[test]
    fn a_ruleset_unlike_the_standards_fails_with_every_difference() {
        let results = run(changed("/rulesets/1", |ruleset| {
            ruleset["enforcement"] = json!("evaluate");
            *ruleset.pointer_mut("/conditions/ref_name").unwrap() = json!({"include": ["~DEFAULT_BRANCH", "refs/heads/release"], "exclude": ["refs/heads/x"]});
            ruleset["bypass_actors"] = json!([{"actor_id": 5, "actor_type": "RepositoryRole"}]);
            let rules = ruleset["rules"].as_array_mut().unwrap();
            rules.retain(|rule| rule["type"] != "required_signatures");
            rules.push(json!({"type": "pull_request"}));
            let checks = &mut rules
                .iter_mut()
                .find(|rule| rule["type"] == "required_status_checks")
                .unwrap()["parameters"];
            checks["strict_required_status_checks_policy"] = json!(false);
            checks["required_status_checks"] =
                json!([{"context": "CI success"}, {"context": "lint", "integration_id": 9}]);
        }));

        assert_eq!(
            status(&results, "github-ruleset-default"),
            (
                Status::Fail,
                "Its enforcement is evaluate, not active; It targets [~DEFAULT_BRANCH, refs/heads/release], \
                 excluding refs/heads/x, not ~DEFAULT_BRANCH alone; It has bypass actors; It doesn't enable \
                 required_signatures; It also enables pull_request; It requires [CI success from any source, lint \
                 from app 9], not CI success from GitHub Actions alone; It doesn't require branches to be up to date"
            )
        );
    }

    #[test]
    fn bypass_actors_unreported_to_the_token_warn() {
        let results = run(changed("/rulesets/1", |ruleset| {
            ruleset.as_object_mut().unwrap().remove("bypass_actors");
        }));
        assert_eq!(
            status(&results, "github-ruleset-default"),
            (
                Status::Warn,
                "Its bypass actors are shown only to those who can edit it, so they weren't checked"
            )
        );

        let disabled = run(changed("/rulesets/1", |ruleset| {
            ruleset.as_object_mut().unwrap().remove("bypass_actors");
            ruleset["enforcement"] = json!("disabled");
        }));
        assert_eq!(status(&disabled, "github-ruleset-default").0, Status::Fail);
    }

    #[test]
    fn a_ruleset_that_cant_be_read_is_skipped() {
        let mut responses = conforming();
        responses.insert(
            format!("repos/{REPO}/rulesets/2"),
            forbidden("Resource not accessible"),
        );
        let results = run(responses);
        assert_eq!(
            status(&results, "github-ruleset-development"),
            (
                Status::Skip,
                "Couldn't read it: Resource not accessible (HTTP 403)"
            )
        );
        assert_eq!(status(&results, "github-ruleset-default").0, Status::Pass);
    }

    #[test]
    fn a_repository_that_cant_be_read_is_an_error() {
        let error = Settings::fetch(&api(HashMap::new()), REPO).unwrap_err();
        assert_eq!(
            error.to_string(),
            "Couldn't read the repository owner/name: Not Found (HTTP 404)"
        );
    }

    #[test]
    fn capitalize_uppercases_the_first_letter() {
        assert_eq!(capitalize("merge commits"), "Merge commits");
        assert_eq!(capitalize(""), "");
    }
}
