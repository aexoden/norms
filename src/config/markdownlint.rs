// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `.markdownlint-cli2.jsonc`, and the rule configuration markdownlint derives from it.
//!
//! Only the file's `config` object is read: a config that `extends` another file, or `overrides` that change it for
//! some files, are reported as such, and so are the other markdownlint configuration files the project holds.

use indexmap::IndexMap;
use serde::Deserialize;
use serde_json::{Map, Value};

/// A markdownlint rule: its names (the rule name, then its aliases) and its tags.
struct Rule {
    names: &'static [&'static str],
    tags: &'static [&'static str],
}

/// markdownlint's rules, as of markdownlint 0.42, in its order.
const RULES: &[Rule] = &[
    rule(&["MD001", "heading-increment"], &["headings"]),
    rule(&["MD003", "heading-style"], &["headings"]),
    rule(&["MD004", "ul-style"], &["bullet", "ul"]),
    rule(&["MD005", "list-indent"], &["bullet", "ul", "indentation"]),
    rule(&["MD007", "ul-indent"], &["bullet", "ul", "indentation"]),
    rule(&["MD009", "no-trailing-spaces"], &["whitespace"]),
    rule(&["MD010", "no-hard-tabs"], &["whitespace", "hard_tab"]),
    rule(&["MD011", "no-reversed-links"], &["links"]),
    rule(
        &["MD012", "no-multiple-blanks"],
        &["whitespace", "blank_lines"],
    ),
    rule(&["MD013", "line-length"], &["line_length"]),
    rule(&["MD014", "commands-show-output"], &["code"]),
    rule(
        &["MD018", "no-missing-space-atx"],
        &["headings", "atx", "spaces"],
    ),
    rule(
        &["MD019", "no-multiple-space-atx"],
        &["headings", "atx", "spaces"],
    ),
    rule(
        &["MD020", "no-missing-space-closed-atx"],
        &["headings", "atx_closed", "spaces"],
    ),
    rule(
        &["MD021", "no-multiple-space-closed-atx"],
        &["headings", "atx_closed", "spaces"],
    ),
    rule(
        &["MD022", "blanks-around-headings"],
        &["headings", "blank_lines"],
    ),
    rule(&["MD023", "heading-start-left"], &["headings", "spaces"]),
    rule(&["MD024", "no-duplicate-heading"], &["headings"]),
    rule(&["MD025", "single-title", "single-h1"], &["headings"]),
    rule(&["MD026", "no-trailing-punctuation"], &["headings"]),
    rule(
        &["MD027", "no-multiple-space-blockquote"],
        &["blockquote", "whitespace", "indentation"],
    ),
    rule(
        &["MD028", "no-blanks-blockquote"],
        &["blockquote", "whitespace"],
    ),
    rule(&["MD029", "ol-prefix"], &["ol"]),
    rule(&["MD030", "list-marker-space"], &["ol", "ul", "whitespace"]),
    rule(&["MD031", "blanks-around-fences"], &["code", "blank_lines"]),
    rule(
        &["MD032", "blanks-around-lists"],
        &["bullet", "ul", "ol", "blank_lines"],
    ),
    rule(&["MD033", "no-inline-html"], &["html"]),
    rule(&["MD034", "no-bare-urls"], &["links", "url"]),
    rule(&["MD035", "hr-style"], &["hr"]),
    rule(
        &["MD036", "no-emphasis-as-heading"],
        &["headings", "emphasis"],
    ),
    rule(
        &["MD037", "no-space-in-emphasis"],
        &["whitespace", "emphasis"],
    ),
    rule(&["MD038", "no-space-in-code"], &["whitespace", "code"]),
    rule(&["MD039", "no-space-in-links"], &["whitespace", "links"]),
    rule(&["MD040", "fenced-code-language"], &["code", "language"]),
    rule(
        &["MD041", "first-line-heading", "first-line-h1"],
        &["headings"],
    ),
    rule(&["MD042", "no-empty-links"], &["links"]),
    rule(&["MD043", "required-headings"], &["headings"]),
    rule(&["MD044", "proper-names"], &["spelling"]),
    rule(&["MD045", "no-alt-text"], &["accessibility", "images"]),
    rule(&["MD046", "code-block-style"], &["code"]),
    rule(&["MD047", "single-trailing-newline"], &["blank_lines"]),
    rule(&["MD048", "code-fence-style"], &["code"]),
    rule(&["MD049", "emphasis-style"], &["emphasis"]),
    rule(&["MD050", "strong-style"], &["emphasis"]),
    rule(&["MD051", "link-fragments"], &["links"]),
    rule(&["MD052", "reference-links-images"], &["images", "links"]),
    rule(
        &["MD053", "link-image-reference-definitions"],
        &["images", "links"],
    ),
    rule(&["MD054", "link-image-style"], &["images", "links"]),
    rule(&["MD055", "table-pipe-style"], &["table"]),
    rule(&["MD056", "table-column-count"], &["table"]),
    rule(&["MD058", "blanks-around-tables"], &["table"]),
    rule(
        &["MD059", "descriptive-link-text"],
        &["accessibility", "links"],
    ),
    rule(&["MD060", "table-column-style"], &["table"]),
];

