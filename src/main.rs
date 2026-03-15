use anyhow::{Context, Result};
use clap::Parser;
use std::io::{IsTerminal, Read};

use deepagent::agent::Agent;
use deepagent::api::gemini::GeminiClient;
use deepagent::cli::{daily_limit_for_model, rpm_for_model, Cli};
use deepagent::tools::ToolRegistry;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(&cli.log_level)),
        )
        .with_writer(std::io::stderr)
        .init();

    // Read stdin if available (non-blocking check)
    let stdin_content = if cli.stdin || !std::io::stdin().is_terminal() {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read stdin")?;
        if buf.is_empty() {
            None
        } else {
            Some(buf)
        }
    } else {
        None
    };

    let prompt = cli.get_prompt(stdin_content).context(
        "No prompt provided. Use -p \"prompt\" or pipe input via stdin.\n\
         Example: deepagent -p \"list all .rs files\"\n\
         Example: echo \"explain this code\" | deepagent",
    )?;

    // Get API key
    let api_key = std::env::var("GEMINI_API_KEY").context(
        "GEMINI_API_KEY environment variable not set.\nGet a key from https://ai.google.dev",
    )?;

    // Set up working directory
    let working_dir = std::env::current_dir().context("failed to get current directory")?;

    // Create tools
    let tools = ToolRegistry::with_defaults(working_dir.clone());

    // Build system prompt
    let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    let system_prompt =
        Agent::build_system_prompt(&tools, &working_dir.display().to_string(), &os_info);

    // Create Gemini client with free-tier rate limits
    let daily_limit = daily_limit_for_model(&cli.model);
    let rpm = rpm_for_model(&cli.model);
    let client = GeminiClient::new(api_key, cli.model.clone(), daily_limit, rpm);

    // Create and run agent
    let agent = Agent::new(Box::new(client), tools, cli.max_turns, system_prompt);

    tracing::info!("Running agent with model: {}", cli.model);

    let result = agent.run(&prompt).await?;

    println!("{}", result);

    Ok(())
}
