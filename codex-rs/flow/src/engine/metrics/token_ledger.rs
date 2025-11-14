use codex_exec::exec_events::Usage;

use crate::runner::state_store::TokenUsage;

/// Records token usage emitted by engine runners so we can persist cost data in
/// `WorkflowRunState`.
pub trait UsageRecorder {
    fn record_turn_usage(&mut self, usage: &Usage);
}

#[derive(Default)]
pub struct TokenLedger {
    total: TokenUsage,
    has_usage: bool,
}

impl TokenLedger {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn step(&'_ mut self, model: &str) -> StepHandle<'_> {
        StepHandle::new(self, ModelPricing::for_model(model))
    }

    fn commit(&mut self, usage: &TokenUsage) {
        self.total.prompt_tokens += usage.prompt_tokens;
        self.total.completion_tokens += usage.completion_tokens;
        self.total.total_tokens += usage.total_tokens;
        self.total.total_cost += usage.total_cost;
        self.has_usage = true;
    }

    pub fn total_usage(&self) -> Option<&TokenUsage> {
        self.has_usage.then_some(&self.total)
    }
}

pub struct StepHandle<'a> {
    ledger: &'a mut TokenLedger,
    usage: TokenUsage,
    pricing: ModelPricing,
    has_usage: bool,
}

impl<'a> StepHandle<'a> {
    fn new(ledger: &'a mut TokenLedger, pricing: ModelPricing) -> Self {
        Self {
            ledger,
            usage: TokenUsage::default(),
            pricing,
            has_usage: false,
        }
    }

    pub fn finish(self) -> Option<TokenUsage> {
        if !self.has_usage {
            return None;
        }
        self.ledger.commit(&self.usage);
        Some(self.usage)
    }
}

impl UsageRecorder for StepHandle<'_> {
    fn record_turn_usage(&mut self, usage: &Usage) {
        let prompt_tokens = usage.input_tokens.saturating_add(usage.cached_input_tokens);
        let completion_tokens = usage.output_tokens;
        let total_tokens = prompt_tokens.saturating_add(completion_tokens);

        self.usage.prompt_tokens += prompt_tokens;
        self.usage.completion_tokens += completion_tokens;
        self.usage.total_tokens += total_tokens;
        self.usage.total_cost += self
            .pricing
            .cost(prompt_tokens as f64, completion_tokens as f64);
        self.has_usage = true;
    }
}

#[derive(Clone, Copy)]
struct ModelPricing {
    prompt_per_token: f64,
    completion_per_token: f64,
}

impl ModelPricing {
    const fn new(prompt_per_token: f64, completion_per_token: f64) -> Self {
        Self {
            prompt_per_token,
            completion_per_token,
        }
    }

    fn for_model(model: &str) -> Self {
        let slug = model.to_ascii_lowercase();
        if slug.starts_with("gpt-4o") {
            // $5 / $15 per 1M tokens.
            Self::new(0.000_005, 0.000_015)
        } else if slug.starts_with("o4-mini") {
            // $2.5 / $10 per 1M tokens.
            Self::new(0.000_002_5, 0.000_010)
        } else if slug.starts_with("o3") {
            Self::new(0.000_015, 0.000_060)
        } else if slug.starts_with("gpt-4.1") {
            // $30 / $60 per 1M tokens.
            Self::new(0.000_030, 0.000_060)
        } else if slug.starts_with("gpt-5") || slug.starts_with("codex-") {
            Self::new(0.000_030, 0.000_060)
        } else if slug.starts_with("gpt-3.5") {
            // $0.50 / $1.50 per 1M tokens.
            Self::new(0.000_000_5, 0.000_001_5)
        } else {
            Self::new(0.0, 0.0)
        }
    }

    fn cost(&self, prompt_tokens: f64, completion_tokens: f64) -> f64 {
        (prompt_tokens * self.prompt_per_token) + (completion_tokens * self.completion_per_token)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: i64, cached: i64, output: i64) -> Usage {
        Usage {
            input_tokens: input,
            cached_input_tokens: cached,
            output_tokens: output,
        }
    }

    #[test]
    fn accumulates_usage() {
        let mut ledger = TokenLedger::new();

        {
            let mut step = ledger.step("gpt-4o");
            step.record_turn_usage(&usage(1_000, 0, 200));
            let delta = step.finish().expect("delta");
            assert_eq!(delta.prompt_tokens, 1_000);
            assert_eq!(delta.completion_tokens, 200);
            assert_eq!(delta.total_tokens, 1_200);
            assert!((delta.total_cost - 0.008).abs() < 1e-9);
        }

        {
            let mut step = ledger.step("mystery-model");
            step.record_turn_usage(&usage(0, 50, 10));
            let delta = step.finish().expect("delta");
            assert_eq!(delta.prompt_tokens, 50);
            assert_eq!(delta.completion_tokens, 10);
            assert_eq!(delta.total_tokens, 60);
            assert_eq!(delta.total_cost, 0.0);
        }

        let total = ledger.total_usage().expect("total usage");
        assert_eq!(total.prompt_tokens, 1_050);
        assert_eq!(total.completion_tokens, 210);
        assert_eq!(total.total_tokens, 1_260);
        assert!((total.total_cost - 0.008).abs() < 1e-9);
    }
}
