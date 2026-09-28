//! Model choices discovered from the installed provider CLI or ACP session metadata.
//! Discovery failures leave the catalogue empty; the provider still owns its default.

use std::process::Stdio;
use std::time::Duration;

use serde::Deserialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader as AsyncBufReader};
use tokio::process::Command;

use crate::event::ModelChoice;
use crate::provider::{which, Provider, ProviderId};

fn choice(id: &str, name: &str, description: Option<&str>) -> ModelChoice {
    ModelChoice {
        id: id.to_string(),
        name: name.to_string(),
        description: description.map(|s| s.to_string()),
    }
}

fn effort_label(effort: &str) -> String {
    let mut chars = effort.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Query afresh so account/configuration changes and transient failures can recover.
/// Providers without a standalone catalogue report their choices through ACP session metadata.
pub async fn available_models(provider: &Provider) -> Vec<ModelChoice> {
    let queried = match provider.id {
        ProviderId::Codex => {
            let executable = provider
                .launch
                .env
                .iter()
                .find_map(|(key, value)| (key == "CODEX_PATH").then_some(value.as_str()))
                .map(std::path::PathBuf::from)
                .or_else(|| which("codex"));
            match executable {
                Some(executable) => query_codex_models(executable, &provider.launch.env).await,
                None => Err(()),
            }
        }
        ProviderId::Grok => query_cli_catalog(provider, &["models"], parse_grok_models).await,
        ProviderId::Cursor => {
            query_cli_catalog(provider, &["--list-models"], parse_cursor_models).await
        }
        ProviderId::OpenCode | ProviderId::OpenCode2 => {
            query_cli_catalog(provider, &["models"], parse_opencode_models).await
        }
        _ => return Vec::new(),
    };
    queried.unwrap_or_default()
}

async fn query_cli_catalog(
    provider: &Provider,
    args: &[&str],
    parse: fn(&str) -> Vec<ModelChoice>,
) -> Result<Vec<ModelChoice>, ()> {
    let executable = which(&provider.launch.command).ok_or(())?;
    let mut command = Command::new(executable);
    command
        .args(args)
        .envs(provider.launch.env.iter().cloned())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(8), command.output())
        .await
        .map_err(|_| ())?
        .map_err(|_| ())?;
    if !output.status.success() {
        return Err(());
    }
    let stdout = String::from_utf8(output.stdout).map_err(|_| ())?;
    Ok(parse(&stdout))
}

fn parse_cursor_models(stdout: &str) -> Vec<ModelChoice> {
    stdout
        .lines()
        .filter_map(|line| {
            let (id, raw_name) = line.trim().split_once(" - ")?;
            if id.is_empty() || raw_name.is_empty() {
                return None;
            }
            let name = raw_name.strip_suffix(" (default)").unwrap_or(raw_name);
            Some(choice(id, name, None))
        })
        .collect()
}

fn parse_opencode_models(stdout: &str) -> Vec<ModelChoice> {
    stdout
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty() && line.contains('/') && !line.contains(char::is_whitespace)
        })
        .map(|id| choice(id, id, None))
        .collect()
}

