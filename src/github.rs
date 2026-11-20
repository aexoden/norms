// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! A GitHub repository's settings and rulesets, read through the GitHub CLI's `gh api`.
//!
//! [`Settings::fetch`] reads every endpoint `norms github` checks, once, through an [`Api`], which tests replace with
//! recorded responses. An endpoint the token can't read, such as one that needs admin access, is recorded as
//! [`Fetched::Unavailable`] with the reason, and its checks are skipped. A failure that leaves nothing to check, such as
//! `gh` missing or not logged in, or the repository not found, is an error.

use std::path::Path;
use std::process::Command;

use anyhow::{Context as _, bail};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

/// The response to a `GET` of a REST API endpoint.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Response {
    /// A successful response, with its JSON body, or `null` for an empty one (`204 No Content`).
    Ok(Value),
    /// An HTTP error, with its status code and GitHub's message.
    Http { status: u16, message: String },
}

/// Reads GitHub's REST API.
pub(crate) trait Api {
    /// `GET` the endpoint, a path such as `repos/OWNER/NAME`. An HTTP error other than an authentication failure is a
    /// [`Response::Http`]; failing to make the request at all, or to authenticate, is an error.
    fn get(&self, endpoint: &str) -> anyhow::Result<Response>;
}

/// The GitHub CLI, run as `gh api`, with the account `gh auth login` set up.
pub(crate) struct Gh;

impl Api for Gh {
    fn get(&self, endpoint: &str) -> anyhow::Result<Response> {
        let output = Command::new("gh")
            .args(["api", "--method", "GET", endpoint])
            .output()
            .context(
                "Failed to run gh, the GitHub CLI (https://cli.github.com/); is it installed?",
            )?;
        parse_gh_api(output.status.success(), &output.stdout, &output.stderr)
            .with_context(|| format!("gh api {endpoint} failed"))
    }
}

/// The JSON body of a REST API error.
#[derive(Deserialize)]
struct ErrorBody {
    status: String,
    message: String,
}

/// The response `gh api` gave, from whether it succeeded and what it printed. On an HTTP error, gh prints GitHub's
/// JSON error body, with its `status` and `message`, to stdout.
fn parse_gh_api(success: bool, stdout: &[u8], stderr: &[u8]) -> anyhow::Result<Response> {
    if success {
        if stdout.iter().all(u8::is_ascii_whitespace) {
            return Ok(Response::Ok(Value::Null));
        }
        return Ok(Response::Ok(
            serde_json::from_slice(stdout).context("gh printed invalid JSON")?,
        ));
    }

    let stderr = String::from_utf8_lossy(stderr);
    let Some((status, message)) = serde_json::from_slice::<ErrorBody>(stdout)
        .ok()
        .and_then(|body| Some((body.status.parse::<u16>().ok()?, body.message)))
    else {
        bail!("{}", stderr.trim());
    };
    if status == 401 {
        bail!("{} (run `gh auth login`)", stderr.trim());
    }
    Ok(Response::Http { status, message })
}

/// The `OWNER/NAME` of the GitHub repository the checkout at `path` belongs to, as gh finds it: from `GH_REPO`, or
/// else the checkout's git remotes.
pub(crate) fn resolve_repository(path: &Path) -> anyhow::Result<String> {
    let output = Command::new("gh")
        .args([
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ])
        .current_dir(path)
        .output()
        .context("Failed to run gh, the GitHub CLI (https://cli.github.com/); is it installed?")?;
    let repository = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !output.status.success() || repository.is_empty() {
        bail!(
            "gh found no GitHub repository for {} (name one with --repo OWNER/NAME): {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(repository)
}

/// An endpoint's data, or why it couldn't be read.
#[derive(Debug, Clone)]
pub(crate) enum Fetched<T> {
    Ok(T),
    Unavailable(String),
}

impl<T> Fetched<T> {
    /// The value read, converted with `f`, or the same reason it couldn't be.
    fn map<U>(self, f: impl FnOnce(T) -> U) -> Fetched<U> {
        match self {
            Self::Ok(value) => Fetched::Ok(f(value)),
            Self::Unavailable(reason) => Fetched::Unavailable(reason),
        }
    }
}

/// What `GET repos/OWNER/NAME` reports. The fields are optional because GitHub leaves some out for a token without
/// admin access to the repository.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Repository {
    pub private: bool,
    pub has_issues: Option<bool>,
    pub has_wiki: Option<bool>,
    pub has_projects: Option<bool>,
    pub allow_squash_merge: Option<bool>,
    pub allow_merge_commit: Option<bool>,
    pub allow_rebase_merge: Option<bool>,
    pub squash_merge_commit_title: Option<String>,
    pub squash_merge_commit_message: Option<String>,
    pub allow_update_branch: Option<bool>,
    pub allow_auto_merge: Option<bool>,
    pub delete_branch_on_merge: Option<bool>,
    pub permissions: Option<Permissions>,
    pub security_and_analysis: Option<SecurityAndAnalysis>,
}

impl Repository {
    /// Whether the token has admin access to the repository, which reading most of its settings needs.
    pub(crate) fn is_admin(&self) -> bool {
        self.permissions.as_ref().is_some_and(|p| p.admin)
    }
}

/// The token's access to the repository.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Permissions {
    pub admin: bool,
}

