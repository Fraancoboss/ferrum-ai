use chrono::Utc;
use regex::Regex;
use serde_json::Value;

use crate::models::{EventKind, LlmUsage, NormalizedEvent, ProviderKind};

pub fn normalize_stream_line(
    provider: ProviderKind,
    sequence: i64,
    line: &str,
) -> Vec<NormalizedEvent> {
    let raw =
        serde_json::from_str::<Value>(line).unwrap_or_else(|_| Value::String(line.to_owned()));
    let text = match &raw {
        Value::String(text) => Some(text.clone()),
        _ => extract_assistant_text(&raw),
    };
    let usage = extract_usage(&raw);
    let provider_session_ref = extract_provider_session_ref(&raw);

    let mut events = Vec::new();

    if let Some(provider_session_ref) = provider_session_ref.clone() {
        events.push(NormalizedEvent {
            event_kind: EventKind::ProviderSessionBound,
            provider,
            sequence,
            raw: raw.clone(),
            text: None,
            usage: None,
            provider_session_ref: Some(provider_session_ref),
            created_at: Utc::now(),
        });
    }

    if let Some(usage) = usage.clone() {
        events.push(NormalizedEvent {
            event_kind: EventKind::UsageUpdated,
            provider,
            sequence,
            raw: raw.clone(),
            text: None,
            usage: Some(usage),
            provider_session_ref: provider_session_ref.clone(),
            created_at: Utc::now(),
        });
    }

    if let Some(text) = text.clone().filter(|text| {
        !text.trim().is_empty() && !looks_like_user_echo(&raw) && !looks_like_non_chat_output(&raw)
    }) {
        events.push(NormalizedEvent {
            event_kind: if is_final_event(&raw) {
                EventKind::AssistantFinal
            } else {
                EventKind::AssistantDelta
            },
            provider,
            sequence,
            raw: raw.clone(),
            text: Some(text),
            usage,
            provider_session_ref,
            created_at: Utc::now(),
        });
    }

    if events.is_empty() {
        events.push(NormalizedEvent {
            event_kind: EventKind::RunStarted,
            provider,
            sequence,
            raw,
            text: None,
            usage: None,
            provider_session_ref: None,
            created_at: Utc::now(),
        });
    }

    events
}

pub fn normalize_auth_line(
    provider: ProviderKind,
    sequence: i64,
    line: &str,
) -> Vec<NormalizedEvent> {
    if line.trim().is_empty() {
        return Vec::new();
    }

    let mut events = vec![NormalizedEvent {
        event_kind: EventKind::AuthOutput,
        provider,
        sequence,
        raw: Value::String(line.to_owned()),
        text: Some(line.trim().to_owned()),
        usage: None,
        provider_session_ref: None,
        created_at: Utc::now(),
    }];

    let url_re = Regex::new(r"https?://[^\s]+").expect("valid auth URL regex");
    for url in url_re.find_iter(line) {
        events.push(NormalizedEvent {
            event_kind: EventKind::AuthUrl,
            provider,
            sequence,
            raw: Value::String(url.as_str().to_owned()),
            text: Some(url.as_str().trim_end_matches('.').to_owned()),
            usage: None,
            provider_session_ref: None,
            created_at: Utc::now(),
        });
    }

    events
}

pub fn normalize_stderr_line(provider: ProviderKind, sequence: i64, line: &str) -> NormalizedEvent {
    NormalizedEvent {
        event_kind: EventKind::StdErr,
        provider,
        sequence,
        raw: Value::String(line.to_owned()),
        text: Some(line.to_owned()),
        usage: None,
        provider_session_ref: None,
        created_at: Utc::now(),
    }
}

