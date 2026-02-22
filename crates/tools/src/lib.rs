//! Built-in tool implementations for RustedClaw.
//!
//! Tools give the agent the ability to interact with the world:
//! run shell commands, read/write files, search the web, do math,
//! check the weather, and query a knowledge base.
//!
//! With the `wasm` feature, user-provided WASM modules can also be
//! loaded as sandboxed tools.

pub mod calculator;
pub mod file_read;
pub mod file_write;
pub mod http_request;
pub mod knowledge_base_query;
pub mod memory_search;
pub mod shell;
pub mod weather_lookup;
pub mod web_search;

#[cfg(feature = "wasm")]
pub mod wasm_tool;

use rustedclaw_core::tool::ToolRegistry;

#[cfg(feature = "wasm")]
pub use wasm_tool::{
    WasmCapability, WasmPolicy, WasmTool, WasmToolConfig, load_wasm_tools_from_dir,
};

/// Create a default tool registry with all built-in tools.
pub fn default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(shell::ShellTool::new(vec![])));
    registry.register(Box::new(file_read::FileReadTool::new()));
    registry.register(Box::new(file_write::FileWriteTool::new()));
    registry.register(Box::new(web_search::WebSearchTool));
    registry.register(Box::new(calculator::CalculatorTool));
    registry.register(Box::new(weather_lookup::WeatherLookupTool));
    registry.register(Box::new(knowledge_base_query::KnowledgeBaseQueryTool));
    registry.register(Box::new(http_request::HttpRequestTool));
    registry.register(Box::new(memory_search::MemorySearchTool::new()));
    registry
}