const fn rule(names: &'static [&'static str], tags: &'static [&'static str]) -> Rule {
    Rule { names, tags }
}

impl Rule {
    /// The rule's name, such as `MD013`.
    fn name(&self) -> &'static str {
        self.names.first().copied().unwrap_or_default()
    }

    /// The rule's name and its first alias, such as `MD013 (line-length)`.
    fn label(&self) -> String {
        match self.names {
            [name, alias, ..] => format!("{name} ({alias})"),
            _ => self.name().to_owned(),
        }
    }

    /// Whether a configuration key applies to this rule: one of its names or tags, in any case, as markdownlint
    /// compares them.
    fn matches(&self, key: &str) -> bool {
        self.names
            .iter()
            .chain(self.tags)
            .any(|name| name.eq_ignore_ascii_case(key))
    }
}

/// Top-level `.markdownlint-cli2.jsonc` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct MarkdownlintCli2Config {
    /// markdownlint's rule configuration, in the order written, since a later key naming a rule overrides an earlier
    /// one.
    pub config: Option<IndexMap<String, Value>>,
    /// Configurations that apply to some files instead of `config`, which norms doesn't read.
    pub overrides: Option<Value>,
    /// Glob patterns of files that markdownlint-cli2 doesn't lint, kept as written.
    pub ignores: Option<Value>,
}

/// The styles MD004 knows; it requires dashes for any other.
const MD004_STYLES: [&str; 5] = ["consistent", "asterisk", "plus", "dash", "sublist"];

/// How markdownlint runs a rule, as derived from the configuration.
struct RuleSetting<'a> {
    enabled: bool,
    /// Whether the rule only warns, which doesn't fail markdownlint-cli2.
    warning: bool,
    /// The rule's options, apart from `enabled` and `severity`.
    options: Option<&'a Map<String, Value>>,
}

impl RuleSetting<'_> {
    fn option(&self, name: &str) -> Option<&Value> {
        self.options?.get(name)
    }
}

/// Whether JavaScript treats a value as true.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(string) => !string.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// A value as JavaScript's `Number()` converts it, for the values a number option plausibly holds.
fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.trim().parse().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

impl MarkdownlintCli2Config {
    /// The configuration's entries, empty if it has none.
    fn entries(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.config.iter().flatten()
    }

    /// The value of `default`, which turns every rule on or off unless a rule's own key says otherwise. Only the
    /// first key spelling `default` in any case counts, as in markdownlint.
    fn default_value(&self) -> Option<&Value> {
        self.entries()
            .find(|(key, _)| key.eq_ignore_ascii_case("default"))
            .map(|(_, value)| value)
    }

