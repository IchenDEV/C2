//! A real launch of the running system: the engine spawns an actual provider subprocess (a small
//! Node "stub" ACP agent) over real OS pipes and drives a full turn end to end — compile a document
//! (skill + text) → session/new → session/prompt → streamed updates → permission (auto-answered) →
//! turn end → persisted transcript.
//!
//! Run: `cargo run -p codetwo-core --example live_demo`
//! (Needs `node` on PATH; it stands in for a real ACP provider so no credentials are required.)

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use codetwo_core::event::Event;
use codetwo_core::provider::{LaunchSpec, Provider, ProviderId};
use codetwo_core::skill::{builtin_skills, DocBlock, SkillLibrary};
use codetwo_core::{Engine, Op, Store};

// A minimal ACP agent in JS: answers initialize/new, streams a couple of message chunks (echoing the
// compiled prompt so we can see the skill made it through), reports a tool call, requests permission,
// and finishes the turn once we answer.
const STUB_AGENT_JS: &str = r#"
const rl = require('readline').createInterface({ input: process.stdin });
let promptId = null;
const send = (o) => process.stdout.write(JSON.stringify(o) + "\n");
const update = (u) => send({ jsonrpc:"2.0", method:"session/update", params:{ sessionId:"stub-1", update:u }});
rl.on('line', (line) => {
  if (!line.trim()) return;
  let m; try { m = JSON.parse(line); } catch { return; }
  if (m.method === 'initialize') {
    send({ jsonrpc:"2.0", id:m.id, result:{ protocolVersion:1, agentCapabilities:{} }});
  } else if (m.method === 'session/new') {
    send({ jsonrpc:"2.0", id:m.id, result:{ sessionId:"stub-1" }});
  } else if (m.method === 'session/prompt') {
    promptId = m.id;
    const text = (m.params.prompt && m.params.prompt[0] && m.params.prompt[0].text) || "";
    const preview = text.replace(/\s+/g, ' ').slice(0, 90);
    update({ sessionUpdate:"agent_message_chunk", content:{ type:"text", text:"I received your document. " }});
    update({ sessionUpdate:"agent_message_chunk", content:{ type:"text", text:"Compiled prompt begins: “" + preview + "…” " }});
    update({ sessionUpdate:"tool_call", toolCallId:"t1", title:"read src/login.rs", kind:"read", status:"completed" });
    send({ jsonrpc:"2.0", id:900, method:"session/request_permission", params:{
      sessionId:"stub-1",
      toolCall:{ toolCallId:"t2", title:"cargo test", kind:"execute" },
      options:[ { optionId:"allow", name:"Allow", kind:"allow_once" },
                { optionId:"reject", name:"Reject", kind:"reject_once" } ] }});
  } else if (m.id === 900 && m.method === undefined) {
    const outcome = (m.result && m.result.outcome && m.result.outcome.outcome) || "?";
    update({ sessionUpdate:"agent_message_chunk", content:{ type:"text", text:"Permission was " + outcome + ". Finished." }});
    if (promptId !== null) send({ jsonrpc:"2.0", id:promptId, result:{ stopReason:"end_turn" }});
  }
});
"#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    if codetwo_core::provider::which("node").is_none() {
        eprintln!("node not found on PATH; this demo needs it to run the stub provider.");
        return Ok(());
    }

    // Write the stub provider to a temp file.
    let tmp = std::env::temp_dir().join(format!("codetwo-live-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp)?;
    let script = tmp.join("stub_agent.js");
    std::fs::write(&script, STUB_AGENT_JS)?;

    // Persist to a temp SQLite store so we can print the saved transcript afterward.
    let store = Arc::new(Store::open(
        tmp.join("codetwo.db").to_string_lossy().as_ref(),
    )?);

    // Register the stub as a custom provider and build the engine.
    let stub = Provider {
        id: ProviderId::Custom("stub".into()),
        display_name: "Node stub agent".into(),
        launch: LaunchSpec {
            command: "node".into(),
            args: vec![script.to_string_lossy().to_string()],
            env: Vec::new(),
            cwd: None,
        },
        needs_node: true,
    };
    let skills = SkillLibrary::new(builtin_skills());
    let (engine, mut rx) = Engine::with_store(vec![stub], skills, store.clone());

    println!("── launching engine + spawning provider subprocess ──\n");

    // Start a session; the rest is driven by the event stream.
    engine
        .submit(Op::NewSession {
            provider: ProviderId::Custom("stub".into()),
            cwd: ".".into(),
            use_worktree: false,
            worktree_base: None,
            worktree_base_sha: None,
            request_id: None,
            model: None,
            initial_policy: None,
        })
        .await?;

    let mut session_id = String::new();
    let drive = async {
        while let Some(ev) = rx.recv().await {
            match &ev {
                Event::SessionCreated { session, .. } => {
                    session_id = session.clone();
                    println!("● session created: {}", short(session));
                    // Compose a document: the built-in "reviewer" skill + a line of text.
                    let doc = vec![
                        DocBlock::Skill {
                            skill_id: "reviewer".into(),
                            params: HashMap::new(),
                        },
                        DocBlock::Text {
                            text: "Now refactor the login handler for clarity.".into(),
                        },
                    ];
                    println!("▶ submitting a document (skill:reviewer + text)\n");
                    engine
                        .submit(Op::Prompt {
                            session: session.clone(),
                            doc,
                            request_id: None,
                        })
                        .await
                        .ok();
                }
                Event::SessionTitleChanged { title, .. } => {
                    println!("session title: {title}")
                }
                Event::ProviderChanged {
                    provider, model, ..
                } => {
                    println!("provider changed: {} ({model:?})", provider.as_str())
                }
                Event::SessionActivityChanged { activity, .. } => {
                    println!("activity r{}: {:?}", activity.revision, activity.state)
                }
                Event::TurnStarted { .. } => println!("turn started"),
                Event::AgentText { text, .. } => println!("  🟢 agent: {text}"),
                Event::AgentThought { text, .. } => println!("  💭 thinking: {text}"),
                Event::ToolCall { title, status, .. } => println!("  ⚙  tool: {title} [{status}]"),
                Event::PermissionRequest {
                    session,
                    request_id,
                    title,
                    options,
                    ..
                } => {
                    println!(
                        "  🔐 permission: {title}  options={:?}",
                        options.iter().map(|(_, l)| l).collect::<Vec<_>>()
                    );
                    let opt = options
                        .iter()
                        .find(|(id, _)| id.contains("allow"))
                        .map(|(id, _)| id.clone());
                    println!("  → auto-answering: allow");
                    engine
                        .submit(Op::AnswerPermission {
                            session: session.clone(),
                            request_id: request_id.clone(),
                            option_id: opt,
                        })
                        .await
                        .ok();
                }
                Event::ElicitationRequest {
                    session,
                    request_id,
                    form,
                } => {
                    println!("  ❓ question: {}", form.message);
                    println!("  → auto-answering: skip");
                    engine
                        .submit(Op::AnswerElicitation {
                            session: session.clone(),
                            request_id: request_id.clone(),
                            answer: codetwo_core::elicitation::ElicitationAnswer::Decline,
                        })
                        .await
                        .ok();
                }
                Event::TurnEnded { stop_reason, .. } => {
                    println!("\n■ turn ended: {stop_reason}");
                    break;
                }
                Event::Error { message, .. } => {
                    eprintln!("  ✗ error: {message}");
                    break;
                }
                Event::Usage { .. } => {}
                Event::SubagentUpdated { .. } => {}
                Event::ThreadDisposition { .. } => {}
                Event::ContextWindow { .. } => {}
                Event::MemoryContext { receipt, .. } => {
                    println!("memory: {} recalled items", receipt.items.len());
                }
                Event::Models {
                    available, current, ..
                } => {
                    println!("models: {} available, current = {current}", available.len());
                }
                Event::ConfigOptions { options, .. } => {
                    println!(
                        "config options: {}",
                        options
                            .iter()
                            .map(|o| o.id.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                Event::ExecutionPolicyChanged { policy, .. } => {
                    println!("execution policy: {:?} / {:?}", policy.mode, policy.sandbox);
                }
                // Scene hook/exit/cost projections (R8) and worktree lifecycle — not rendered
                // by this demo.
                Event::TaskSnapshotChanged { .. }
                | Event::TestSignal { .. }
                | Event::ArtifactProduced { .. }
                | Event::ExitCriteriaMet { .. }
                | Event::HookSuggestion { .. }
                | Event::HookTurnStarted { .. }
                | Event::SessionCost { .. }
                | Event::SessionCapabilities { .. }
                | Event::PromptQueued { .. }
                | Event::SteerAccepted { .. }
                | Event::WorktreeDiscarded { .. }
                | Event::AssistantAlert { .. } => {}
            }
        }
    };

    // Guard against a hang.
    if tokio::time::timeout(Duration::from_secs(20), drive)
        .await
        .is_err()
    {
        eprintln!("timed out waiting for the turn to complete");
    }

    // Prove persistence: read the transcript back out of SQLite.
    if !session_id.is_empty() {
        println!("\n── persisted transcript (from SQLite) ──");
        for (role, part) in store.transcript(&session_id).unwrap_or_default() {
            println!("  [{role:?}] {part:?}");
        }
    }

    let _ = std::fs::remove_dir_all(&tmp);
    println!("\n✓ live run complete.");
    Ok(())
}

fn short(s: &str) -> String {
    s.chars().take(8).collect()
}
