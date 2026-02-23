## What this PR adds

### 💰 Telemetry — Cost Tracking, Execution Tracing & Budget Enforcement

New crate `crates/telemetry/` (14th workspace member) that adds real-time cost tracking, span-based execution tracing, and budget enforcement to prevent runaway API spend.

```toml
[telemetry]
enabled = true

[[telemetry.budgets]]
scope = "daily"
max_usd = 5.00
on_exceed = "deny"

[[telemetry.budgets]]
scope = "per_request"
max_usd = 0.50
on_exceed = "warn"
```

#### Built-in Pricing for 20+ Models

| Provider | Models |
|---|---|
| **Anthropic** | claude-sonnet-4 ($3/$15), claude-opus-4 ($15/$75), 3.5-sonnet, 3.5-haiku, 3-haiku |
| **OpenAI** | gpt-4o ($2.50/$10), gpt-4o-mini ($0.15/$0.60), gpt-4-turbo, o1, o1-mini, o3-mini |
| **Google** | gemini-2.0-flash, gemini-2.0-pro, gemini-1.5-pro, gemini-1.5-flash |
| **Meta** | llama-3.1-405b, llama-3.1-70b, llama-3.1-8b |
| **Mistral** | mistral-large, mistral-small, codestral |
| **DeepSeek** | deepseek-v3, deepseek-r1 |

Custom pricing overrides via config:
```toml
[telemetry.custom_pricing."my-provider/my-model"]
input_per_m = 1.50
output_per_m = 4.00
```

#### Budget Scopes & Actions

| Scope | Tracks |
|---|---|
| `per_request` | Single agent loop execution |
| `per_session` | All requests in current session |
| `daily` | Rolling 24-hour window |
| `monthly` | Rolling 30-day window |
| `total` | All-time cumulative spend |

| Action | Behavior |
|---|---|
| `deny` | Block the LLM call, return 429 error to agent |
| `warn` | Log warning but allow execution |

#### Execution Tracing

Every agent loop execution produces a `Trace` with individual `Span` records:

| SpanKind | What it records |
|---|---|
| `LlmCall` | Model, input/output tokens, cost in USD, latency |
| `ToolExecution` | Tool name, success/failure, duration |
| `MemoryOp` | Memory read/write operations |
| `ContractCheck` | Contract evaluations |
| `Turn` | Full conversation turn |

#### Integration Points

| Layer | What was added |
|---|---|
| **Agent loop** | Budget pre-check before every LLM call. LLM span recording (tokens + cost). Tool execution span recording. Trace lifecycle (start/end). |
| **Gateway startup** | `TelemetryEngine` built from `config.telemetry`, shared via `Arc` between agent and v1 API state |
| **REST API** | `GET /v1/usage` — cost/token snapshot<br>`GET /v1/traces` — list recent traces<br>`GET /v1/traces/:id` — full trace with spans<br>`GET /v1/budgets` — list budgets<br>`POST /v1/budgets` — add budget at runtime<br>`DELETE /v1/budgets/:scope` — remove budget |
| **CLI** | `rustedclaw usage show` — current spend snapshot<br>`rustedclaw usage pricing` — list all model pricing<br>`rustedclaw usage budgets` — show configured budgets<br>`rustedclaw usage estimate <model> <in> <out>` — estimate cost |
| **Config** | `[telemetry]` section in `~/.rustedclaw/config.toml` |
| **Events** | New `DomainEvent::BudgetExceeded` variant with scope, spent_usd, limit_usd, action, timestamp |
| **SSE logs** | `budget_exceeded` event type in `/v1/logs` stream |
| **Status** | `session_cost_usd` and `trace_count` added to `/v1/status` response |

#### Architecture

```
config.toml [telemetry]
        ↓
  TelemetryConfig (config crate)
        ↓
  PricingTable (20+ built-in models)
        ↓
  TelemetryEngine (thread-safe, RwLock)
    ├── pricing table (built-in + custom overrides)
    ├── budget management (add/remove/check)
    ├── running totals (daily/monthly rollover)
    ├── trace storage + span recording
    └── cost summaries + usage snapshots
        ↓
  AgentLoop.telemetry: Option<Arc<TelemetryEngine>>
    ├── start_trace() → before main loop
    ├── check_budget() → before every LLM call
    ├── record_span(LlmCall) → after LLM response
    ├── record_span(ToolExecution) → after tool exec
    └── end_trace() → on all exit paths
```

### 📝 README Updates

- **Cost Tracking & Budgets section** — pricing tables, budget examples, CLI usage, API examples
- **Updated benchmark numbers** — all from actual `benchmark_lowend.ps1` run on this branch
- **Added telemetry to**: feature table, CLI commands, API endpoints, project structure, competition comparison

---

## 📊 Benchmark Results (measured on this branch)

All numbers from `scripts\benchmark_lowend.ps1` on i7-12700F, 32 GB RAM, Windows 11, NVMe.

### Binary Size

| Branch | Size | Delta |
|---|:---:|:---:|
| `master` (with contracts) | 4.06 MB | — |
| `feature/telemetry-cost-tracking` | **4.16 MB** | **+100 KB** |

