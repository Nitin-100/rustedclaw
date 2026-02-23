//! CLI commands for telemetry / usage / cost tracking.

use rustedclaw_config::AppConfig;
use rustedclaw_telemetry::{PricingTable, TelemetryEngine};
use std::sync::Arc;

/// Show current usage snapshot (costs, tokens, budgets).
pub async fn usage() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load()?;
    let engine = build_telemetry(&config);

    let snapshot = engine.usage_snapshot();

    println!("📊 Usage Snapshot");
    println!("─────────────────────────────────────");
    println!("  Session cost:   ${:.6}", snapshot.session_cost_usd);
    println!("  Daily cost:     ${:.6}", snapshot.daily_cost_usd);
    println!("  Monthly cost:   ${:.6}", snapshot.monthly_cost_usd);
    println!("  Total cost:     ${:.6}", snapshot.total_cost_usd);
    println!("  Session tokens: {}", snapshot.session_tokens);
    println!("  Traces:         {}", snapshot.trace_count);

    if !snapshot.budgets.is_empty() {
        println!();
        println!("  Budgets:");
        for b in &snapshot.budgets {
            let pct = if b.max_usd > 0.0 {
                (b.spent_usd / b.max_usd * 100.0).min(100.0)
            } else {
                0.0
            };
            let status = if b.exceeded { "⛔ EXCEEDED" } else { "✅" };
            println!(
                "    {}: ${:.4} / ${:.4} ({:.1}%) {}",
                b.scope, b.spent_usd, b.max_usd, pct, status
            );
        }
    }

    Ok(())
}

/// List available model pricing.
pub async fn pricing() -> Result<(), Box<dyn std::error::Error>> {
    let table = PricingTable::with_defaults();
    let models = table.models();

    println!("💰 Model Pricing (per 1M tokens)");
    println!("─────────────────────────────────────────────────────");
    println!("{:<40} {:>10} {:>10}", "Model", "Input", "Output");
    println!("{:<40} {:>10} {:>10}", "─────", "─────", "──────");

    for name in &models {
        if let Some(p) = table.get(name) {
            println!(
                "{:<40} ${:>8.3} ${:>8.3}",
                name, p.input_per_m, p.output_per_m
            );
        }
    }

    println!();
    println!("  {} models with pricing data", models.len());

    Ok(())
}

/// Show configured budgets.
pub async fn budgets() -> Result<(), Box<dyn std::error::Error>> {
    let config = AppConfig::load()?;

    if config.telemetry.budgets.is_empty() {
        println!("No budgets configured.");
        println!();
        println!("Add budgets in ~/.rustedclaw/config.toml:");
        println!();
        println!("  [[telemetry.budgets]]");
        println!("  scope = \"daily\"");
        println!("  max_usd = 5.00");
        println!("  on_exceed = \"deny\"");
        return Ok(());
    }

    println!("🔒 Configured Budgets");
    println!("─────────────────────────────────────");
    for b in &config.telemetry.budgets {
        println!(
            "  {} → max ${:.2}, tokens {}, action: {}",
            b.scope,
            b.max_usd,
            if b.max_tokens > 0 {
                b.max_tokens.to_string()
            } else {
                "unlimited".into()
            },
            b.on_exceed
        );
    }

    Ok(())
}

/// Estimate cost for a given model and token counts.
pub async fn estimate(
    model: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> Result<(), Box<dyn std::error::Error>> {
    let table = PricingTable::with_defaults();

    let cost = table.compute_cost(model, input_tokens, output_tokens);
    if cost == 0.0 {
        println!("⚠ Model '{}' not found in pricing table.", model);
        println!("  Use `rustedclaw usage pricing` to see available models.");
    } else {
        println!("💵 Cost estimate for {}", model);
        println!("   Input tokens:  {}", input_tokens);
        println!("   Output tokens: {}", output_tokens);
        println!("   Estimated cost: ${:.6}", cost);
    }

    Ok(())
}

fn build_telemetry(config: &AppConfig) -> Arc<TelemetryEngine> {
    let engine = TelemetryEngine::new();
    for budget_cfg in &config.telemetry.budgets {
        let scope = match budget_cfg.scope.as_str() {
            "per_request" => rustedclaw_telemetry::BudgetScope::PerRequest,
            "per_session" => rustedclaw_telemetry::BudgetScope::PerSession,
            "daily" => rustedclaw_telemetry::BudgetScope::Daily,
            "monthly" => rustedclaw_telemetry::BudgetScope::Monthly,
            _ => rustedclaw_telemetry::BudgetScope::Total,
        };
        let action = match budget_cfg.on_exceed.as_str() {
            "warn" => rustedclaw_telemetry::BudgetAction::Warn,
            _ => rustedclaw_telemetry::BudgetAction::Deny,
        };
        engine.add_budget(rustedclaw_telemetry::Budget {
            scope,
            max_usd: budget_cfg.max_usd,
            max_tokens: budget_cfg.max_tokens,
            on_exceed: action,
        });
    }
    Arc::new(engine)
}
