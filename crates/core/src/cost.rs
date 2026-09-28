//! Per-session cost accounting (R7 core half).
//!
//! A [`SessionCostTracker`] folds the engine's [`Event::Usage`] token reports into per-session
//! totals and prices them against a hardcoded table ([`PRICES`]) keyed by (provider, model-id
//! prefix). A provider-reported cumulative cost arriving on [`Event::ContextWindow::cost_usd`] is
//! authoritative and always overrides the table estimate. Unknown models degrade to
//! `priced: false` — token counts stay meaningful, dollars are simply absent.
//!
//! Nothing here persists; totals reset on restart (a future `sessions.cost_usd` column is the
//! upgrade path).

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;

use serde::Serialize;

use crate::event::Event;

/// One (provider, model-prefix) → USD-per-million-tokens price row.
pub struct ModelPrice {
    /// [`crate::provider::ProviderId::as_str`] value ("claude_code", "codex", "grok", …).
    pub provider: &'static str,
    /// Prefix-matched against the session's model id; the longest matching prefix wins.
    pub model_prefix: &'static str,
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
}

const fn price(
    provider: &'static str,
    model_prefix: &'static str,
    input_per_mtok: f64,
    output_per_mtok: f64,
) -> ModelPrice {
    ModelPrice {
        provider,
        model_prefix,
        input_per_mtok,
        output_per_mtok,
    }
}

/// Published per-MTok prices for recognized model ids. Discovery does not depend on this table.
///
/// Deliberately incomplete: models whose vendors publish no per-token USD price (Cursor's
/// `composer-1`, the Kimi K3 and GLM-5 families) are omitted rather than guessed — the designed
/// fallback is `priced: false`. Gemini 2.5 Pro is listed at its base (≤200K-token prompt) tier.
pub const PRICES: &[ModelPrice] = &[
    // Claude Code aliases resolve to the current model in each tier.
    price("claude_code", "opus", 5.00, 25.00),
    price("claude_code", "sonnet", 3.00, 15.00),
    price("claude_code", "haiku", 1.00, 5.00),
    // OpenAI GPT-5.1 family (Codex variants share base pricing; effort suffixes ride the prefix).
    price("codex", "gpt-5.1", 1.25, 10.00),
    // xAI. `grok-4-fast` must out-rank the `grok-4` prefix — longest match wins.
    price("grok", "grok-4", 3.00, 15.00),
    price("grok", "grok-4-fast", 0.20, 0.50),
    price("grok", "grok-code-fast-1", 0.20, 1.50),
    // Cursor's passthrough frontier models (its own `composer-1` has no published token price).
    price("cursor", "sonnet-4.5", 3.00, 15.00),
    price("cursor", "opus-4.1", 15.00, 75.00),
    price("cursor", "gpt-5", 1.25, 10.00),
    // OpenCode routes `provider/model` ids.
    price("opencode", "anthropic/claude-sonnet-4-5", 3.00, 15.00),
    price("opencode", "anthropic/claude-opus-4-1", 15.00, 75.00),
    price("opencode", "openai/gpt-5", 1.25, 10.00),
    price("opencode", "google/gemini-2.5-pro", 1.25, 10.00),
    // Pi is BYOK with the same id scheme.
    price("pi", "anthropic/claude-sonnet-4-5", 3.00, 15.00),
    price("pi", "anthropic/claude-opus-4-1", 15.00, 75.00),
    price("pi", "openai/gpt-5", 1.25, 10.00),
    price("pi", "openai/gpt-5.1", 1.25, 10.00),
    price("pi", "google/gemini-2.5-pro", 1.25, 10.00),
];

/// The price row for `model` under `provider`: prefix-matched, longest prefix wins, `None` on miss.
pub fn price_for(provider: &str, model: &str) -> Option<&'static ModelPrice> {
    // Both OpenCode generations route the same provider/model ids. Keep one price table so adding
    // V2 cannot let the two entries drift apart.
    let provider = if provider == "opencode2" {
        "opencode"
    } else {
        provider
    };
    PRICES
        .iter()
        .filter(|p| p.provider == provider && model.starts_with(p.model_prefix))
        .max_by_key(|p| p.model_prefix.len())
}