    /// How markdownlint runs `rule`, following its `getEffectiveConfig`: `default` sets every rule, then each key
    /// naming the rule, by name, alias or tag, replaces the setting in turn.
    fn setting(&self, rule: &Rule) -> RuleSetting<'_> {
        let default = self.default_value();
        let mut setting = RuleSetting {
            enabled: default.is_none_or(truthy),
            warning: default.is_some_and(|value| value == "warning"),
            options: None,
        };
        for (_, value) in self.entries().filter(|(key, _)| rule.matches(key)) {
            setting = match value {
                Value::Object(options) => RuleSetting {
                    enabled: options.get("enabled").is_none_or(truthy),
                    warning: options
                        .get("severity")
                        .is_some_and(|severity| severity == "warning"),
                    options: Some(options),
                },
                value @ (Value::Null
                | Value::Bool(_)
                | Value::Number(_)
                | Value::String(_)
                | Value::Array(_)) => RuleSetting {
                    enabled: truthy(value),
                    warning: value == "warning",
                    options: None,
                },
            };
        }
        setting
    }

    /// Whether the configuration stands alone: markdownlint applies it over the file its `extends` names, which norms
    /// doesn't read, so a rule turned off there would go unseen.
    fn check_not_extended(&self) -> Result<(), String> {
        match self
            .config
            .as_ref()
            .and_then(|config| config.get("extends"))
        {
            None | Some(Value::Null) => {}
            Some(extends) => {
                return Err(format!(
                    "config extends {extends}, which norms doesn't read"
                ));
            }
        }
        match &self.overrides {
            None | Some(Value::Null) => Ok(()),
            Some(Value::Array(overrides)) if overrides.is_empty() => Ok(()),
            Some(_) => Err(
                "overrides change the configuration for some files, which norms doesn't read"
                    .to_owned(),
            ),
        }
    }

    /// The glob patterns of `ignores`, the files markdownlint-cli2 doesn't lint. Entries that aren't strings are left
    /// out.
    pub(crate) fn ignores(&self) -> Vec<&str> {
        match &self.ignores {
            Some(Value::Array(ignores)) => ignores.iter().filter_map(Value::as_str).collect(),
            _ => Vec::new(),
        }
    }

    /// How markdownlint runs the rule named `name`, which must be in [`RULES`].
    fn setting_of(&self, name: &str) -> RuleSetting<'_> {
        RULES.iter().find(|rule| rule.name() == name).map_or_else(
            || RuleSetting {
                enabled: false,
                warning: false,
                options: None,
            },
            |rule| self.setting(rule),
        )
    }

    /// Whether markdownlint runs every rule as an error: `default` isn't off, and no rule is turned off or only warns.
    pub(crate) fn check_rules(&self) -> Result<(), String> {
        self.check_not_extended()?;
        let mut off = Vec::new();
        let mut warning = Vec::new();
        for rule in RULES {
            let setting = self.setting(rule);
            if !setting.enabled {
                off.push(rule.label());
            } else if setting.warning {
                warning.push(rule.label());
            }
        }

        let mut issues = Vec::new();
        if off.len() == RULES.len() {
            issues.push("Every rule is turned off".to_owned());
        } else if !off.is_empty() {
            issues.push(format!("Turned off: {}", off.join(", ")));
        }
        if warning.len() == RULES.len() {
            issues.push("Every rule only warns".to_owned());
        } else if !warning.is_empty() {
            issues.push(format!("Only warning: {}", warning.join(", ")));
        }
        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    }

    /// Whether the standard's rule options hold, after markdownlint's defaults for those left out: MD004 requires
    /// dashes, MD013 wraps at 80 columns, headings included, and exempts tables and code blocks, and MD024 allows
    /// repeated headings under different parents.
    pub(crate) fn check_settings(&self) -> Result<(), String> {
        self.check_not_extended()?;
        let mut issues = Vec::new();

        let md004 = self.setting_of("MD004");
        let style = md004
            .option("style")
            .filter(|style| truthy(style))
            .map_or_else(
                || "consistent".to_owned(),
                |style| {
                    style
                        .as_str()
                        .map_or_else(|| style.to_string(), str::to_owned)
                },
            );
        // markdownlint lints with dashes when the style isn't one it knows.
        if MD004_STYLES.contains(&style.as_str()) && style != "dash" {
            issues.push(format!("MD004 style is {style}, not dash"));
        }

        let md013 = self.setting_of("MD013");
        // Left out or null, the line length is 80.
        if let Some(line_length) = md013.option("line_length").filter(|value| !value.is_null())
            && number(line_length).is_none_or(|length| (length - 80.0).abs() > f64::EPSILON)
        {
            issues.push(format!("MD013 line_length is {line_length}, not 80"));
        }
        for option in ["tables", "code_blocks"] {
            // Left out, the option is on; otherwise any false value turns it off.
            if md013.option(option).is_none_or(truthy) {
                issues.push(format!("MD013 {option} isn't false"));
            }
        }
        // Left out or null, headings have the line length, 80 once that holds.
        if let Some(length) = md013
            .option("heading_line_length")
            .filter(|value| !value.is_null())
            && number(length).is_none_or(|length| (length - 80.0).abs() > f64::EPSILON)
        {
            issues.push(format!("MD013 heading_line_length is {length}, not 80"));
        }
        // Left out, headings are checked; otherwise any false value exempts them.
        if md013
            .option("headings")
            .is_some_and(|headings| !truthy(headings))
        {
            issues.push("MD013 headings isn't true".to_owned());
        }

        if !self
            .setting_of("MD024")
            .option("siblings_only")
            .is_some_and(truthy)
        {
            issues.push("MD024 siblings_only isn't true".to_owned());
        }

        if issues.is_empty() {
            Ok(())
        } else {
            Err(issues.join("; "))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const STANDARD: &str = r#"{
        "config": {
            "default": true,
            // MD004 - Unordered list style
            "MD004": { "style": "dash" },
            "MD013": { "line_length": 80, "tables": false, "code_blocks": false },
            "MD024": { "siblings_only": true },
        },
        "ignores": ["tests/fixtures/**"],
    }"#;

    fn config(json: &str) -> MarkdownlintCli2Config {
        json5::from_str(json).unwrap()
    }

    fn with(entries: &str) -> MarkdownlintCli2Config {
        config(&format!("{{ config: {{ {entries} }} }}"))
    }

    const STANDARD_ENTRIES: &str = r#""MD004": { style: "dash" }, "MD013": { line_length: 80, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#;

    #[test]
    fn standard_config_passes() {
        let config = config(STANDARD);
        assert_eq!(config.check_rules(), Ok(()));
        assert_eq!(config.check_settings(), Ok(()));
    }

    #[test]
    fn settings_follow_aliases_tags_case_and_order() {
        for entries in [
            r#""ul-style": { style: "dash" }, "line-length": { line_length: "80", tables: 0, code_blocks: null }, "no-duplicate-heading": { siblings_only: 1 }"#,
            r#"md004: { style: "dash" }, LINE_LENGTH: { tables: false, code_blocks: "" }, headings: { siblings_only: true }"#,
            // markdownlint lints with dashes when the style isn't one it knows.
            r#""MD004": { style: "Dash" }, "MD013": { tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
            r#""MD004": { style: true }, "MD013": { tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
            r#""MD004": { style: "dash" }, "MD013": { heading_line_length: "80", headings: 1, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
            r#""MD004": { style: "dash" }, "MD013": { heading_line_length: null, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
            // The later key replaces the earlier one's options.
            r#""MD004": { style: "plus" }, ul: { style: "dash" }, "MD013": { line_length: 100, tables: false, code_blocks: false }, "MD013": { tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
        ] {
            assert_eq!(with(entries).check_settings(), Ok(()), "{entries}");
        }
        for (entries, error) in [
            (
                "",
                "MD004 style is consistent, not dash; MD013 tables isn't false; MD013 code_blocks isn't false; MD024 \
                 siblings_only isn't true",
            ),
            (
                r#""MD004": { style: "asterisk" }, "MD013": { line_length: 120, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
                "MD004 style is asterisk, not dash; MD013 line_length is 120, not 80",
            ),
            (
                r#""MD004": { style: "dash" }, "MD013": { line_length: 80, tables: false, code_blocks: false }, "MD024": { siblings_only: true }, headings: true"#,
                "MD024 siblings_only isn't true",
            ),
            (
                r#""MD004": { style: "dash" }, "MD013": { heading_line_length: 100, headings: false, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
                "MD013 heading_line_length is 100, not 80; MD013 headings isn't true",
            ),
            // Unlike a missing value, null is false.
            (
                r#""MD004": { style: "dash" }, "MD013": { headings: null, tables: false, code_blocks: false }, "MD024": { siblings_only: true }"#,
                "MD013 headings isn't true",
            ),
            (
                r#""MD004": { style: "dash" }, "MD013": { line_length: 80, tables: true }, "MD024": { siblings_only: false }"#,
                "MD013 tables isn't false; MD013 code_blocks isn't false; MD024 siblings_only isn't true",
            ),
        ] {
            assert_eq!(
                with(entries).check_settings(),
                Err(error.to_owned()),
                "{entries}"
            );
        }
    }

    #[test]
    fn rules_must_all_run_as_errors() {
        for entries in [
            STANDARD_ENTRIES.to_owned(),
            format!(r#"default: "error", {STANDARD_ENTRIES}"#),
            // Keys that aren't rules are ignored, and a rule turned off may be turned on again.
            format!(r#""$schema": "x", html: false, "MD033": true, {STANDARD_ENTRIES}"#),
            // Only the first default counts.
            format!("default: true, DEFAULT: false, {STANDARD_ENTRIES}"),
        ] {
            assert_eq!(with(&entries).check_rules(), Ok(()), "{entries}");
        }
        for (entries, error) in [
            ("default: false", "Every rule is turned off"),
            (
                r#""MD033": false, "no-bare-urls": null, MD013: { enabled: false }"#,
                "Turned off: MD013 (line-length), MD033 (no-inline-html), MD034 (no-bare-urls)",
            ),
            (r#"default: "warning""#, "Every rule only warns"),
            (
                r#"table: "warning", MD041: { severity: "warning" }, MD040: false"#,
                "Turned off: MD040 (fenced-code-language); Only warning: MD041 (first-line-heading), MD055 \
                 (table-pipe-style), MD056 (table-column-count), MD058 (blanks-around-tables), MD060 \
                 (table-column-style)",
            ),
        ] {
            assert_eq!(
                with(entries).check_rules(),
                Err(error.to_owned()),
                "{entries}"
            );
        }
    }

    #[test]
    fn default_warning_spares_rules_set_again() {
        let error = with(r#"default: "warning", MD001: true, headings: "error""#)
            .check_rules()
            .unwrap_err();
        assert!(
            error.starts_with("Only warning: MD004 (ul-style),"),
            "{error}"
        );
        assert!(
            !error.contains("MD001") && !error.contains("MD024"),
            "{error}"
        );
    }

    #[test]
    fn extended_config_is_reported() {
        let config = with(&format!(r#"extends: "base.jsonc", {STANDARD_ENTRIES}"#));
        let error = Err(r#"config extends "base.jsonc", which norms doesn't read"#.to_owned());
        let overridden: MarkdownlintCli2Config = json5::from_str(&format!(
            r#"{{ "config": {{ {STANDARD_ENTRIES} }}, "overrides": [{{ "files": ["docs/**"], "config": {{ "MD013": false }} }}] }}"#
        ))
        .unwrap();
        let overrides = Err(
            "overrides change the configuration for some files, which norms doesn't read"
                .to_owned(),
        );
        assert_eq!(overridden.check_rules(), overrides);
        assert_eq!(overridden.check_settings(), overrides);
        let empty: MarkdownlintCli2Config = json5::from_str(&format!(
            r#"{{ "config": {{ {STANDARD_ENTRIES} }}, "overrides": [], "ignores": ["vendor/**", 3] }}"#
        ))
        .unwrap();
        assert_eq!(empty.check_rules(), Ok(()));
        assert_eq!(empty.ignores(), ["vendor/**"]);
        assert_eq!(config.check_rules(), error);
        assert_eq!(config.check_settings(), error);
        assert_eq!(
            with(&format!("extends: null, {STANDARD_ENTRIES}")).check_rules(),
            Ok(())
        );
    }

    #[test]
    fn tags_turn_off_only_their_rules() {
        let error = with("Default: 0, headings: true")
            .check_rules()
            .unwrap_err();
        assert!(
            error.starts_with("Turned off: MD004 (ul-style), MD005 (list-indent),"),
            "{error}"
        );
        assert!(!error.contains("MD024"), "{error}");
        assert!(error.ends_with("MD060 (table-column-style)"), "{error}");
    }

    #[test]
    fn missing_config_uses_defaults() {
        let config = config("{}");
        assert_eq!(config.check_rules(), Ok(()));
        assert!(config.check_settings().is_err());
    }
}
