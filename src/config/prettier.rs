// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! Type-safe model for `.prettierrc`, which Prettier reads as YAML or JSON.
//!
//! Only the subset of fields inspected by checks is modeled. All structs use `#[serde(default)]` so that missing keys
//! deserialize to `None` / empty rather than causing a parse error.

use serde::Deserialize;
use serde_json::Value;

/// Top-level `.prettierrc` structure.
#[derive(Debug, Deserialize, Default)]
#[serde(default)]
pub(crate) struct PrettierConfig {
    /// The plugins Prettier loads, by package name or path. Kept unparsed, so that a malformed value fails only the
    /// check reading it.
    pub plugins: Option<Value>,

    /// The line length Prettier wraps at. Kept unparsed, as for `plugins`.
    #[serde(rename = "printWidth")]
    pub print_width: Option<Value>,
}

impl PrettierConfig {
    /// Check that `printWidth` is `width`. Prettier's own default is 80.
    pub(crate) fn check_print_width(&self, width: u64) -> Result<(), String> {
        match &self.print_width {
            Some(value) if value.as_u64() == Some(width) => Ok(()),
            Some(value) => Err(format!("printWidth is {value}, not {width}")),
            None => Err("Missing printWidth".to_owned()),
        }
    }

    /// Whether `plugins` lists the given package by name.
    pub(crate) fn has_plugin(&self, name: &str) -> bool {
        matches!(&self.plugins, Some(Value::Array(plugins)) if plugins.iter().any(|p| p.as_str() == Some(name)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> PrettierConfig {
        serde_saphyr::from_str(text).unwrap()
    }

    #[test]
    fn print_width_is_checked() {
        assert_eq!(parse("printWidth: 120\n").check_print_width(120), Ok(()));
        assert_eq!(
            parse(r#"{ "printWidth": 80 }"#).check_print_width(120),
            Err("printWidth is 80, not 120".to_owned())
        );
        assert_eq!(
            parse(r#"{ "printWidth": "120" }"#).check_print_width(120),
            Err("printWidth is \"120\", not 120".to_owned())
        );
        assert_eq!(
            parse("plugins: []\n").check_print_width(120),
            Err("Missing printWidth".to_owned())
        );
    }

    #[test]
    fn plugins_are_matched_by_name() {
        let json = parse(r#"{ "plugins": ["prettier-plugin-packagejson"], "printWidth": 120 }"#);
        assert!(json.has_plugin("prettier-plugin-packagejson"));

        let yaml = parse("plugins:\n  - other\n  - prettier-plugin-packagejson\n");
        assert!(yaml.has_plugin("prettier-plugin-packagejson"));

        for text in [
            "printWidth: 120\n",
            "plugins: prettier-plugin-packagejson\n",
            "plugins: [./prettier-plugin-packagejson]\n",
            "plugins: [1]\n",
        ] {
            assert!(
                !parse(text).has_plugin("prettier-plugin-packagejson"),
                "{text}"
            );
        }
    }
}
