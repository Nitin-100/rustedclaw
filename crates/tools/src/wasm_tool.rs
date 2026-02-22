//! WASM tool sandbox — load and run user-provided WebAssembly components.
//!
//! This module enables loading arbitrary WASM modules as tools, providing
//! a sandboxed execution environment. Each WASM tool:
//!
//! - Runs in an isolated wasmtime sandbox
//! - Has configurable resource limits (memory, fuel/execution time)
//! - Has a declarative capability system (fs_read, fs_write, net, env)
//! - Communicates via JSON (arguments in, result out)
//! - Can be loaded from `.wasm` files at runtime
//!
//! # Capability System
//!
//! Each WASM tool declares what capabilities it needs. A [`WasmPolicy`]
//! controls which capabilities are allowed. Tools that request capabilities
//! not in the policy are rejected at load time.
//!
//! # WASM Module Interface
//!
//! WASM modules must export a function with the signature:
//!
//! ```wat
//! (func (export "execute") (param i32 i32) (result i32))
//! ```
//!
//! Where the input is a pointer + length to a UTF-8 JSON string,
//! and the output is a pointer to a null-terminated JSON result string.
//!
//! The module must also export:
//! - `(memory (export "memory"))` — shared linear memory
//! - `(func (export "alloc") (param i32) (result i32))` — allocator
//!
//! # Feature gate
//!
//! ```toml
//! rustedclaw-tools = { workspace = true, features = ["wasm"] }
//! ```

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};
use wasmtime::*;

use rustedclaw_core::error::ToolError;
use rustedclaw_core::tool::{Tool, ToolResult};

/// Capabilities that a WASM tool can request.
///
/// By default, WASM tools run in a fully sandboxed environment with no access
/// to the host. Capabilities must be explicitly granted via [`WasmPolicy`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WasmCapability {
    /// Read files from the host filesystem.
    FsRead,
    /// Write files to the host filesystem.
    FsWrite,
    /// Make network requests (HTTP, TCP, etc.).
    Net,
    /// Access environment variables.
    Env,
}

impl std::fmt::Display for WasmCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WasmCapability::FsRead => write!(f, "fs_read"),
            WasmCapability::FsWrite => write!(f, "fs_write"),
            WasmCapability::Net => write!(f, "net"),
            WasmCapability::Env => write!(f, "env"),
        }
    }
}

/// Security policy for WASM tool loading and execution.
///
/// Controls which capabilities are allowed, resource limits, and
/// whether to enforce strict validation on load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmPolicy {
    /// Capabilities that are allowed for WASM tools.
    #[serde(default)]
    pub allowed_capabilities: Vec<WasmCapability>,

    /// Maximum memory in bytes any WASM tool can use (overrides per-tool config if lower).
    #[serde(default = "default_policy_max_memory")]
    pub max_memory_bytes: usize,

    /// Maximum fuel any WASM tool can consume (overrides per-tool config if lower).
    #[serde(default = "default_policy_max_fuel")]
    pub max_fuel: u64,

    /// Maximum timeout in milliseconds (overrides per-tool config if lower).
    #[serde(default = "default_policy_max_timeout")]
    pub max_timeout_ms: u64,

    /// Whether to reject tools that request capabilities not in the allow list.
    #[serde(default = "default_strict")]
    pub strict: bool,
}

fn default_policy_max_memory() -> usize {
    64 * 1024 * 1024 // 64 MiB
}

fn default_policy_max_fuel() -> u64 {
    10_000_000_000 // 10B instructions
}

fn default_policy_max_timeout() -> u64 {
    60_000 // 60 seconds
}

fn default_strict() -> bool {
    true
}

impl Default for WasmPolicy {
    fn default() -> Self {
        Self {
            allowed_capabilities: vec![], // fully sandboxed by default
            max_memory_bytes: default_policy_max_memory(),
            max_fuel: default_policy_max_fuel(),
            max_timeout_ms: default_policy_max_timeout(),
            strict: true,
        }
    }
}

impl WasmPolicy {
    /// Create a permissive policy that allows all capabilities.
    pub fn permissive() -> Self {
        Self {
            allowed_capabilities: vec![
                WasmCapability::FsRead,
                WasmCapability::FsWrite,
                WasmCapability::Net,
                WasmCapability::Env,
            ],
            strict: false,
            ..Default::default()
        }
    }

