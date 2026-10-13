//! English text for errors returned to MCP clients.
//!
//! Validation errors carry an i18n key and values (`key|{json}`, see `AppError::validation_with`).
//! The app translates them in the UI; AI clients read English rendered from the same English
//! strings the UI uses (`src/i18n/locales/en/common.json`, embedded at compile time).

use crate::error::AppError;
use serde_json::Value;
use std::sync::LazyLock;

static EN_COMMON: LazyLock<Value> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../../../../src/i18n/locales/en/common.json"))
        .unwrap_or(Value::Null)
});

fn lookup(key: &str) -> Option<&'static str> {
    key.split('.')
        .try_fold(&*EN_COMMON, |node, part| node.get(part))
        .and_then(Value::as_str)
}

/// `"Line {{line}}: {{reason}}"` + `{"line":7,"reason":"…"}` → `"Line 7: …"`; nested keyed
/// values are rendered first.
fn render_keyed(detail: &str) -> Option<String> {
    let (key, params) = match detail.split_once('|') {
        Some((k, p)) => (
            k,
            serde_json::from_str::<serde_json::Map<String, Value>>(p).ok()?,
        ),
        None => (detail, serde_json::Map::new()),
    };
    let mut text = lookup(key)?.to_string();
    for (name, value) in params {
        let shown = match &value {
            Value::String(s) if s.starts_with("validation.") => {
                render_keyed(s).unwrap_or_else(|| s.clone())
            }
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        text = text.replace(&format!("{{{{{name}}}}}"), &shown);
    }
    Some(text)
}

pub fn render(err: &AppError) -> String {
    match err {
        AppError::Validation(detail) if detail.starts_with("validation.") => {
            match render_keyed(detail) {
                Some(text) => format!("Validation error: {text}"),
                None => err.to_string(),
            }
        }
        _ => err.to_string(),
    }
}

/// `render` for an error that was already turned into text (a row of a bulk write report):
/// `Validation error: key|{json}` becomes `Validation error: <English>`, anything else is kept.
pub fn render_message(message: &str) -> String {
    match message.strip_prefix("Validation error: ") {
        Some(detail) if detail.starts_with("validation.") => match render_keyed(detail) {
            Some(text) => format!("Validation error: {text}"),
            None => message.to_string(),
        },
        _ => message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use serde_json::json;

    #[test]
    fn a_keyed_validation_error_reads_as_english() {
        let e = AppError::validation_with(
            "validation.sellWithoutPosition",
            json!({ "ticker": "MSFT" }),
        );
        let text = render(&e);
        assert!(text.starts_with("Validation error: "), "{text}");
        assert!(text.contains("MSFT"), "{text}");
        assert!(!text.contains("validation."), "{text}");
        assert!(!text.contains('|'), "{text}");
    }

    #[test]
    fn a_plain_key_reads_as_english() {
        let e = AppError::Validation("validation.passwordMinLength".into());
        let text = render(&e);
        assert!(!text.contains("validation."), "{text}");
    }

    #[test]
    fn a_nested_keyed_value_is_rendered_too() {
        let inner = AppError::validation_with(
            "validation.currencyMismatch",
            json!({ "expected": "USD", "actual": "EUR" }),
        );
        let crate::error::AppError::Validation(inner_text) = inner else {
            unreachable!()
        };
        let e = AppError::validation_with(
            "validation.stockImportLine",
            json!({ "line": 7, "reason": inner_text }),
        );
        let text = render(&e);
        assert!(
            text.contains('7') && text.contains("USD") && text.contains("EUR"),
            "{text}"
        );
        assert!(!text.contains("validation."), "{text}");
    }

    #[test]
    fn an_unknown_key_falls_back_to_the_raw_message_without_panicking() {
        let e = AppError::Validation("validation.doesNotExist|{\"a\":1}".into());
        assert!(render(&e).contains("validation.doesNotExist"));
    }

    #[test]
    fn other_errors_are_unchanged() {
        let e = AppError::NotFound("position 42".into());
        assert_eq!(render(&e), e.to_string());
    }

    #[test]
    fn every_validation_key_of_the_english_file_renders_without_leftovers() {
        let keys = EN_COMMON["validation"]
            .as_object()
            .expect("validation section");
        assert!(
            keys.len() > 100,
            "the section should not be empty: {}",
            keys.len()
        );
        for (name, template) in keys {
            let template = template.as_str().expect("validation values are strings");
            let params: serde_json::Map<String, Value> = template
                .split("{{")
                .skip(1)
                .filter_map(|rest| rest.split("}}").next())
                .map(|param| (param.to_string(), json!(format!("<{param}>"))))
                .collect();
            let key = format!("validation.{name}");
            let text = render(&AppError::validation_with(
                &key,
                Value::Object(params.clone()),
            ));
            assert!(text.starts_with("Validation error: "), "{key}: {text}");
            assert!(!text.contains("validation."), "{key}: {text}");
            assert!(!text.contains("{{"), "{key}: {text}");
            for param in params.keys() {
                assert!(
                    text.contains(&format!("<{param}>")),
                    "{key} lost {param}: {text}"
                );
            }
        }
    }

    #[test]
    fn a_flattened_message_reads_as_english_too() {
        let e = AppError::validation_with(
            "validation.sellWithoutPosition",
            json!({ "ticker": "MSFT" }),
        );
        assert_eq!(render_message(&e.to_string()), render(&e));
        assert_eq!(
            render_message("Not found: position 42"),
            "Not found: position 42"
        );
        assert_eq!(
            render_message("Category not found: x"),
            "Category not found: x"
        );
        let unknown = "Validation error: validation.doesNotExist|{\"a\":1}";
        assert_eq!(render_message(unknown), unknown);
    }
}
