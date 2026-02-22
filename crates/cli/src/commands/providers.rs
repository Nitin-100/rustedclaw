//! `rustedclaw providers` — List supported LLM providers.

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("🤖 Supported LLM Providers");
    println!("==========================");
    println!();
    println!("  Built-in providers:");
    println!("  ┌──────────────────┬──────────────────────────────┬──────────────┐");
    println!("  │ Provider         │ Base URL                     │ Auth         │");
    println!("  ├──────────────────┼──────────────────────────────┼──────────────┤");
    println!("  │ openrouter       │ openrouter.ai/api/v1         │ API key      │");
    println!("  │ openai           │ api.openai.com/v1            │ API key      │");
    println!("  │ anthropic        │ api.anthropic.com/v1         │ API key      │");
    println!("  │ ollama           │ localhost:11434/v1            │ None (local) │");
    println!("  │ groq             │ api.groq.com/openai/v1       │ API key      │");
    println!("  │ deepseek         │ api.deepseek.com/v1          │ API key      │");
    println!("  │ together         │ api.together.xyz/v1          │ API key      │");
    println!("  │ fireworks        │ api.fireworks.ai/inference/v1 │ API key      │");
    println!("  │ mistral          │ api.mistral.ai/v1            │ API key      │");
    println!("  │ xai              │ api.x.ai/v1                  │ API key      │");
    println!("  │ perplexity       │ api.perplexity.ai            │ API key      │");
    println!("  └──────────────────┴──────────────────────────────┴──────────────┘");
    println!();
    println!("  Custom endpoints:");
    println!("    Any OpenAI-compatible API works out of the box:");
    println!("    default_provider = \"openai\"");
    println!("    [providers.openai]");
    println!("    api_url = \"https://your-custom-endpoint.com/v1\"");
    println!("    api_key = \"your-key\"");
    println!();
    println!("  Environment variables:");
    println!("    OPENAI_API_KEY, OPENROUTER_API_KEY, RUSTEDCLAW_API_KEY");
    println!("    RUSTEDCLAW_PROVIDER, RUSTEDCLAW_MODEL");

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn provider_list_compiles() {}
}