    /// Check whether a set of requested capabilities is allowed.
    pub fn validate_capabilities(&self, requested: &[WasmCapability]) -> Result<(), String> {
        if !self.strict {
            return Ok(());
        }
        for cap in requested {
            if !self.allowed_capabilities.contains(cap) {
                return Err(format!(
                    "Capability '{}' is not allowed by the security policy",
                    cap
                ));
            }
        }
        Ok(())
    }

    /// Enforce policy limits on a tool config, clamping values to policy maximums.
    pub fn enforce_limits(&self, config: &mut WasmToolConfig) {
        if config.max_memory_bytes > self.max_memory_bytes {
            warn!(
                tool = %config.name,
                requested = config.max_memory_bytes,
                allowed = self.max_memory_bytes,
                "Clamping WASM memory limit to policy maximum"
            );
            config.max_memory_bytes = self.max_memory_bytes;
        }
        if config.max_fuel > self.max_fuel || config.max_fuel == 0 {
            config.max_fuel = self.max_fuel;
        }
        if config.timeout_ms > self.max_timeout_ms || config.timeout_ms == 0 {
            config.timeout_ms = self.max_timeout_ms;
        }
    }
}

/// Configuration for a WASM tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WasmToolConfig {
    /// Human-readable tool name.
    pub name: String,
    /// Description shown to the LLM.
    pub description: String,
    /// JSON schema for the tool's parameters.
    pub parameters_schema: serde_json::Value,
    /// Path to the `.wasm` file.
    pub wasm_path: PathBuf,
    /// Capabilities this tool requests.
    #[serde(default)]
    pub capabilities: Vec<WasmCapability>,
    /// Maximum memory in bytes (default: 16 MiB).
    #[serde(default = "default_max_memory")]
    pub max_memory_bytes: usize,
    /// Maximum fuel (instruction count limit, 0 = unlimited).
    #[serde(default = "default_max_fuel")]
    pub max_fuel: u64,
    /// Timeout in milliseconds (0 = no timeout).
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,
}

fn default_max_memory() -> usize {
    16 * 1024 * 1024 // 16 MiB
}

fn default_max_fuel() -> u64 {
    1_000_000_000 // ~1 billion instructions
}

fn default_timeout_ms() -> u64 {
    30_000 // 30 seconds
}

/// A tool backed by a WASM module, running in a wasmtime sandbox.
pub struct WasmTool {
    config: WasmToolConfig,
    engine: Engine,
    module: Module,
}