pub fn extract_assistant_text(value: &Value) -> Option<String> {
    match value {
        Value::String(_) => None,
        Value::Object(map) => {
            let direct_keys = [
                "delta",
                "text",
                "message",
                "content",
                "output_text",
                "result",
                "completion",
                "response",
            ];

            for key in direct_keys {
                if let Some(candidate) = map.get(key) {
                    if let Some(text) = flatten_text(candidate) {
                        if !text.trim().is_empty() {
                            return Some(text);
                        }
                    }
                }
            }

            let nested_keys = [
                "delta",
                "message",
                "content",
                "result",
                "response",
                "output",
                "completion",
                "item",
                "data",
            ];
            for key in nested_keys {
                if let Some(nested) = map.get(key) {
                    if let Some(text) = extract_assistant_text(nested) {
                        if !text.trim().is_empty() {
                            return Some(text);
                        }
                    }
                }
            }
            None
        }
        Value::Array(values) => {
            let joined = values
                .iter()
                .filter_map(flatten_text)
                .collect::<Vec<_>>()
                .join("");
            (!joined.trim().is_empty()).then_some(joined)
        }
        _ => None,
    }
}

fn flatten_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(values) => {
            let joined = values
                .iter()
                .filter_map(flatten_text)
                .collect::<Vec<_>>()
                .join("");
            (!joined.trim().is_empty()).then_some(joined)
        }
        Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(flatten_text) {
                return Some(text);
            }
            if let Some(text) = map.get("content").and_then(flatten_text) {
                return Some(text);
            }
            if let Some(text) = map.get("message").and_then(flatten_text) {
                return Some(text);
            }
            None
        }
        _ => None,
    }
}

pub fn extract_usage(value: &Value) -> Option<LlmUsage> {
    let usage_value = value
        .get("usage")
        .cloned()
        .or_else(|| {
            value
                .get("result")
                .and_then(|result| result.get("usage"))
                .cloned()
        })
        .unwrap_or_else(|| value.clone());

    let input_tokens = find_i64(&usage_value, &["input_tokens", "prompt_tokens"]);
    let output_tokens = find_i64(&usage_value, &["output_tokens", "completion_tokens"]);
    let total_tokens = find_i64(&usage_value, &["total_tokens"]);
    let estimated_cost_usd = find_f64(&usage_value, &["cost_usd", "estimated_cost_usd"]);
    let model = find_string(value, &["model", "model_name"])
        .or_else(|| find_string(&usage_value, &["model", "model_name"]));

    if input_tokens.is_none()
        && output_tokens.is_none()
        && total_tokens.is_none()
        && estimated_cost_usd.is_none()
        && model.is_none()
    {
        return None;
    }

    Some(LlmUsage {
        model,
        input_tokens,
        output_tokens,
        total_tokens: total_tokens.or_else(|| match (input_tokens, output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        }),
        estimated_cost_usd,
    })
}

pub fn extract_provider_session_ref(value: &Value) -> Option<String> {
    find_string(
        value,
        &[
            "session_id",
            "conversation_id",
            "thread_id",
            "sessionId",
            "conversationId",
        ],
    )
}

fn find_i64(value: &Value, keys: &[&str]) -> Option<i64> {
    find_value(value, keys).and_then(|candidate| match candidate {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    })
}

fn find_f64(value: &Value, keys: &[&str]) -> Option<f64> {
    find_value(value, keys).and_then(|candidate| match candidate {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    })
}

fn find_string(value: &Value, keys: &[&str]) -> Option<String> {
    find_value(value, keys).and_then(|candidate| match candidate {
        Value::String(text) => Some(text.clone()),
        other => Some(other.to_string()),
    })
}

fn find_value<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(candidate) = map.get(*key) {
                    return Some(candidate);
                }
            }
            for nested in map.values() {
                if let Some(found) = find_value(nested, keys) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|nested| find_value(nested, keys)),
        _ => None,
    }
}

fn is_final_event(value: &Value) -> bool {
    let body = value.to_string().to_ascii_lowercase();
    body.contains("\"final\"")
        || body.contains("\"result\"")
        || body.contains("\"completed\"")
        || body.contains("\"message_stop\"")
}

fn looks_like_user_echo(value: &Value) -> bool {
    let body = value.to_string().to_ascii_lowercase();
    body.contains("\"role\":\"user\"")
        || body.contains("\"author\":\"user\"")
        || body.contains("\"type\":\"user\"")
        || body.contains("\"type\":\"user_message\"")
        || body.contains("\"event\":\"user\"")
        || body.contains("\"kind\":\"user\"")
}

