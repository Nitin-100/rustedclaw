//! `rustedclaw daemon` — Full autonomous runtime.

use tracing::info;

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config =
        rustedclaw_config::AppConfig::load().map_err(|e| format!("Failed to load config: {e}"))?;

    println!("🦀 RustedClaw Daemon — Starting full runtime");
    println!(
        "   Gateway: {}:{}",
        config.gateway.host, config.gateway.port
    );
    println!(
        "   Heartbeat: {}",
        if config.heartbeat.enabled {
            "enabled"
        } else {
            "disabled"
        }
    );
    println!("   Routines: {}", config.routines.len());

    // Start workflow engine
    let workflow = std::sync::Arc::new(rustedclaw_workflow::WorkflowEngine::new(
        config.heartbeat.enabled,
        config.heartbeat.interval_minutes,
    ));

    // Load routines from config
    if !config.routines.is_empty() {
        let errors = workflow.load_routines(&config.routines).await;
        for err in &errors {
            tracing::warn!("Routine load error: {err}");
        }
        let loaded = config.routines.len() - errors.len();
        info!(
            loaded,
            total = config.routines.len(),
            "Routines loaded from config"
        );
    }

    let (mut task_rx, _workflow_handle) = workflow.start();

    info!("Workflow engine started");

    // Spawn a task to process triggered tasks
    tokio::spawn(async move {
        while let Some(triggered) = task_rx.recv().await {
            info!(
                task_id = %triggered.task_id,
                target = ?triggered.target_channel,
                "Processing triggered task"
            );
            // In a full implementation, this would:
            // 1. Create an AgentLoop with the task instruction
            // 2. Call the LLM provider
            // 3. Route the result to the target channel
            // For now, log the trigger
            tracing::debug!(
                instruction = %triggered.instruction,
                "Triggered task would be processed here"
            );
        }
    });

    // Start gateway (this blocks)
    rustedclaw_gateway::start(config).await?;

    Ok(())
}
