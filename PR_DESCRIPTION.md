## What this PR adds

### �️ Agent Contracts — Declarative Behavior Guardrails

New crate `crates/contracts/` (13th workspace member) that lets you define rules in TOML that intercept tool calls **before** they execute.

```toml
[[contracts]]
name = "no-rm-rf"
trigger = "tool:shell"
condition = 'args.command CONTAINS "rm -rf"'
action = "deny"
message = "Blocked: rm -rf is forbidden"
```

#### Condition DSL

Full expression language with tokenizer → recursive descent parser → AST:

| Operator | Example |
|---|---|
| `CONTAINS` | `args.command CONTAINS "rm -rf"` |
| `MATCHES` (regex) | `args.url MATCHES "https?://10\\."` |
| `STARTS_WITH` / `ENDS_WITH` | `args.path STARTS_WITH "/etc"` |
| `==`, `!=`, `>`, `<`, `>=`, `<=` | `args.amount > 100` |
| `AND`, `OR`, `NOT` | `args.x CONTAINS "a" AND NOT args.y == "b"` |
| Parentheses | `(A OR B) AND C` |
| Dotted field paths | `args.nested.deep.key CONTAINS "val"` |

#### Actions

| Action | Behavior |
|---|---|
| `deny` | Block the tool call, return error to agent |
| `confirm` | Pause and ask user for approval |
| `warn` | Log warning but allow execution |
| `allow` | Explicit pass (useful in priority chains) |

#### Integration Points

| Layer | What was added |
|---|---|
| **Agent loop** | Pre-execution contract check before every `tools.execute()`. Denied calls emit `ContractViolation` domain event and skip execution. |
| **Gateway startup** | `ContractEngine` built from `config.contracts`, shared via `Arc` between legacy agent and v1 API state |
| **REST API** | `GET /v1/contracts` — list all contracts<br>`POST /v1/contracts` — add contract at runtime<br>`DELETE /v1/contracts/:name` — remove contract |
| **CLI** | `rustedclaw contract list` — list configured contracts<br>`rustedclaw contract validate` — check for errors<br>`rustedclaw contract test <tool> <args>` — simulate a tool call |
| **Config** | `[[contracts]]` TOML array in `~/.rustedclaw/config.toml` |
| **Events** | New `DomainEvent::ContractViolation` variant with contract_name, tool_name, action, message, timestamp |
| **SSE logs** | `contract_violation` event type in `/v1/logs` stream |
| **Status** | `contracts_count` added to `/v1/status` response |

#### Architecture

```
config.toml [[contracts]]
        ↓
  ContractConfig (config crate)
        ↓
  Contract → ContractSet (contracts crate)
        ↓
  ContractEngine (thread-safe, RwLock)
    ├── compiled condition cache
    ├── priority-based evaluation (highest first)
    └── audit log
        ↓
  AgentLoop.contracts: Option<Arc<ContractEngine>>
        ↓
  check_tool_call(name, args) → Verdict { allowed, action, message }
```

### 📝 README Updates

- **Explicit API key setup** — 2 methods (env var or config file), exact file paths for Linux/macOS/Windows
- **Full config.toml reference** — every field commented, provider list, env var override table with priority order
- **Agent Contracts section** — DSL reference, 4 example contracts, CLI usage
- **Updated benchmark numbers** — all from actual `benchmark_lowend.ps1` run on this branch
- **Added contracts to**: feature table, CLI commands, API endpoints, project structure, competition comparison

---

## 📊 Benchmark Results (measured on this branch)

All numbers from `scripts\benchmark_lowend.ps1` on i7-12700F, 32 GB RAM, Windows 11, NVMe.

### Binary Size

| Branch | Size | Delta |
|---|:---:|:---:|
| `master` (before) | 3.94 MB | — |
| `feature/agent-contracts` | **4.06 MB** | **+120 KB** |

> +120 KB from the contracts engine + `regex-lite` crate. Originally +860 KB with full `regex` — switched to `regex-lite` to save 740 KB.

### Cold Start (20 runs)

| Metric | Value |
|---|:---:|
| **Average** | 5.9 ms |
| **P50** | 5 ms |
| **P99** | 9 ms |
| **Min / Max** | 5 ms / 14 ms |

### Native Performance

