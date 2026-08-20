//! JSON 文本解析与访问（配置值、清单等）。

/// Parses a JSON text into a [`serde_json::Value`].
///
/// Convenience wrapper around `serde_json::from_str` for the common "parse a JSON string" case.
pub fn parse(text: &str) -> Result<serde_json::Value, serde_json::Error> {
    serde_json::from_str(text)
}

/// Re-exported JSON value type so callers do not need to depend on `serde_json` directly.
pub use serde_json::Value;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scalar_and_object_values() {
        let value = parse(r#"{"app": {"theme": "dark", "flags": [true, false]}}"#).expect("parse");
        assert_eq!(value["app"]["theme"], "dark");
        assert_eq!(value["app"]["flags"][0], true);
    }

    #[test]
    fn rejects_malformed_json() {
        assert!(parse("{not json}").is_err());
    }
}