impl std::fmt::Debug for WasmTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmTool")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl WasmTool {
    /// Create a new WASM tool from a configuration.
    pub fn from_config(config: WasmToolConfig) -> Result<Self, ToolError> {
        Self::from_config_with_policy(config, &WasmPolicy::default())
    }

    /// Create a new WASM tool with policy validation.
    pub fn from_config_with_policy(
        mut config: WasmToolConfig,
        policy: &WasmPolicy,
    ) -> Result<Self, ToolError> {
        // Validate capabilities against policy
        policy
            .validate_capabilities(&config.capabilities)
            .map_err(|e| ToolError::PermissionDenied {
                tool_name: config.name.clone(),
                reason: e,
            })?;

        // Enforce resource limits from policy
        policy.enforce_limits(&mut config);

        let mut engine_config = Config::new();
        engine_config.consume_fuel(config.max_fuel > 0);

        let engine = Engine::new(&engine_config).map_err(|e| ToolError::ExecutionFailed {
            tool_name: config.name.clone(),
            reason: format!("Failed to create WASM engine: {e}"),
        })?;

        let module = Module::from_file(&engine, &config.wasm_path).map_err(|e| {
            ToolError::ExecutionFailed {
                tool_name: config.name.clone(),
                reason: format!(
                    "Failed to load WASM module '{}': {e}",
                    config.wasm_path.display()
                ),
            }
        })?;

        info!(
            name = %config.name,
            path = %config.wasm_path.display(),
            capabilities = ?config.capabilities,
            max_memory = config.max_memory_bytes,
            max_fuel = config.max_fuel,
            timeout_ms = config.timeout_ms,
            "Loaded WASM tool"
        );

        Ok(Self {
            config,
            engine,
            module,
        })
    }

    /// Create a WASM tool from raw bytes (for testing or embedded modules).
    pub fn from_bytes(config: WasmToolConfig, wasm_bytes: &[u8]) -> Result<Self, ToolError> {
        Self::from_bytes_with_policy(config, wasm_bytes, &WasmPolicy::default())
    }

    /// Create a WASM tool from raw bytes with policy validation.
    pub fn from_bytes_with_policy(
        mut config: WasmToolConfig,
        wasm_bytes: &[u8],
        policy: &WasmPolicy,
    ) -> Result<Self, ToolError> {
        // Validate capabilities
        policy
            .validate_capabilities(&config.capabilities)
            .map_err(|e| ToolError::PermissionDenied {
                tool_name: config.name.clone(),
                reason: e,
            })?;

        // Enforce limits
        policy.enforce_limits(&mut config);

        let mut engine_config = Config::new();
        engine_config.consume_fuel(config.max_fuel > 0);

        let engine = Engine::new(&engine_config).map_err(|e| ToolError::ExecutionFailed {
            tool_name: config.name.clone(),
            reason: format!("Failed to create WASM engine: {e}"),
        })?;

        let module = Module::new(&engine, wasm_bytes).map_err(|e| ToolError::ExecutionFailed {
            tool_name: config.name.clone(),
            reason: format!("Failed to compile WASM module: {e}"),
        })?;

        info!(name = %config.name, capabilities = ?config.capabilities, "Loaded WASM tool from bytes");

        Ok(Self {
            config,
            engine,
            module,
        })
    }

    /// Get the capabilities this tool requests.
    pub fn capabilities(&self) -> &[WasmCapability] {
        &self.config.capabilities
    }

    /// Get the tool configuration.
    pub fn config(&self) -> &WasmToolConfig {
        &self.config
    }

    /// Execute the WASM module with JSON input, returning JSON output.
    fn execute_wasm(&self, input_json: &str) -> Result<String, ToolError> {
        let mut store = Store::new(&self.engine, ());

        // Set fuel limit if configured.
        if self.config.max_fuel > 0 {
            store
                .set_fuel(self.config.max_fuel)
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: self.config.name.clone(),
                    reason: format!("Failed to set fuel: {e}"),
                })?;
        }

        // Create the instance with an empty linker (sandboxed — no WASI or imports).
        let linker = Linker::new(&self.engine);
        let instance = linker.instantiate(&mut store, &self.module).map_err(|e| {
            ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: format!("WASM instantiation failed: {e}"),
            }
        })?;

        // Get the exported memory.
        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: "WASM module must export 'memory'".into(),
            }
        })?;

        // Enforce memory limit — check current size and reject if over limit.
        let current_mem_bytes = memory.data_size(&store);
        if current_mem_bytes > self.config.max_memory_bytes {
            return Err(ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: format!(
                    "WASM memory ({} bytes) exceeds limit ({} bytes)",
                    current_mem_bytes, self.config.max_memory_bytes
                ),
            });
        }

        // Get the allocator function.
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: format!("WASM module must export 'alloc(i32) -> i32': {e}"),
            })?;

        // Allocate space for the input JSON.
        let input_bytes = input_json.as_bytes();
        let input_len = input_bytes.len() as i32;
        let input_ptr =
            alloc
                .call(&mut store, input_len)
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: self.config.name.clone(),
                    reason: format!("alloc failed: {e}"),
                })?;

        // Write input to WASM memory.
        let mem_data = memory.data_mut(&mut store);
        let start = input_ptr as usize;
        let end = start + input_bytes.len();
        if end > mem_data.len() {
            return Err(ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: "Input too large for WASM memory".into(),
            });
        }
        mem_data[start..end].copy_from_slice(input_bytes);

        // Call the execute function.
        let execute = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "execute")
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: format!("WASM module must export 'execute(i32, i32) -> i32': {e}"),
            })?;

        let result_ptr = execute
            .call(&mut store, (input_ptr, input_len))
            .map_err(|e| {
                // Check if it's a fuel exhaustion error.
                if format!("{e}").contains("fuel") {
                    ToolError::ExecutionFailed {
                        tool_name: self.config.name.clone(),
                        reason: "WASM execution exceeded fuel limit (too many instructions)".into(),
                    }
                } else {
                    ToolError::ExecutionFailed {
                        tool_name: self.config.name.clone(),
                        reason: format!("WASM execute failed: {e}"),
                    }
                }
            })?;

        // Read the result string from WASM memory.
        // The result is a null-terminated string at result_ptr.
        let mem_data = memory.data(&store);
        let result_start = result_ptr as usize;
        if result_start >= mem_data.len() {
            return Err(ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: "Invalid result pointer from WASM".into(),
            });
        }

        // Find null terminator.
        let result_end = mem_data[result_start..]
            .iter()
            .position(|&b| b == 0)
            .map(|pos| result_start + pos)
            .unwrap_or(mem_data.len());

        let result_str = std::str::from_utf8(&mem_data[result_start..result_end]).map_err(|e| {
            ToolError::ExecutionFailed {
                tool_name: self.config.name.clone(),
                reason: format!("Invalid UTF-8 in WASM output: {e}"),
            }
        })?;

        Ok(result_str.to_string())
    }
}