fn parse_grok_models(stdout: &str) -> Vec<ModelChoice> {
    stdout
        .lines()
        .filter_map(|line| {
            let item = line.trim().strip_prefix('*')?.trim();
            let id = item.split_whitespace().next()?;
            (!id.is_empty()).then(|| choice(id, id, None))
        })
        .collect()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexModelList {
    #[serde(default)]
    data: Vec<CodexModel>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexModel {
    id: String,
    display_name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    supported_reasoning_efforts: Vec<CodexReasoningEffort>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodexReasoningEffort {
    reasoning_effort: String,
    #[serde(default)]
    description: Option<String>,
}

fn parse_codex_models(result: &serde_json::Value) -> Result<Vec<ModelChoice>, ()> {
    let list: CodexModelList = serde_json::from_value(result.clone()).map_err(|_| ())?;
    let mut choices = Vec::new();
    for model in list.data.into_iter().filter(|model| !model.hidden) {
        if model.supported_reasoning_efforts.is_empty() {
            choices.push(ModelChoice {
                id: model.id,
                name: model.display_name,
                description: model.description,
            });
            continue;
        }
        for effort in model.supported_reasoning_efforts {
            let description = match (model.description.as_deref(), effort.description.as_deref()) {
                (Some(model), Some(effort)) => Some(format!("{model} {effort}")),
                (Some(model), None) => Some(model.to_string()),
                (None, Some(effort)) => Some(effort.to_string()),
                (None, None) => None,
            };
            choices.push(ModelChoice {
                id: format!("{}[{}]", model.id, effort.reasoning_effort),
                name: format!(
                    "{} ({})",
                    model.display_name,
                    effort_label(&effort.reasoning_effort)
                ),
                description,
            });
        }
    }
    Ok(choices)
}

async fn query_codex_models(
    executable: std::path::PathBuf,
    env: &[(String, String)],
) -> Result<Vec<ModelChoice>, ()> {
    let mut child = Command::new(executable)
        .args(["app-server", "--stdio"])
        .envs(env.iter().cloned())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| ())?;
    let mut stdin = child.stdin.take().ok_or(())?;
    let stdout = child.stdout.take().ok_or(())?;

    write_codex_rpc(
        &mut stdin,
        serde_json::json!({
            "method": "initialize",
            "id": 1,
            "params": {
                "clientInfo": { "name": "codetwo", "title": "C2", "version": env!("CARGO_PKG_VERSION") },
                "capabilities": null
            }
        }),
    )
    .await?;

    let exchange = async {
        let mut choices = Vec::new();
        let mut cursors = std::collections::HashSet::new();
        let mut lines = AsyncBufReader::new(stdout).lines();
        while let Some(line) = lines.next_line().await.map_err(|_| ())? {
            let message: serde_json::Value = serde_json::from_str(&line).map_err(|_| ())?;
            match message.get("id").and_then(serde_json::Value::as_i64) {
                Some(1) if message.get("error").is_some() => return Err(()),
                Some(1) => {
                    write_codex_rpc(
                        &mut stdin,
                        serde_json::json!({ "method": "initialized", "params": {} }),
                    )
                    .await?;
                    write_codex_rpc(
                        &mut stdin,
                        serde_json::json!({ "method": "model/list", "id": 2, "params": { "limit": 100 } }),
                    )
                    .await?;
                }
                Some(2) if message.get("error").is_some() => return Err(()),
                Some(2) => {
                    let result = message.get("result").ok_or(())?;
                    choices.extend(parse_codex_models(result)?);
                    let Some(cursor) = result
                        .get("nextCursor")
                        .and_then(serde_json::Value::as_str)
                        .filter(|cursor| !cursor.is_empty())
                    else {
                        return Ok(choices);
                    };
                    if !cursors.insert(cursor.to_string()) {
                        return Err(());
                    }
                    write_codex_rpc(&mut stdin, serde_json::json!({
                        "method": "model/list", "id": 2, "params": { "limit": 100, "cursor": cursor }
                    })).await?;
                }
                _ => {}
            }
        }
        Err(())
    };

    let result = tokio::time::timeout(Duration::from_secs(8), exchange)
        .await
        .unwrap_or(Err(()));
    drop(stdin);
    let _ = child.kill().await;
    let _ = child.wait().await;
    result
}

async fn write_codex_rpc(
    stdin: &mut tokio::process::ChildStdin,
    message: serde_json::Value,
) -> Result<(), ()> {
    let mut bytes = serde_json::to_vec(&message).map_err(|_| ())?;
    bytes.push(b'\n');
    stdin.write_all(&bytes).await.map_err(|_| ())?;
    stdin.flush().await.map_err(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unknown_catalogues_do_not_invent_models() {
        for id in [
            ProviderId::ClaudeCode,
            ProviderId::Kimi,
            ProviderId::ZCode,
            ProviderId::Amp,
            ProviderId::Custom("future-agent".into()),
        ] {
            let provider = Provider {
                id,
                display_name: "Unknown".into(),
                launch: crate::provider::LaunchSpec::new("missing", [] as [&str; 0]),
                needs_node: false,
            };
            assert!(available_models(&provider).await.is_empty());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn discovery_refreshes_and_honors_provider_environment() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("catalogue");
        std::fs::write(
            &executable,
            "#!/bin/sh\nprintf '* %s\\n' \"$DISCOVERED_MODEL\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut provider = Provider {
            id: ProviderId::Grok,
            display_name: "Test".into(),
            launch: crate::provider::LaunchSpec::new(executable.to_string_lossy(), [] as [&str; 0]),
            needs_node: false,
        };
        for model in ["account-one", "account-two"] {
            provider.launch.env = vec![("DISCOVERED_MODEL".into(), model.into())];
            assert_eq!(available_models(&provider).await[0].id, model);
        }
        std::fs::write(&executable, "#!/bin/sh\nexit 1\n").unwrap();
        assert!(available_models(&provider).await.is_empty());
        std::fs::write(&executable, "#!/bin/sh\nprintf '* recovered\\n'\n").unwrap();
        assert_eq!(available_models(&provider).await[0].id, "recovered");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn codex_catalog_follows_pages_and_rejects_cursor_cycles() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("app-server");
        std::fs::write(
            &executable,
            r#"#!/usr/bin/env python3
import json, os, sys
for line in sys.stdin:
    m = json.loads(line)
    if m.get('id') is None: continue
    result = {}
    if m['method'] == 'model/list':
        cursor = m['params'].get('cursor')
        result = {'data':[{'id': 'page-two' if cursor else 'page-one', 'displayName':'Discovered'}],
                  'nextCursor': 'next' if not cursor or os.environ.get('CYCLE') else None}
    print(json.dumps({'id':m['id'], 'result':result}), flush=True)
"#,
        )
        .unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let models = query_codex_models(executable.clone(), &[]).await.unwrap();
        assert_eq!(
            models
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["page-one", "page-two"]
        );
        assert!(
            query_codex_models(executable, &[("CYCLE".into(), "1".into())])
                .await
                .is_err()
        );
    }

    #[test]
    fn codex_live_catalog_expands_efforts_and_omits_hidden_models() {
        let choices = parse_codex_models(&serde_json::json!({
            "data": [
                {
                    "id": "frontier",
                    "displayName": "Frontier",
                    "description": "Current model.",
                    "supportedReasoningEfforts": [
                        { "reasoningEffort": "low", "description": "Fast" },
                        { "reasoningEffort": "high", "description": "Deep" }
                    ]
                },
                {
                    "id": "hidden",
                    "displayName": "Hidden",
                    "hidden": true,
                    "supportedReasoningEfforts": []
                }
            ]
        }))
        .expect("valid model/list response");

        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].id, "frontier[low]");
        assert_eq!(choices[0].name, "Frontier (Low)");
        assert_eq!(choices[1].id, "frontier[high]");
        assert!(choices.iter().all(|model| !model.id.starts_with("hidden")));
    }

    #[test]
    fn cursor_catalog_keeps_account_ids_and_removes_only_default_marker() {
        let choices = parse_cursor_models(
            "Available models\n\nauto - Auto (default)\ngpt-5.6-sol-high - GPT-5.6 Sol 1M High\n",
        );
        assert_eq!(choices.len(), 2);
        assert_eq!(choices[0].id, "auto");
        assert_eq!(choices[0].name, "Auto");
        assert_eq!(choices[1].id, "gpt-5.6-sol-high");
        assert_eq!(choices[1].name, "GPT-5.6 Sol 1M High");
    }

    #[test]
    fn opencode_catalog_accepts_only_provider_model_ids() {
        let choices =
            parse_opencode_models("opencode/big-pickle\nopenai/gpt-5.6\nnot a model id\n");
        assert_eq!(
            choices
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>(),
            ["opencode/big-pickle", "openai/gpt-5.6",]
        );
    }

    #[test]
    fn grok_catalog_reads_the_cli_bullet_list() {
        let choices = parse_grok_models(
            "Default model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n",
        );
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].id, "grok-4.6");
    }
}