fn looks_like_non_chat_output(value: &Value) -> bool {
    let body = value.to_string().to_ascii_lowercase();
    let item_type = value
        .get("item")
        .and_then(|item| item.get("type"))
        .and_then(Value::as_str)
        .map(|value| value.to_ascii_lowercase());

    matches!(
        item_type.as_deref(),
        Some("reasoning" | "tool_call" | "tool_result")
    ) || body.contains("\"type\":\"turn.started\"")
        || body.contains("\"type\":\"turn.completed\"")
        || body.contains("\"type\":\"item.started\"")
        || body.contains("\"type\":\"item.updated\"")
        || body.contains("\"type\":\"reasoning\"")
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::models::{EventKind, ProviderKind};

    use super::{
        extract_assistant_text, extract_usage, normalize_auth_line, normalize_stream_line,
    };

    #[test]
    fn extracts_usage_from_nested_payload() {
        let payload = json!({
            "type": "result",
            "usage": {
                "input_tokens": 11,
                "output_tokens": 7
            },
            "model": "gpt-5-codex"
        });

        let usage = extract_usage(&payload).expect("usage");
        assert_eq!(usage.input_tokens, Some(11));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.total_tokens, Some(18));
        assert_eq!(usage.model.as_deref(), Some("gpt-5-codex"));
    }

    #[test]
    fn extracts_text_from_content_array() {
        let payload = json!({
            "content": [
                {"text": "hola "},
                {"text": "mundo"}
            ]
        });

        let text = extract_assistant_text(&payload).expect("text");
        assert_eq!(text, "hola mundo");
    }

    #[test]
    fn normalizes_plain_text_lines_as_events() {
        let events = normalize_stream_line(ProviderKind::Claude, 1, "texto plano");
        assert!(
            events
                .iter()
                .any(|event| event.text.as_deref() == Some("texto plano"))
        );
    }

    #[test]
    fn normalizes_auth_urls() {
        let events = normalize_auth_line(ProviderKind::Codex, 1, "Open https://example.com/auth");
        assert!(
            events
                .iter()
                .any(|event| event.event_kind == EventKind::AuthUrl)
        );
    }

    #[test]
    fn ignores_user_echo_events() {
        let payload = json!({
            "type": "user_message",
            "role": "user",
            "text": "[Pasted Content 123 chars]"
        });
        let events = normalize_stream_line(ProviderKind::Codex, 1, &payload.to_string());
        assert!(
            !events
                .iter()
                .any(|event| event.event_kind == EventKind::AssistantDelta)
        );
    }

    #[test]
    fn ignores_control_event_text() {
        let payload = json!({
            "type": "turn.started",
            "event": "meta"
        });
        let events = normalize_stream_line(ProviderKind::Codex, 1, &payload.to_string());
        assert!(!events.iter().any(|event| {
            matches!(
                event.event_kind,
                EventKind::AssistantDelta | EventKind::AssistantFinal
            )
        }));
    }

    #[test]
    fn ignores_reasoning_items() {
        let payload = json!({
            "type": "item.completed",
            "item": {
                "id": "item_0",
                "type": "reasoning",
                "text": "**Preparing summary response**"
            }
        });
        let events = normalize_stream_line(ProviderKind::Codex, 1, &payload.to_string());
        assert!(!events.iter().any(|event| {
            matches!(
                event.event_kind,
                EventKind::AssistantDelta | EventKind::AssistantFinal
            )
        }));
    }

    #[test]
    fn keeps_agent_message_items() {
        let payload = json!({
            "type": "item.completed",
            "item": {
                "id": "item_1",
                "type": "agent_message",
                "text": "Hola desde Codex"
            }
        });
        let events = normalize_stream_line(ProviderKind::Codex, 1, &payload.to_string());
        assert!(
            events
                .iter()
                .any(|event| event.text.as_deref() == Some("Hola desde Codex"))
        );
    }
}