/// Samples older than this are dropped from the burn-rate window.
const RETAIN_MS: i64 = 30 * 60 * 1000;
/// Burn rate is the trailing cost delta over this window, scaled to an hour.
const BURN_WINDOW_MS: i64 = 10 * 60 * 1000;
/// A burn rate over a shorter observed span than this is noise, not a rate.
const MIN_SPAN_MS: i64 = 60 * 1000;

#[derive(Default)]
struct SessionUsage {
    input_tokens: u64,
    output_tokens: u64,
    provider: Option<String>,
    model: Option<String>,
    /// From [`Event::ContextWindow::cost_usd`]; wins over the table estimate.
    authoritative_cost: Option<f64>,
    /// (unix ms, cumulative usd) — retained for [`RETAIN_MS`], drives the burn rate.
    samples: VecDeque<(i64, f64)>,
}

impl SessionUsage {
    fn cost_usd(&self) -> Option<f64> {
        if let Some(cost) = self.authoritative_cost {
            return Some(cost);
        }
        let price = price_for(self.provider.as_deref()?, self.model.as_deref()?)?;
        Some(
            self.input_tokens as f64 * price.input_per_mtok / 1e6
                + self.output_tokens as f64 * price.output_per_mtok / 1e6,
        )
    }

    fn push_sample(&mut self, now_ms: i64) {
        if let Some(cost) = self.cost_usd() {
            self.samples.push_back((now_ms, cost));
            while let Some(&(ts, _)) = self.samples.front() {
                if now_ms - ts > RETAIN_MS {
                    self.samples.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    /// `(cost(now) − cost(now − 10 min)) × 6` USD/h. The baseline is the newest sample at or
    /// before the window edge (else the oldest retained sample). `None` when unpriced, fewer than
    /// two samples, or the observed span is under a minute.
    fn burn_rate_usd_per_hour(&self, now_ms: i64) -> Option<f64> {
        if self.samples.len() < 2 {
            return None;
        }
        let &(last_ts, last_cost) = self.samples.back()?;
        let cutoff = now_ms - BURN_WINDOW_MS;
        let (base_ts, base_cost) = self
            .samples
            .iter()
            .rev()
            .find(|(ts, _)| *ts <= cutoff)
            .copied()
            .unwrap_or(*self.samples.front()?);
        if last_ts - base_ts < MIN_SPAN_MS {
            return None;
        }
        Some((last_cost - base_cost) * 6.0)
    }
}

/// Wire shape matches the frontend's `SessionUsage` interface
/// (`{ input_tokens, output_tokens, cost_usd, burn_rate_usd_per_hour, priced }`); `model` rides
/// along as an extra field. Optional fields serialize as explicit `null` — the frontend does
/// strict `!== null` checks, so absent-vs-null matters here.
#[derive(Debug, Clone, Serialize)]
pub struct SessionCostSnapshot {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    pub burn_rate_usd_per_hour: Option<f64>,
    pub priced: bool,
    pub model: Option<String>,
}

/// Accumulates per-session token usage from the event stream and prices it. Thread-safe; intended
/// to sit behind an `Arc` and be fed from a broadcast-subscription task.
#[derive(Default)]
pub struct SessionCostTracker {
    inner: Mutex<HashMap<String, SessionUsage>>,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl SessionCostTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fold one engine event in: `Usage` accumulates token deltas, `ContextWindow.cost_usd`
    /// overrides the table estimate. Everything else is ignored.
    pub fn observe(&self, event: &Event) {
        self.observe_at(event, now_ms());
    }

    fn observe_at(&self, event: &Event, now_ms: i64) {
        match event {
            Event::Usage {
                session,
                input_tokens,
                output_tokens,
            } => {
                let mut inner = self.inner.lock().unwrap();
                let usage = inner.entry(session.clone()).or_default();
                usage.input_tokens += input_tokens;
                usage.output_tokens += output_tokens;
                usage.push_sample(now_ms);
            }
            Event::ContextWindow {
                session,
                cost_usd: Some(cost),
                ..
            } => {
                let mut inner = self.inner.lock().unwrap();
                let usage = inner.entry(session.clone()).or_default();
                usage.authoritative_cost = Some(*cost);
                usage.push_sample(now_ms);
            }
            _ => {}
        }
    }

    /// Record which (provider, model) the session runs on, so table pricing can apply.
    pub fn set_session_model(&self, session: &str, provider: &str, model: &str) {
        let mut inner = self.inner.lock().unwrap();
        let usage = inner.entry(session.to_string()).or_default();
        usage.provider = Some(provider.to_string());
        usage.model = Some(model.to_string());
    }

    /// Whether the session already has a model recorded (used by the feeder to avoid repeated
    /// store lookups).
    pub fn has_model(&self, session: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .get(session)
            .is_some_and(|u| u.model.is_some())
    }

    /// Current totals for `session`, or `None` when it was never observed.
    pub fn snapshot(&self, session: &str) -> Option<SessionCostSnapshot> {
        self.snapshot_at(session, now_ms())
    }

    fn snapshot_at(&self, session: &str, now_ms: i64) -> Option<SessionCostSnapshot> {
        let inner = self.inner.lock().unwrap();
        let usage = inner.get(session)?;
        let cost_usd = usage.cost_usd();
        Some(SessionCostSnapshot {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            cost_usd,
            burn_rate_usd_per_hour: if cost_usd.is_some() {
                usage.burn_rate_usd_per_hour(now_ms)
            } else {
                None
            },
            priced: cost_usd.is_some(),
            model: usage.model.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(session: &str, input: u64, output: u64) -> Event {
        Event::Usage {
            session: session.to_string(),
            input_tokens: input,
            output_tokens: output,
        }
    }

    fn context_cost(session: &str, cost: f64) -> Event {
        Event::ContextWindow {
            session: session.to_string(),
            used_tokens: 0,
            context_window: 200_000,
            cost_usd: Some(cost),
        }
    }

    #[test]
    fn accumulates_usage_and_prices_from_table() {
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "grok", "grok-4");
        tracker.observe_at(&usage("s1", 1_000_000, 100_000), 0);
        tracker.observe_at(&usage("s1", 500_000, 200_000), 1_000);

        let snap = tracker.snapshot_at("s1", 1_000).unwrap();
        assert_eq!(snap.input_tokens, 1_500_000);
        assert_eq!(snap.output_tokens, 300_000);
        assert!(snap.priced);
        // grok-4: $3/MTok in, $15/MTok out.
        let expected = 1.5 * 3.0 + 0.3 * 15.0;
        assert!((snap.cost_usd.unwrap() - expected).abs() < 1e-9);
        assert_eq!(snap.model.as_deref(), Some("grok-4"));
    }

    #[test]
    fn prefix_matching_longest_wins_and_misses() {
        // "grok-4-fast" matches both the "grok-4" and "grok-4-fast" prefixes; longest wins.
        let fast = price_for("grok", "grok-4-fast").unwrap();
        assert_eq!(fast.model_prefix, "grok-4-fast");
        assert!((fast.input_per_mtok - 0.20).abs() < 1e-9);
        // A dated snapshot still rides the shorter prefix.
        let base = price_for("grok", "grok-4-0709").unwrap();
        assert_eq!(base.model_prefix, "grok-4");
        // Longer id under Pi picks the more specific row.
        let gpt51 = price_for("pi", "openai/gpt-5.1").unwrap();
        assert_eq!(gpt51.model_prefix, "openai/gpt-5.1");
        assert_eq!(
            price_for("opencode2", "openai/gpt-5")
                .unwrap()
                .model_prefix,
            "openai/gpt-5"
        );
        // Misses: unpriced provider, wrong provider for a known model, unknown id.
        assert!(price_for("kimi", "k3").is_none());
        assert!(price_for("zcode", "glm-5.2").is_none());
        assert!(price_for("codex", "grok-4").is_none());
        assert!(price_for("codex", "o3").is_none());
    }

    #[test]
    fn unknown_model_reports_tokens_unpriced() {
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "kimi", "k3");
        tracker.observe_at(&usage("s1", 10_000, 2_000), 0);

        let snap = tracker.snapshot_at("s1", 0).unwrap();
        assert_eq!(snap.input_tokens, 10_000);
        assert!(!snap.priced);
        assert!(snap.cost_usd.is_none());
        assert!(snap.burn_rate_usd_per_hour.is_none());
    }

    #[test]
    fn authoritative_cost_overrides_table_estimate() {
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "grok", "grok-4");
        tracker.observe_at(&usage("s1", 1_000_000, 0), 0);
        tracker.observe_at(&context_cost("s1", 9.99), 1_000);

        let snap = tracker.snapshot_at("s1", 1_000).unwrap();
        assert_eq!(snap.cost_usd, Some(9.99));
        assert!(snap.priced);

        // Authoritative cost also prices a model the table doesn't know.
        tracker.set_session_model("s2", "zcode", "glm-5.2");
        tracker.observe_at(&usage("s2", 1_000, 1_000), 0);
        tracker.observe_at(&context_cost("s2", 0.42), 1_000);
        let snap2 = tracker.snapshot_at("s2", 1_000).unwrap();
        assert_eq!(snap2.cost_usd, Some(0.42));
        assert!(snap2.priced);
    }

    #[test]
    fn burn_rate_trailing_window_math() {
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "grok", "grok-4");
        let min = 60_000i64;
        // $3/MTok input → 1M input tokens per observation = $3 cumulative steps.
        tracker.observe_at(&usage("s1", 1_000_000, 0), 0); // cost 3.0
        tracker.observe_at(&usage("s1", 1_000_000, 0), 5 * min); // cost 6.0
        tracker.observe_at(&usage("s1", 1_000_000, 0), 10 * min); // cost 9.0

        // At t=10min the baseline is the t=0 sample (exactly the window edge):
        // (9.0 − 3.0) × 6 = 36 USD/h.
        let snap = tracker.snapshot_at("s1", 10 * min).unwrap();
        assert!((snap.burn_rate_usd_per_hour.unwrap() - 36.0).abs() < 1e-9);

        // Later, with no new spend, the delta over the trailing window shrinks: at t=16min the
        // baseline is the t=5min sample (newest at/before t=6min) → (9.0 − 6.0) × 6 = 18 USD/h.
        let snap = tracker.snapshot_at("s1", 16 * min).unwrap();
        assert!((snap.burn_rate_usd_per_hour.unwrap() - 18.0).abs() < 1e-9);
    }

    #[test]
    fn burn_rate_none_conditions() {
        let min = 60_000i64;

        // Fewer than two samples.
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "grok", "grok-4");
        tracker.observe_at(&usage("s1", 1_000_000, 0), 0);
        assert!(tracker
            .snapshot_at("s1", 5 * min)
            .unwrap()
            .burn_rate_usd_per_hour
            .is_none());

        // Observed span under 60 s.
        tracker.observe_at(&usage("s1", 1_000_000, 0), 30_000);
        assert!(tracker
            .snapshot_at("s1", 30_000)
            .unwrap()
            .burn_rate_usd_per_hour
            .is_none());

        // Unpriced sessions never accumulate samples, so no rate either.
        let unpriced = SessionCostTracker::new();
        unpriced.set_session_model("s2", "kimi", "k3");
        unpriced.observe_at(&usage("s2", 1_000, 0), 0);
        unpriced.observe_at(&usage("s2", 1_000, 0), 5 * min);
        assert!(unpriced
            .snapshot_at("s2", 5 * min)
            .unwrap()
            .burn_rate_usd_per_hour
            .is_none());
    }

    #[test]
    fn samples_older_than_thirty_minutes_are_pruned() {
        let tracker = SessionCostTracker::new();
        tracker.set_session_model("s1", "grok", "grok-4");
        let min = 60_000i64;
        tracker.observe_at(&usage("s1", 1_000_000, 0), 0);
        tracker.observe_at(&usage("s1", 1_000_000, 0), 40 * min);
        // The t=0 sample fell out of the 30-min retention, leaving one sample → no rate.
        let snap = tracker.snapshot_at("s1", 40 * min).unwrap();
        assert!(snap.burn_rate_usd_per_hour.is_none());
    }

    #[test]
    fn snapshot_unknown_session_is_none_and_model_tracking() {
        let tracker = SessionCostTracker::new();
        assert!(tracker.snapshot("nope").is_none());
        assert!(!tracker.has_model("s1"));
        tracker.set_session_model("s1", "claude_code", "sonnet");
        assert!(tracker.has_model("s1"));
        let p = price_for("claude_code", "sonnet").unwrap();
        assert!((p.input_per_mtok - 3.0).abs() < 1e-9);
    }
}
