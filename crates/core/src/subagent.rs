//! Provider-native subagent runs projected from the tool-call stream for UI observability.

use crate::artifact::ToolOutput;
use crate::provider::ProviderId;
use crate::session::{tool_status_is_terminal, SessionId};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_FIELD_CHARS: usize = 2_048;
const MAX_SUMMARY_CHARS: usize = 512;
const MAX_NATIVE_REF_CHARS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentOrigin {
    ProviderNative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubagentStatus {
    Idle,
    Pending,
    Running,
    Waiting,
    Completed,
    Failed,
    Cancelled,
    Interrupted,
}

impl SubagentStatus {
    fn rank(self) -> u8 {
        match self {
            Self::Idle | Self::Pending => 0,
            Self::Waiting => 1,
            Self::Running => 2,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted => 3,
        }
    }

    pub fn should_replace(previous: Option<Self>, next: Self) -> bool {
        match previous {
            None => true,
            Some(prev) if prev.rank() < next.rank() => true,
            Some(prev) if prev.rank() == next.rank() && prev != next => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentRun {
    pub id: String,
    pub parent_tool_call_id: String,
    pub provider: ProviderId,
    pub origin: SubagentOrigin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub status: SubagentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_ref: Option<Value>,
}

fn normalize_token(value: &str) -> String {
    let mut out = String::new();
    let mut separator = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            if ch.is_ascii_uppercase() && !out.is_empty() && !separator {
                out.push('_');
            }
            out.push(ch.to_ascii_lowercase());
            separator = false;
        } else if !out.is_empty() {
            separator = true;
        }
        if separator && !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn is_agent_signal(value: &str) -> bool {
    const SIGNALS: [&str; 13] = [
        "agent",
        "delegate",
        "delegate_task",
        "run_agent",
        "run_subagent",
        "run_workflow",
        "spawn_agent",
        "spawn_subagent",
        "start_agent",
        "start_subagent",
        "subagent",
        "workflow",
        "task",
    ];
    let value = normalize_token(value);
    SIGNALS
        .iter()
        .any(|signal| value == *signal || value.ends_with(&format!("_{signal}")))
}

fn input_object(raw: Option<&Value>) -> Option<Map<String, Value>> {
    let value = raw?;
    let mut map = match value {
        Value::Object(map) => map.clone(),
        Value::String(text) => serde_json::from_str::<Value>(text)
            .ok()?
            .as_object()
            .cloned()?,
        _ => return None,
    };
    for key in ["arguments", "args", "input", "params"] {
        if let Some(Value::Object(nested)) = map.get(key) {
            map.extend(nested.clone());
        }
    }
    Some(map)
}

fn string_field(map: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        map.get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| bounded_text(value, MAX_FIELD_CHARS))
    })
}

fn bounded_text(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

pub fn is_subagent_tool_call(kind: Option<&str>, title: &str, agent_input: Option<&Value>) -> bool {
    if kind.map(is_agent_signal).unwrap_or(false) || is_agent_signal(title) {
        return true;
    }
    let Some(map) = input_object(agent_input) else {
        return false;
    };
    if string_field(
        &map,
        &["agent_type", "agentType", "subagent_type", "subagentType"],
    )
    .is_some()
    {
        return true;
    }
    if string_field(&map, &["task_name", "taskName"]).is_some()
        && string_field(&map, &["message", "prompt", "task", "objective"]).is_some()
    {
        return true;
    }
    if string_field(&map, &["tool", "tool_name", "toolName", "operation"])
        .map(|value| is_agent_signal(&value))
        .unwrap_or(false)
    {
        return true;
    }
    false
}

pub fn map_tool_status(status: &str) -> SubagentStatus {
    match normalize_token(status).as_str() {
        "pending" | "queued" | "scheduled" => SubagentStatus::Pending,
        "waiting" => SubagentStatus::Waiting,
        "in_progress" | "running" => SubagentStatus::Running,
        "completed" | "done" | "success" | "succeeded" => SubagentStatus::Completed,
        "cancelled" | "canceled" => SubagentStatus::Cancelled,
        "interrupted" => SubagentStatus::Interrupted,
        "failed" | "error" | "rejected" | "denied" => SubagentStatus::Failed,
        _ => SubagentStatus::Running,
    }
}

fn output_text_summary(outputs: &[ToolOutput]) -> Option<String> {
    let mut parts = Vec::new();
    let mut budget = MAX_SUMMARY_CHARS;
    for output in outputs {
        if let ToolOutput::Text { text } = output {
            if text.trim().is_empty() {
                continue;
            }
            let take: String = text.chars().take(budget).collect();
            budget = budget.saturating_sub(take.chars().count());
            parts.push(take);
            if budget == 0 {
                break;
            }
        }
    }
    (!parts.is_empty()).then(|| parts.join("\n"))
}

fn result_from_raw(raw_output: Option<&Value>) -> Option<String> {
    let map = input_object(raw_output)?;
    string_field(
        &map,
        &["result_summary", "resultSummary", "summary", "message"],
    )
}

pub fn subagent_id(session: &SessionId, tool_call_id: &str) -> String {
    format!("{session}:{tool_call_id}")
}

pub fn derive_subagent_run(
    session: &SessionId,
    provider: ProviderId,
    tool_call_id: &str,
    title: &str,
    kind: Option<&str>,
    status: &str,
    agent_input: Option<&Value>,
    raw_output: Option<&Value>,
    outputs: &[ToolOutput],
    started_at_ms: Option<i64>,
    completed_at_ms: Option<i64>,
    native_ref: Option<Value>,
) -> Option<SubagentRun> {
    if !is_subagent_tool_call(kind, title, agent_input) {
        return None;
    }
    let map = input_object(agent_input);
    let prompt_summary = map.as_ref().and_then(|map| {
        string_field(
            map,
            &[
                "message",
                "prompt",
                "task",
                "objective",
                "instructions",
                "description",
            ],
        )
    });
    let display_title = map
        .as_ref()
        .and_then(|map| {
            string_field(
                map,
                &["title", "name", "task_name", "taskName", "description"],
            )
        })
        .filter(|value| !is_agent_signal(value))
        .or_else(|| {
            if is_agent_signal(title) {
                None
            } else {
                Some(bounded_text(title.trim(), 128))
            }
        });
    let model = map.as_ref().and_then(|map| string_field(map, &["model"]));
    let mapped = map_tool_status(status);
    let result_summary = if tool_status_is_terminal(status) {
        result_from_raw(raw_output).or_else(|| output_text_summary(outputs))
    } else {
        None
    };
    let progress = if tool_status_is_terminal(status) {
        None
    } else {
        output_text_summary(outputs)
    };
    Some(SubagentRun {
        id: subagent_id(session, tool_call_id),
        parent_tool_call_id: tool_call_id.to_string(),
        provider,
        origin: SubagentOrigin::ProviderNative,
        title: display_title,
        prompt_summary,
        model,
        status: mapped,
        progress: progress.map(|value| bounded_text(&value, MAX_SUMMARY_CHARS)),
        result_summary: result_summary.map(|value| bounded_text(&value, MAX_SUMMARY_CHARS)),
        started_at: started_at_ms,
        completed_at: if tool_status_is_terminal(status) {
            completed_at_ms
        } else {
            None
        },
        native_ref: native_ref.map(|value| bounded_native_ref(value)),
    })
}

fn bounded_native_ref(value: Value) -> Value {
    match value {
        Value::Object(mut map) => {
            map.retain(|_, v| !v.is_null());
            for (_, v) in map.iter_mut() {
                if let Value::String(text) = v {
                    *text = bounded_text(text, MAX_NATIVE_REF_CHARS);
                }
            }
            Value::Object(map)
        }
        Value::String(text) => Value::String(bounded_text(&text, MAX_NATIVE_REF_CHARS)),
        other => other,
    }
}

pub fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collab_style_agent_input_is_recognized() {
        let raw = json!({
            "tool": "researcher",
            "prompt": "Inspect auth flow",
            "model": "gpt-5"
        });
        assert!(is_subagent_tool_call(
            Some("agent"),
            "researcher",
            Some(&raw)
        ));
        let run = derive_subagent_run(
            &"s1".into(),
            ProviderId::Codex,
            "tc-1",
            "researcher",
            Some("agent"),
            "in_progress",
            Some(&raw),
            None,
            &[],
            Some(1),
            None,
            None,
        )
        .unwrap();
        assert_eq!(run.prompt_summary.as_deref(), Some("Inspect auth flow"));
        assert_eq!(run.model.as_deref(), Some("gpt-5"));
        assert_eq!(run.status, SubagentStatus::Running);
    }

    #[test]
    fn status_is_monotonic_by_rank() {
        assert!(SubagentStatus::should_replace(
            Some(SubagentStatus::Pending),
            SubagentStatus::Running
        ));
        assert!(!SubagentStatus::should_replace(
            Some(SubagentStatus::Completed),
            SubagentStatus::Running
        ));
    }

    #[test]
    fn terminal_output_becomes_result_summary() {
        let outputs = vec![ToolOutput::Text {
            text: "done".into(),
        }];
        let run = derive_subagent_run(
            &"s".into(),
            ProviderId::ClaudeCode,
            "id",
            "spawn_agent",
            Some("agent"),
            "completed",
            Some(&json!({"message": "go"})),
            None,
            &outputs,
            Some(10),
            Some(20),
            None,
        )
        .unwrap();
        assert_eq!(run.result_summary.as_deref(), Some("done"));
        assert_eq!(run.completed_at, Some(20));
    }
}