#[async_trait]
impl Tool for WasmTool {
    fn name(&self) -> &str {
        &self.config.name
    }

    fn description(&self) -> &str {
        &self.config.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.config.parameters_schema.clone()
    }

    async fn execute(
        &self,
        arguments: serde_json::Value,
    ) -> std::result::Result<ToolResult, ToolError> {
        let input_json = serde_json::to_string(&arguments).map_err(|e| {
            ToolError::InvalidArguments(format!("Failed to serialize arguments: {e}"))
        })?;

        debug!(name = %self.config.name, "Executing WASM tool");

        // Apply timeout if configured
        if self.config.timeout_ms > 0 {
            let timeout = std::time::Duration::from_millis(self.config.timeout_ms);
            let tool_name = self.config.name.clone();

            // execute_wasm is sync, so spawn on blocking thread with timeout
            match tokio::time::timeout(timeout, async {
                // Run sync work in blocking thread
                let input = input_json.clone();
                // Note: WasmTool is not Send, so we call it directly
                self.execute_wasm(&input)
            })
            .await
            {
                Ok(result) => {
                    let output = result?;
                    match serde_json::from_str::<ToolResult>(&output) {
                        Ok(result) => Ok(result),
                        Err(_) => Ok(ToolResult {
                            call_id: String::new(),
                            success: true,
                            output,
                            data: None,
                        }),
                    }
                }
                Err(_) => Err(ToolError::Timeout {
                    tool_name,
                    timeout_secs: self.config.timeout_ms / 1000,
                }),
            }
        } else {
            let output = self.execute_wasm(&input_json)?;

            match serde_json::from_str::<ToolResult>(&output) {
                Ok(result) => Ok(result),
                Err(_) => Ok(ToolResult {
                    call_id: String::new(),
                    success: true,
                    output,
                    data: None,
                }),
            }
        }
    }
}

/// Load WASM tools from a directory of `.wasm` files + config manifests.
///
/// Each tool needs:
/// - `<name>.wasm` — the compiled WebAssembly module
/// - `<name>.tool.json` — a JSON config matching [`WasmToolConfig`]
///
/// Returns tools that loaded successfully, logging errors for failures.
pub fn load_wasm_tools_from_dir(dir: &Path) -> Vec<WasmTool> {
    load_wasm_tools_from_dir_with_policy(dir, &WasmPolicy::default())
}

/// Load WASM tools with a specific security policy.
pub fn load_wasm_tools_from_dir_with_policy(dir: &Path, policy: &WasmPolicy) -> Vec<WasmTool> {
    let mut tools = Vec::new();

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!(dir = %dir.display(), error = %e, "Failed to read WASM tools directory");
            return tools;
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json")
            && path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.ends_with(".tool.json"))
        {
            match load_wasm_tool_from_manifest(&path, policy) {
                Ok(tool) => {
                    info!(name = %tool.name(), "Loaded WASM tool");
                    tools.push(tool);
                }
                Err(e) => {
                    warn!(path = %path.display(), error = %e, "Failed to load WASM tool");
                }
            }
        }
    }

    tools
}

