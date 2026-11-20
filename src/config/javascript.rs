// SPDX-License-Identifier: MIT OR Apache-2.0
// SPDX-FileCopyrightText: 2026 Jason Lynch <jason@aexoden.com>

//! JSON values as JavaScript tools read them.

use serde_json::Value;

/// Whether JavaScript treats a value as true.
pub(super) fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().is_some_and(|number| number != 0.0),
        Value::String(string) => !string.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// A value as JavaScript's `Number()` converts it, for the values a number option plausibly holds.
pub(super) fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(string) => string.trim().parse().ok(),
        Value::Null | Value::Bool(_) | Value::Array(_) | Value::Object(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn reads_values_as_javascript_does() {
        for value in [json!(true), json!(1), json!("x"), json!([]), json!({})] {
            assert!(truthy(&value), "{value}");
        }
        for value in [json!(null), json!(false), json!(0), json!(0.0), json!("")] {
            assert!(!truthy(&value), "{value}");
        }
        assert_eq!(number(&json!(80)), Some(80.0));
        assert_eq!(number(&json!(" 80.0 ")), Some(80.0));
        assert_eq!(number(&json!(true)), None);
    }
}
