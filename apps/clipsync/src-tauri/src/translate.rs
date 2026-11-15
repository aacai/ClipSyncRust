use std::fmt;

use serde_json::Value;

pub const MAX_TRANSLATE_CHARS: usize = 500;
const ENDPOINT: &str = "https://api.mymemory.translated.net/get";

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Target {
    English,
    Chinese,
}

impl Target {
    pub fn langpair(self) -> &'static str {
        match self {
            Self::English => "zh-CN|en",
            Self::Chinese => "en|zh-CN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Request,
    Malformed,
    Empty,
    Rejected,
}

#[derive(Debug)]
pub struct TranslationError {
    pub failure: Failure,
    pub detail: String,
}

impl fmt::Display for TranslationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.failure, self.detail)
    }
}

pub fn detect_target(text: &str) -> Target {
    if text
        .chars()
        .any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c) || ('\u{3040}'..='\u{30ff}').contains(&c))
    {
        Target::English
    } else {
        Target::Chinese
    }
}

pub fn translate(text: &str, target: Target) -> Result<String, TranslationError> {
    let response = ureq::get(ENDPOINT)
        .query("q", text)
        .query("langpair", target.langpair())
        .call();
    let raw = match response {
        Ok(response) => response.into_string(),
        Err(ureq::Error::Status(_, response)) => response.into_string(),
        Err(error) => {
            return Err(TranslationError {
                failure: Failure::Request,
                detail: error.to_string(),
            })
        }
    }
    .map_err(|error| TranslationError {
        failure: Failure::Request,
        detail: error.to_string(),
    })?;
    parse_reply(&raw)
}

pub fn parse_reply(raw: &str) -> Result<String, TranslationError> {
    let body: Value = serde_json::from_str(raw).map_err(|error| TranslationError {
        failure: Failure::Malformed,
        detail: error.to_string(),
    })?;
    let status = body.get("responseStatus").map(status_number).unwrap_or(0);
    let translated_raw = body
        .get("responseData")
        .and_then(|data| data.get("translatedText"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if status != 200 {
        let details = body
            .get("responseDetails")
            .and_then(Value::as_str)
            .filter(|detail| !detail.trim().is_empty());
        let detail = details
            .map(str::to_string)
            .or_else(|| (!translated_raw.is_empty()).then(|| translated_raw.to_string()))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(TranslationError {
            failure: Failure::Rejected,
            detail,
        });
    }
    let translated = unescape_html_entities(translated_raw);
    if translated.starts_with("MYMEMORY WARNING") {
        return Err(TranslationError {
            failure: Failure::Rejected,
            detail: translated,
        });
    }
    if translated.trim().is_empty() {
        return Err(TranslationError {
            failure: Failure::Empty,
            detail: String::new(),
        });
    }
    Ok(translated)
}

fn status_number(value: &Value) -> i32 {
    match value {
        Value::Number(number) => number.as_i64().unwrap_or(0) as i32,
        Value::String(text) => text
            .trim()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse()
            .unwrap_or(0),
        _ => 0,
    }
}

pub fn unescape_html_entities(raw: &str) -> String {
    if !raw.contains('&') {
        return raw.to_string();
    }
    raw.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::{detect_target, parse_reply, unescape_html_entities, Failure, Target};

    #[test]
    fn direction_follows_cjk_presence() {
        assert!(matches!(detect_target("你好"), Target::English));
        assert!(matches!(detect_target("こんにちは"), Target::English));
        assert!(matches!(detect_target("hello"), Target::Chinese));
        assert!(matches!(detect_target("hello 你好"), Target::English));
    }

    #[test]
    fn a_successful_reply_is_unescaped() {
        let raw =
            r#"{"responseData":{"translatedText":"Tom &amp; Jerry&#39;s"},"responseStatus":200}"#;
        assert_eq!(parse_reply(raw).unwrap(), "Tom & Jerry's");
    }

    #[test]
    fn string_error_status_is_rejected() {
        let raw = r#"{"responseData":{"translatedText":"QUERY LENGTH LIMIT"},"responseStatus":"403","responseDetails":"QUERY LENGTH LIMIT"}"#;
        let error = parse_reply(raw).unwrap_err();
        assert_eq!(error.failure, Failure::Rejected);
        assert_eq!(error.detail, "QUERY LENGTH LIMIT");
    }

    #[test]
    fn quota_warning_is_rejected_even_with_200() {
        let raw = r#"{"responseData":{"translatedText":"MYMEMORY WARNING: YOU USED ALL AVAILABLE FREE TRANSLATIONS"},"responseStatus":200}"#;
        assert_eq!(parse_reply(raw).unwrap_err().failure, Failure::Rejected);
    }

    #[test]
    fn blank_and_malformed_replies_carry_the_right_failure() {
        let blank = r#"{"responseData":{"translatedText":"  "},"responseStatus":200}"#;
        assert_eq!(parse_reply(blank).unwrap_err().failure, Failure::Empty);
        assert_eq!(
            parse_reply("not json").unwrap_err().failure,
            Failure::Malformed
        );
    }

    #[test]
    fn entities_round_trip() {
        assert_eq!(
            unescape_html_entities("a &lt;b&gt; &quot;c&quot;&nbsp;d"),
            "a <b> \"c\" d"
        );
    }
}