> +100 KB from the telemetry engine, pricing table, and trace storage. No new external crates — uses existing workspace deps (serde, chrono, uuid).

### Cold Start (20 runs)

| Metric | Value |
|---|:---:|
| **Average** | 12.8 ms |
| **P50** | 5 ms |
| **P99** | 6 ms |
| **Min / Max** | 5 ms / 159 ms |

### Native Performance

| Metric | Value |
|---|:---:|
| **Idle RAM** | 6.71 MB |
| **After 1K requests** | 6.95 MB |
| **After 6K requests** | 7.03 MB |
| **RAM growth** | 0.32 MB |
| **Throughput (sequential)** | 4,049 req/s |
| **Threads** | 6 |

### Docker Tiers

| Metric | Pi (1 CPU, 256 MB) | $5 VPS (1 CPU, 512 MB) | $10 VPS (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|
| **Idle RAM** | 1.09 MiB | 1.10 MiB | 1.09 MiB |
| **Post-load RAM** | 1.13 MiB | 1.11 MiB | 1.12 MiB |
| **Peak RAM** | 1.29 MiB | 1.28 MiB | 1.27 MiB |
| **Throughput (seq)** | 1,799 req/s | 1,786 req/s | 1,754 req/s |
| **Throughput (5× parallel)** | 786 req/s | 973 req/s | 977 req/s |

### Tests

| Branch | Tests | Delta |
|---|:---:|:---:|
| `master` (with contracts) | 440 | — |
| `feature/telemetry-cost-tracking` | **469** | **+29** |

Per-crate breakdown:
```
cli ............   6 tests    (unchanged)
e2e ............  17 tests    (unchanged)
agent ..........  62 tests    (unchanged)
channels .......  38 tests    (unchanged)
config .........   9 tests    (unchanged)
contracts ......  33 tests    (unchanged)
core ...........  29 tests    (unchanged)
gateway ........  32 tests    (unchanged)
memory .........  49 tests    (unchanged)
providers ......  42 tests    (unchanged)
security .......  40 tests    (unchanged)
telemetry ......  29 tests    ← NEW
tools ..........  67 tests    (unchanged)
workflow .......  16 tests    (unchanged)
─────────────────────────────
TOTAL            469 tests    0 failures
```

---

## Files Changed (20 files, +1,792 lines)

### New files
| File | Lines | Purpose |
|---|:---:|---|
| `crates/telemetry/Cargo.toml` | 18 | Crate manifest (deps: serde, serde_json, thiserror, tracing, chrono, uuid) |
| `crates/telemetry/src/lib.rs` | 23 | Module root, `TelemetryError` enum, re-exports |
| `crates/telemetry/src/model.rs` | 369 | `Span`, `SpanKind`, `Trace`, `Budget`, `BudgetScope`, `BudgetAction`, `CostSummary`, `UsageSnapshot` + 8 tests |
| `crates/telemetry/src/pricing.rs` | 206 | `PricingTable` with built-in pricing for 20+ models, `ModelPricing`, `compute_cost()` + 7 tests |
| `crates/telemetry/src/engine.rs` | 564 | `TelemetryEngine` (thread-safe), budget checking, trace management, cost summaries, pruning + 14 tests |
| `crates/cli/src/commands/usage.rs` | 128 | CLI `usage show/pricing/budgets/estimate` commands |

### Modified files
| File | What changed |
|---|---|
| `Cargo.toml` | Added `crates/telemetry` to workspace members + deps |
| `Cargo.lock` | Updated dependency graph |
| `crates/agent/Cargo.toml` | Added `rustedclaw-telemetry` dep |
| `crates/agent/src/loop_runner.rs` | Added `telemetry` field, `with_telemetry()` builder, budget pre-check, LLM + tool span recording, trace lifecycle |
| `crates/cli/Cargo.toml` | Added `rustedclaw-telemetry` dep |
| `crates/cli/src/commands/mod.rs` | Added `pub mod usage` |
| `crates/cli/src/main.rs` | Added `Usage` subcommand + `UsageAction` enum + match arm |
| `crates/config/src/lib.rs` | Added `TelemetryConfig`, `BudgetConfig`, `PricingOverrideConfig` structs + `telemetry` field on `AppConfig` |
| `crates/core/src/event.rs` | Added `BudgetExceeded` variant to `DomainEvent` |
| `crates/gateway/Cargo.toml` | Added `rustedclaw-telemetry` dep |
| `crates/gateway/src/api_v1.rs` | Added `telemetry` to `ApiV1State`, 6 REST handlers, `budget_exceeded` SSE event, `session_cost_usd` + `trace_count` in status |
| `crates/gateway/src/lib.rs` | Build `TelemetryEngine` from config at startup, wire to agent + API state |
| `README.md` | Updated benchmark numbers, added cost tracking section, updated competition table |

---

> **Merge criteria:** All 469 tests pass ✅, binary stayed under 4.2 MB (+100 KB), no RAM regression, all benchmark numbers verified on actual hardware.