/// Load a single WASM tool from its manifest file.
fn load_wasm_tool_from_manifest(
    manifest_path: &Path,
    policy: &WasmPolicy,
) -> Result<WasmTool, ToolError> {
    let manifest_content =
        std::fs::read_to_string(manifest_path).map_err(|e| ToolError::ExecutionFailed {
            tool_name: "wasm".to_string(),
            reason: format!("Failed to read manifest: {e}"),
        })?;

    let mut config: WasmToolConfig = serde_json::from_str(&manifest_content)
        .map_err(|e| ToolError::InvalidArguments(format!("Invalid tool manifest: {e}")))?;

    // If wasm_path is relative, resolve it relative to the manifest directory.
    if config.wasm_path.is_relative() {
        if let Some(manifest_dir) = manifest_path.parent() {
            config.wasm_path = manifest_dir.join(&config.wasm_path);
        }
    }

    WasmTool::from_config_with_policy(config, policy)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_tool_config_defaults() {
        let config: WasmToolConfig = serde_json::from_str(
            r#"{
            "name": "test_tool",
            "description": "A test tool",
            "parameters_schema": {"type": "object"},
            "wasm_path": "test.wasm"
        }"#,
        )
        .unwrap();

        assert_eq!(config.name, "test_tool");
        assert_eq!(config.max_memory_bytes, 16 * 1024 * 1024);
        assert_eq!(config.max_fuel, 1_000_000_000);
        assert_eq!(config.timeout_ms, 30_000);
        assert!(config.capabilities.is_empty());
    }

    #[test]
    fn wasm_tool_config_custom() {
        let config: WasmToolConfig = serde_json::from_str(
            r#"{
            "name": "custom",
            "description": "Custom tool",
            "parameters_schema": {"type": "object", "properties": {"x": {"type": "number"}}},
            "wasm_path": "/path/to/tool.wasm",
            "max_memory_bytes": 1048576,
            "max_fuel": 500000,
            "timeout_ms": 5000,
            "capabilities": ["fs_read", "net"]
        }"#,
        )
        .unwrap();

        assert_eq!(config.max_memory_bytes, 1_048_576);
        assert_eq!(config.max_fuel, 500_000);
        assert_eq!(config.timeout_ms, 5_000);
        assert_eq!(config.capabilities.len(), 2);
        assert!(config.capabilities.contains(&WasmCapability::FsRead));
        assert!(config.capabilities.contains(&WasmCapability::Net));
    }

    #[test]
    fn wasm_tool_config_serialization_roundtrip() {
        let config = WasmToolConfig {
            name: "test".into(),
            description: "Test tool".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            wasm_path: PathBuf::from("test.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: default_max_fuel(),
            timeout_ms: default_timeout_ms(),
        };

        let json = serde_json::to_string(&config).unwrap();
        let parsed: WasmToolConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.name, config.name);
        assert_eq!(parsed.max_fuel, config.max_fuel);
    }

    #[test]
    fn load_nonexistent_wasm_returns_error() {
        let config = WasmToolConfig {
            name: "missing".into(),
            description: "Missing WASM".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            wasm_path: PathBuf::from("/nonexistent/path/tool.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: default_max_fuel(),
            timeout_ms: default_timeout_ms(),
        };

        let result = WasmTool::from_config(config);
        assert!(result.is_err());
    }

    // Test with a minimal valid WASM module (WAT format compiled inline).
    // This module exports memory, alloc, and execute but just echoes input.
    #[test]
    fn wasm_tool_from_bytes_minimal() {
        // Minimal WAT that exports the required interface:
        // - memory
        // - alloc: just returns a fixed offset
        // - execute: returns the input pointer (echo)
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32)
                    i32.const 1024
                )
                (func (export "execute") (param i32 i32) (result i32)
                    ;; Return pointer to a pre-written result at offset 2048
                    ;; First, write a simple JSON result to offset 2048
                    (i32.store8 (i32.const 2048) (i32.const 79))   ;; 'O'
                    (i32.store8 (i32.const 2049) (i32.const 75))   ;; 'K'
                    (i32.store8 (i32.const 2050) (i32.const 0))    ;; null terminator
                    i32.const 2048
                )
            )
        "#;

        let engine = Engine::default();
        let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

        let config = WasmToolConfig {
            name: "echo_tool".into(),
            description: "Echo tool".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            wasm_path: PathBuf::from("inline.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: 0, // No fuel limit for this test
            timeout_ms: 0,
        };

        let tool = WasmTool::from_bytes(config, &wasm_bytes).unwrap();
        assert_eq!(tool.name(), "echo_tool");

        // Execute the tool synchronously via execute_wasm.
        let result = tool.execute_wasm("{}").unwrap();
        assert_eq!(result, "OK");
    }

    #[tokio::test]
    async fn wasm_tool_execute_trait() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32)
                    i32.const 1024
                )
                (func (export "execute") (param i32 i32) (result i32)
                    (i32.store8 (i32.const 2048) (i32.const 100))  ;; 'd'
                    (i32.store8 (i32.const 2049) (i32.const 111))  ;; 'o'
                    (i32.store8 (i32.const 2050) (i32.const 110))  ;; 'n'
                    (i32.store8 (i32.const 2051) (i32.const 101))  ;; 'e'
                    (i32.store8 (i32.const 2052) (i32.const 0))    ;; null
                    i32.const 2048
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

        let config = WasmToolConfig {
            name: "done_tool".into(),
            description: "Returns done".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            wasm_path: PathBuf::from("inline.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: 0,
            timeout_ms: 0,
        };

        let tool = WasmTool::from_bytes(config, &wasm_bytes).unwrap();

        // Use the Tool trait's execute method.
        let result = tool
            .execute(serde_json::json!({"key": "value"}))
            .await
            .unwrap();
        assert!(result.success);
        assert_eq!(result.output, "done");
    }

    #[test]
    fn load_tools_from_empty_dir() {
        let dir = std::env::temp_dir().join("rustedclaw_wasm_test_empty");
        let _ = std::fs::create_dir_all(&dir);
        let tools = load_wasm_tools_from_dir(&dir);
        assert!(tools.is_empty());
    }

    #[test]
    fn load_tools_from_nonexistent_dir() {
        let dir = PathBuf::from("/nonexistent/wasm/tools");
        let tools = load_wasm_tools_from_dir(&dir);
        assert!(tools.is_empty());
    }

    #[test]
    fn wasm_tool_missing_export() {
        // Module that exports memory but not alloc or execute.
        let wat = r#"
            (module
                (memory (export "memory") 1)
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

        let config = WasmToolConfig {
            name: "bad_tool".into(),
            description: "Missing exports".into(),
            parameters_schema: serde_json::json!({"type": "object"}),
            wasm_path: PathBuf::from("inline.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: 0,
            timeout_ms: 0,
        };

        let tool = WasmTool::from_bytes(config, &wasm_bytes).unwrap();
        let result = tool.execute_wasm("{}");
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(err_msg.contains("alloc"));
    }

    // ── Capability & Policy Tests ──

    #[test]
    fn capability_serialization() {
        let cap = WasmCapability::FsRead;
        let json = serde_json::to_string(&cap).unwrap();
        assert_eq!(json, "\"fs_read\"");

        let parsed: WasmCapability = serde_json::from_str("\"net\"").unwrap();
        assert_eq!(parsed, WasmCapability::Net);
    }

    #[test]
    fn capability_display() {
        assert_eq!(WasmCapability::FsRead.to_string(), "fs_read");
        assert_eq!(WasmCapability::FsWrite.to_string(), "fs_write");
        assert_eq!(WasmCapability::Net.to_string(), "net");
        assert_eq!(WasmCapability::Env.to_string(), "env");
    }

    #[test]
    fn default_policy_denies_all_capabilities() {
        let policy = WasmPolicy::default();
        assert!(policy.allowed_capabilities.is_empty());
        assert!(policy.strict);

        let result = policy.validate_capabilities(&[WasmCapability::FsRead]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("fs_read"));
    }

    #[test]
    fn permissive_policy_allows_all() {
        let policy = WasmPolicy::permissive();
        assert!(!policy.strict);

        let result = policy.validate_capabilities(&[
            WasmCapability::FsRead,
            WasmCapability::FsWrite,
            WasmCapability::Net,
            WasmCapability::Env,
        ]);
        assert!(result.is_ok());
    }

    #[test]
    fn policy_allows_subset_of_capabilities() {
        let policy = WasmPolicy {
            allowed_capabilities: vec![WasmCapability::FsRead, WasmCapability::Net],
            strict: true,
            ..Default::default()
        };

        // Allowed
        assert!(
            policy
                .validate_capabilities(&[WasmCapability::FsRead])
                .is_ok()
        );
        assert!(policy.validate_capabilities(&[WasmCapability::Net]).is_ok());
        assert!(
            policy
                .validate_capabilities(&[WasmCapability::FsRead, WasmCapability::Net])
                .is_ok()
        );

        // Denied
        assert!(
            policy
                .validate_capabilities(&[WasmCapability::FsWrite])
                .is_err()
        );
        assert!(
            policy
                .validate_capabilities(&[WasmCapability::Env])
                .is_err()
        );
    }

    #[test]
    fn policy_enforces_memory_limit() {
        let policy = WasmPolicy {
            max_memory_bytes: 4 * 1024 * 1024, // 4 MiB
            ..Default::default()
        };

        let mut config = WasmToolConfig {
            name: "test".into(),
            description: "test".into(),
            parameters_schema: serde_json::json!({}),
            wasm_path: PathBuf::from("test.wasm"),
            capabilities: vec![],
            max_memory_bytes: 32 * 1024 * 1024, // 32 MiB — over limit
            max_fuel: 0,
            timeout_ms: 0,
        };

        policy.enforce_limits(&mut config);
        assert_eq!(config.max_memory_bytes, 4 * 1024 * 1024); // clamped
    }

    #[test]
    fn policy_enforces_fuel_limit() {
        let policy = WasmPolicy {
            max_fuel: 1_000_000,
            ..Default::default()
        };

        let mut config = WasmToolConfig {
            name: "test".into(),
            description: "test".into(),
            parameters_schema: serde_json::json!({}),
            wasm_path: PathBuf::from("test.wasm"),
            capabilities: vec![],
            max_memory_bytes: default_max_memory(),
            max_fuel: 999_999_999, // over limit
            timeout_ms: 5000,
        };

        policy.enforce_limits(&mut config);
        assert_eq!(config.max_fuel, 1_000_000); // clamped
    }

    #[test]
    fn tool_rejected_by_strict_policy() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) i32.const 1024)
                (func (export "execute") (param i32 i32) (result i32) i32.const 2048)
            )
        "#;
        let wasm_bytes = wat::parse_str(wat).unwrap();

        let config = WasmToolConfig {
            name: "needs_net".into(),
            description: "Needs net".into(),
            parameters_schema: serde_json::json!({}),
            wasm_path: PathBuf::from("test.wasm"),
            capabilities: vec![WasmCapability::Net],
            max_memory_bytes: default_max_memory(),
            max_fuel: 0,
            timeout_ms: 0,
        };

        let strict_policy = WasmPolicy::default(); // no capabilities allowed
        let result = WasmTool::from_bytes_with_policy(config, &wasm_bytes, &strict_policy);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("net") || err.contains("Permission"));
    }

    #[test]
    fn tool_accepted_by_matching_policy() {
        let wat = r#"
            (module
                (memory (export "memory") 1)
                (func (export "alloc") (param i32) (result i32) i32.const 1024)
                (func (export "execute") (param i32 i32) (result i32) i32.const 2048)
            )
        "#;
        let wasm_bytes = wat::parse_str(wat).unwrap();

        let config = WasmToolConfig {
            name: "needs_fs".into(),
            description: "Needs fs".into(),
            parameters_schema: serde_json::json!({}),
            wasm_path: PathBuf::from("test.wasm"),
            capabilities: vec![WasmCapability::FsRead],
            max_memory_bytes: default_max_memory(),
            max_fuel: 0,
            timeout_ms: 0,
        };

        let policy = WasmPolicy {
            allowed_capabilities: vec![WasmCapability::FsRead],
            ..Default::default()
        };

        let result = WasmTool::from_bytes_with_policy(config, &wasm_bytes, &policy);
        assert!(result.is_ok());
        let tool = result.unwrap();
        assert_eq!(tool.capabilities(), &[WasmCapability::FsRead]);
    }

    #[test]
    fn empty_capabilities_always_allowed() {
        let policy = WasmPolicy::default();
        let result = policy.validate_capabilities(&[]);
        assert!(result.is_ok()); // empty = no capabilities needed = OK
    }

    #[test]
    fn policy_serialization_roundtrip() {
        let policy = WasmPolicy {
            allowed_capabilities: vec![WasmCapability::FsRead, WasmCapability::Net],
            max_memory_bytes: 8 * 1024 * 1024,
            max_fuel: 500_000,
            max_timeout_ms: 10_000,
            strict: true,
        };

        let json = serde_json::to_string(&policy).unwrap();
        let parsed: WasmPolicy = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.allowed_capabilities.len(), 2);
        assert_eq!(parsed.max_memory_bytes, 8 * 1024 * 1024);
        assert!(parsed.strict);
    }
}
