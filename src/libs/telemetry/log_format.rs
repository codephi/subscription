use anyhow::{anyhow, Result};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum LogFormat {
    EcsJson,
    Text,
}

impl LogFormat {
    pub(super) fn from_environment() -> Result<Self> {
        let value = std::env::var("LOG_FORMAT").ok();
        Self::parse_environment_value(value.as_deref())
    }

    fn parse_environment_value(value: Option<&str>) -> Result<Self> {
        match value {
            Some(value) => Self::parse(value),
            None => Ok(Self::EcsJson),
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "json" => Ok(Self::EcsJson),
            "text" => Ok(Self::Text),
            _ => Err(anyhow!(
                "invalid LOG_FORMAT value '{value}'; expected 'json' or 'text'"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LogFormat;

    #[test]
    fn format_defaults_to_json_and_accepts_text_case_insensitively() {
        assert_eq!(
            LogFormat::parse_environment_value(None).unwrap(),
            LogFormat::EcsJson
        );
        assert_eq!(
            LogFormat::parse_environment_value(Some("json")).unwrap(),
            LogFormat::EcsJson
        );
        assert_eq!(
            LogFormat::parse_environment_value(Some("text")).unwrap(),
            LogFormat::Text
        );
        assert_eq!(
            LogFormat::parse_environment_value(Some(" TEXT ")).unwrap(),
            LogFormat::Text
        );
    }

    #[test]
    fn invalid_format_reports_value_and_expected_choices() {
        let error = LogFormat::parse_environment_value(Some("yaml"))
            .unwrap_err()
            .to_string();
        assert!(error.contains("yaml"));
        assert!(error.contains("json"));
        assert!(error.contains("text"));
    }
}