| Metric | Value |
|---|:---:|
| **Idle RAM** | 6.63 MB |
| **After 1K requests** | 6.88 MB |
| **After 6K requests** | 6.95 MB |
| **RAM growth** | 0.32 MB |
| **Throughput (sequential)** | 4,098 req/s |
| **Threads** | 6 |

### Docker Tiers

| Metric | Pi (1 CPU, 256 MB) | $5 VPS (1 CPU, 512 MB) | $10 VPS (2 CPU, 1 GB) |
|---|:---:|:---:|:---:|
| **Idle RAM** | 1.07 MiB | 1.08 MiB | 1.07 MiB |
| **Post-load RAM** | 1.10 MiB | 1.09 MiB | 1.11 MiB |
| **Peak RAM** | 1.25 MiB | 1.25 MiB | 1.25 MiB |
| **Throughput (seq)** | 1,838 req/s | 1,779 req/s | 1,916 req/s |
| **Throughput (5× parallel)** | 888 req/s | 1,002 req/s | 1,018 req/s |

### Tests

| Branch | Tests | Delta |
|---|:---:|:---:|
| `master` | 407 | — |
| `feature/agent-contracts` | **440** | **+33** |

Per-crate breakdown:
```
cli ............   6 tests    (unchanged)
e2e ............  17 tests    (unchanged)
agent ..........  62 tests    (unchanged)
channels .......  38 tests    (unchanged)
config .........   9 tests    (unchanged)
contracts ......  33 tests    ← NEW
core ...........  29 tests    (unchanged)
gateway ........  32 tests    (unchanged)
memory .........  49 tests    (unchanged)
providers ......  42 tests    (unchanged)
security .......  40 tests    (unchanged)
tools ..........  67 tests    (unchanged)
workflow .......  16 tests    (unchanged)
─────────────────────────────
TOTAL            440 tests    0 failures
```

---

## Files Changed (19 files, +2,115 lines)

### New files
| File | Lines | Purpose |
|---|:---:|---|
| `crates/contracts/Cargo.toml` | 20 | Crate manifest (deps: serde, regex-lite, chrono, tracing) |
| `crates/contracts/src/lib.rs` | 35 | Module root, `ContractError` enum, re-exports |
| `crates/contracts/src/model.rs` | 337 | `Contract`, `ContractSet`, `Trigger`, `Action` + 7 tests |
| `crates/contracts/src/parser.rs` | 590 | Condition DSL tokenizer + recursive descent parser + evaluator + 12 tests |
| `crates/contracts/src/engine.rs` | 531 | `ContractEngine` (thread-safe), `Verdict`, audit log + 14 tests |
| `crates/cli/src/commands/contract.rs` | 130 | CLI `contract list/validate/test` commands |

### Modified files
| File | What changed |
|---|---|
| `Cargo.toml` | Added `crates/contracts` to workspace members + deps |
| `crates/agent/Cargo.toml` | Added `rustedclaw-contracts` dep |
| `crates/agent/src/loop_runner.rs` | Added `contracts` field, `with_contracts()` builder, pre-execution check |
| `crates/cli/Cargo.toml` | Added `rustedclaw-contracts` dep |
| `crates/cli/src/commands/mod.rs` | Added `pub mod contract` |
| `crates/cli/src/main.rs` | Added `Contract` subcommand + `ContractAction` enum + match arm |
| `crates/config/src/lib.rs` | Added `ContractConfig` struct + `contracts` field on `AppConfig` |
| `crates/core/src/event.rs` | Added `ContractViolation` variant to `DomainEvent` |
| `crates/gateway/Cargo.toml` | Added `rustedclaw-contracts` dep |
| `crates/gateway/src/api_v1.rs` | Added `contracts` to `ApiV1State`, 3 REST handlers, `contract_violation` SSE event |
| `crates/gateway/src/lib.rs` | Build `ContractEngine` from config at startup, wire to agent + API state |
| `README.md` | Updated numbers, added contracts section, explicit API key instructions |
| `Cargo.lock` | Added `regex-lite` + `regex-syntax` (no `regex` — size-optimized) |

---

> **Merge criteria:** All 440 tests pass ✅, binary stayed under 4.1 MB, no RAM regression, all benchmark numbers verified on actual hardware.