/// The repository's security features, which GitHub reports only to admins.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct SecurityAndAnalysis {
    pub dependabot_security_updates: Option<FeatureStatus>,
    pub secret_scanning: Option<FeatureStatus>,
    pub secret_scanning_push_protection: Option<FeatureStatus>,
}

/// A security feature's `status`, `enabled` or `disabled`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct FeatureStatus {
    pub status: String,
}

impl FeatureStatus {
    pub(crate) fn enabled(&self) -> bool {
        self.status == "enabled"
    }
}

/// What `GET repos/OWNER/NAME/actions/permissions` reports.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ActionsPermissions {
    pub sha_pinning_required: Option<bool>,
}

/// What `GET repos/OWNER/NAME/actions/permissions/workflow` reports.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct WorkflowPermissions {
    pub default_workflow_permissions: Option<String>,
    pub can_approve_pull_request_reviews: Option<bool>,
}

/// What an endpoint reporting whether a feature is on, such as private vulnerability reporting, returns.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Enabled {
    enabled: bool,
}

/// A ruleset, as `GET repos/OWNER/NAME/rulesets/ID` reports it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Ruleset {
    pub id: u64,
    pub name: String,
    /// What the ruleset belongs to: `Repository`, or, for one it inherits, `Organization` or `Enterprise`.
    pub source_type: String,
    pub target: String,
    pub enforcement: String,
    pub conditions: Option<Conditions>,
    pub rules: Vec<Rule>,
    /// Who may bypass the ruleset, which GitHub reports only to those who can edit it.
    pub bypass_actors: Option<Vec<Value>>,
}

/// A ruleset's conditions.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Conditions {
    pub ref_name: Option<RefName>,
}

/// The branches a ruleset targets.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct RefName {
    pub include: Vec<String>,
    pub exclude: Vec<String>,
}

/// A rule a ruleset enables, by its API type, such as `required_linear_history`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Rule {
    #[serde(rename = "type")]
    pub kind: String,
    pub parameters: Option<Value>,
}

/// The parameters of a `required_status_checks` rule.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct StatusCheckParameters {
    pub strict_required_status_checks_policy: bool,
    pub required_status_checks: Vec<StatusCheck>,
}

/// A required status check, and the app it must come from, if any.
#[derive(Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub(crate) struct StatusCheck {
    pub context: String,
    pub integration_id: Option<u64>,
}

/// The names of the rulesets the standards describe, which [`Settings::fetch`] reads in full if the repository's own,
/// rather than inherited from its organization.
const RULESET_NAMES: [&str; 2] = ["Default", "Development"];

/// Everything `norms github` checks about a repository.
#[derive(Debug)]
pub(crate) struct Settings {
    pub repository: Repository,
    pub actions: Fetched<ActionsPermissions>,
    pub workflow: Fetched<WorkflowPermissions>,
    /// Whether private vulnerability reporting is on.
    pub private_vulnerability_reporting: Fetched<bool>,
    /// Whether Dependabot alerts are on.
    pub vulnerability_alerts: Fetched<bool>,
    /// The repository's rulesets named as the standards name them, by name, each in full or why it couldn't be read.
    pub rulesets: Fetched<Vec<(String, Fetched<Ruleset>)>>,
    /// Whether the repository has a `dev` branch.
    pub dev_branch: Fetched<bool>,
}

impl Settings {
    /// Read the settings of the repository `name` (`OWNER/NAME`).
    pub(crate) fn fetch(api: &dyn Api, name: &str) -> anyhow::Result<Self> {
        let repository = match api.get(&format!("repos/{name}"))? {
            Response::Ok(body) => {
                serde_json::from_value::<Repository>(body).context("Unexpected repository JSON")?
            }
            Response::Http { status, message } => {
                bail!("Couldn't read the repository {name}: {message} (HTTP {status})")
            }
        };

        let actions = fetch_json(api, &format!("repos/{name}/actions/permissions"))?;
        let workflow = fetch_json(api, &format!("repos/{name}/actions/permissions/workflow"))?;

        let private_vulnerability_reporting = fetch_json::<Enabled>(
            api,
            &format!("repos/{name}/private-vulnerability-reporting"),
        )?
        .map(|enabled| enabled.enabled);

        // The endpoint answers 204 when alerts are on and 404 when they're off, but only to an admin.
        let vulnerability_alerts = if repository.is_admin() {
            match api.get(&format!("repos/{name}/vulnerability-alerts"))? {
                Response::Ok(_) => Fetched::Ok(true),
                Response::Http { status: 404, .. } => Fetched::Ok(false),
                Response::Http { status, message } => {
                    Fetched::Unavailable(unavailable(status, &message))
                }
            }
        } else {
            Fetched::Unavailable(
                "Reading Dependabot alerts needs admin access to the repository".to_owned(),
            )
        };

        let rulesets = match api.get(&format!("repos/{name}/rulesets?per_page=100"))? {
            Response::Ok(body) => {
                let summaries: Vec<Ruleset> =
                    serde_json::from_value(body).context("Unexpected rulesets JSON")?;
                let mut rulesets = Vec::new();
                for listed in summaries.iter().filter(|r| {
                    r.source_type == "Repository" && RULESET_NAMES.contains(&r.name.as_str())
                }) {
                    rulesets.push((
                        listed.name.clone(),
                        fetch_json(api, &format!("repos/{name}/rulesets/{}", listed.id))?,
                    ));
                }
                Fetched::Ok(rulesets)
            }
            Response::Http { status: 403, .. } if repository.private => {
                Fetched::Unavailable("Rulesets on a private repository need GitHub Pro".to_owned())
            }
            Response::Http { status, message } => {
                Fetched::Unavailable(unavailable(status, &message))
            }
        };

        let dev_branch = match api.get(&format!("repos/{name}/branches/dev"))? {
            Response::Ok(_) => Fetched::Ok(true),
            Response::Http { status: 404, .. } => Fetched::Ok(false),
            Response::Http { status, message } => {
                Fetched::Unavailable(unavailable(status, &message))
            }
        };

        Ok(Self {
            repository,
            actions,
            workflow,
            private_vulnerability_reporting,
            vulnerability_alerts,
            rulesets,
            dev_branch,
        })
    }

    /// The ruleset with the given name, if the repository has one.
    pub(crate) fn ruleset(&self, name: &str) -> Fetched<Option<&Ruleset>> {
        match &self.rulesets {
            Fetched::Ok(rulesets) => match rulesets.iter().find(|(found, _)| found == name) {
                None => Fetched::Ok(None),
                Some((_, Fetched::Ok(ruleset))) => Fetched::Ok(Some(ruleset)),
                Some((_, Fetched::Unavailable(reason))) => Fetched::Unavailable(reason.clone()),
            },
            Fetched::Unavailable(reason) => Fetched::Unavailable(reason.clone()),
        }
    }
}

/// The JSON body of an endpoint, or why it couldn't be read.
fn fetch_json<T: DeserializeOwned>(api: &dyn Api, endpoint: &str) -> anyhow::Result<Fetched<T>> {
    match api.get(endpoint)? {
        Response::Ok(body) => Ok(Fetched::Ok(
            serde_json::from_value(body)
                .with_context(|| format!("Unexpected JSON from {endpoint}"))?,
        )),
        Response::Http { status, message } => {
            Ok(Fetched::Unavailable(unavailable(status, &message)))
        }
    }
}

/// Why an endpoint couldn't be read, from its HTTP error.
fn unavailable(status: u16, message: &str) -> String {
    format!("Couldn't read it: {message} (HTTP {status})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_api_success_is_its_json_or_null_when_empty() {
        assert_eq!(
            parse_gh_api(true, br#"{"a":1}"#, b"").unwrap(),
            Response::Ok(serde_json::json!({"a": 1}))
        );
        assert_eq!(
            parse_gh_api(true, b"", b"").unwrap(),
            Response::Ok(Value::Null)
        );
        assert!(parse_gh_api(true, b"not json", b"").is_err());
    }

    #[test]
    fn gh_api_http_errors_carry_their_status() {
        let stdout = br#"{"message":"Branch not found","documentation_url":"https://docs.github.com/rest","status":"404"}"#;
        assert_eq!(
            parse_gh_api(false, stdout, b"gh: Branch not found (HTTP 404)").unwrap(),
            Response::Http {
                status: 404,
                message: "Branch not found".to_owned()
            }
        );
    }

    #[test]
    fn gh_api_authentication_and_other_failures_are_errors() {
        let stdout = br#"{"message":"Bad credentials","status":"401"}"#;
        let bad_credentials =
            parse_gh_api(false, stdout, b"gh: Bad credentials (HTTP 401)").unwrap_err();
        assert!(
            bad_credentials.to_string().contains("gh auth login"),
            "{bad_credentials}"
        );

        let logged_out = parse_gh_api(
            false,
            b"",
            b"To get started with GitHub CLI, please run:  gh auth login\n",
        )
        .unwrap_err();
        assert_eq!(
            logged_out.to_string(),
            "To get started with GitHub CLI, please run:  gh auth login"
        );
    }
}
