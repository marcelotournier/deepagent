use anyhow::{Context, Result};
use clap::Parser;
use std::io::{IsTerminal, Read};
use std::sync::{Arc, Mutex};

use deepagent::agent::{Agent, AgentEvent};
use deepagent::api::gemini::{GeminiClient, ModelConfig};
use deepagent::cli::{daily_limit_for_model, rpm_for_model, Cli};
use deepagent::session::{self, Session};
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

    // Handle --sessions flag: list and exit
    if cli.sessions {
        let session_dir = session::default_session_dir();
        let sessions = Session::list(&session_dir).unwrap_or_default();
        if sessions.is_empty() {
            println!("No saved sessions.");
        } else {
            for s in &sessions {
                println!("{}", s);
            }
        }
        return Ok(());
    }

    // Handle --init: create DEEPAGENT.md template
    if cli.init {
        let path = std::path::Path::new("DEEPAGENT.md");
        if path.exists() {
            eprintln!("DEEPAGENT.md already exists. Remove it first to reinitialize.");
            std::process::exit(1);
        }
        std::fs::write(
            path,
            "# DEEPAGENT.md — Project Instructions\n\n\
             <!-- deepagent reads this file and includes it in the system prompt. -->\n\
             <!-- Add project-specific rules, conventions, and context here. -->\n\n\
             ## Project Overview\n\n\
             <!-- Describe what this project does -->\n\n\
             ## Conventions\n\n\
             <!-- Coding style, commit conventions, testing requirements -->\n\n\
             ## Important Files\n\n\
             <!-- Key files the agent should know about -->\n",
        )
        .context("failed to write DEEPAGENT.md")?;
        println!("Created DEEPAGENT.md — edit it to customize agent behavior for this project.");
        return Ok(());
    }

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

    // Handle --resume: load session and continue
    let prompt = if let Some(ref resume_id) = cli.resume {
        let session_dir = session::default_session_dir();
        let session = if resume_id == "last" {
            Session::load_latest(&session_dir).context("no session to resume")?
        } else {
            Session::load(&session_dir, resume_id)
                .with_context(|| format!("session '{}' not found", resume_id))?
        };
        eprintln!(
            "Resuming session {} ({} turns completed)",
            session.id, session.turns_completed
        );
        // Use original prompt + "continue from where you left off"
        format!(
            "{}\n\n(Continuing from turn {}. Pick up where you left off.)",
            session.prompt, session.turns_completed
        )
    } else {
        cli.get_prompt(stdin_content).context(
            "No prompt provided. Use -p \"prompt\" or pipe input via stdin.\n\
             Example: deepagent -p \"list all .rs files\"\n\
             Example: echo \"explain this code\" | deepagent",
        )?
    };

    // Get API key
    let api_key = std::env::var("GEMINI_API_KEY").context(
        "GEMINI_API_KEY environment variable not set.\nGet a key from https://ai.google.dev",
    )?;

    // Set up working directory
    let working_dir = std::env::current_dir().context("failed to get current directory")?;

    // Create tools with configured timeout
    let tools = ToolRegistry::with_config(working_dir.clone(), cli.timeout, 8192);

    // Build system prompt (custom override or default)
    let system_prompt = if let Some(ref custom) = cli.system_prompt {
        custom.clone()
    } else {
        let os_info = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
        Agent::build_system_prompt(&tools, &working_dir.display().to_string(), &os_info)
    };

    // Create Gemini client with fallback chain for free-tier resilience
    let mut model_chain = vec![ModelConfig {
        name: cli.model.clone(),
        daily_limit: daily_limit_for_model(&cli.model),
        rpm: rpm_for_model(&cli.model),
    }];

    if !cli.model.contains("lite") {
        model_chain.push(ModelConfig {
            name: "gemini-2.5-flash-lite".to_string(),
            daily_limit: daily_limit_for_model("gemini-2.5-flash-lite"),
            rpm: rpm_for_model("gemini-2.5-flash-lite"),
        });
    }

    let client = GeminiClient::with_fallback(api_key, model_chain);

    // Create and run agent
    let agent = Agent::new(Box::new(client), tools, cli.max_turns, system_prompt);

    tracing::info!("Running agent with model: {}", cli.model);

    // Create session for persistence
    let session_id = session::generate_session_id();
    let session = Arc::new(Mutex::new(Session::new(
        session_id.clone(),
        prompt.clone(),
        cli.model.clone(),
    )));

    let verbose = cli.verbose;
    let json_mode = cli.json;
    let start_time = std::time::Instant::now();

    // Collect events for JSON output
    let events: Arc<Mutex<Vec<JsonEvent>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = events.clone();
    let session_clone = session.clone();

    let result = agent
        .run_with_progress(&prompt, move |event| {
            if verbose {
                print_progress(&event);
            }
            if json_mode {
                events_clone
                    .lock()
                    .unwrap()
                    .push(JsonEvent::from_agent_event(&event));
            }
            // Track turns in session
            if let AgentEvent::TurnStart { turn, .. } = &event {
                session_clone.lock().unwrap().turns_completed = *turn;
            }
        })
        .await;

    let elapsed = start_time.elapsed();

    // Save session
    {
        let mut s = session.lock().unwrap();
        s.completed = result.is_ok();
        let session_dir = session::default_session_dir();
        if let Err(e) = s.save(&session_dir) {
            tracing::warn!("Failed to save session: {}", e);
        }
    }

    let result = result?;

    if json_mode {
        let collected = events.lock().unwrap();
        let tool_calls: usize = collected
            .iter()
            .filter(|e| e.event_type == "tool_call")
            .count();
        let turns: usize = collected
            .iter()
            .filter(|e| e.event_type == "turn_start")
            .count();
        // Rough token estimate: chars / 4
        let estimated_output_tokens = result.len() / 4;

        let output = serde_json::json!({
            "result": result,
            "metrics": {
                "elapsed_ms": elapsed.as_millis(),
                "turns": turns,
                "tool_calls": tool_calls,
                "estimated_output_tokens": estimated_output_tokens,
                "result_chars": result.len(),
            },
            "model": cli.model,
            "session_id": session_id,
            "events": collected.iter().map(|e| serde_json::to_value(e).unwrap()).collect::<Vec<_>>(),
        });
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        println!("{}", result);
        if verbose {
            eprintln!(
                "\x1b[90m[session {} | completed in {:.2}s]\x1b[0m",
                session_id,
                elapsed.as_secs_f64()
            );
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct JsonEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_turns: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

impl JsonEvent {
    fn from_agent_event(event: &AgentEvent) -> Self {
        match event {
            AgentEvent::TurnStart { turn, max_turns } => Self {
                event_type: "turn_start".into(),
                turn: Some(*turn),
                max_turns: Some(*max_turns),
                name: None,
                args: None,
                output: None,
                text: None,
            },
            AgentEvent::ToolCall { name, args } => Self {
                event_type: "tool_call".into(),
                turn: None,
                max_turns: None,
                name: Some(name.clone()),
                args: Some(args.clone()),
                output: None,
                text: None,
            },
            AgentEvent::ToolResult { name, output } => Self {
                event_type: "tool_result".into(),
                turn: None,
                max_turns: None,
                name: Some(name.clone()),
                args: None,
                output: Some(output.clone()),
                text: None,
            },
            AgentEvent::ModelText { text } => Self {
                event_type: "model_text".into(),
                turn: None,
                max_turns: None,
                name: None,
                args: None,
                output: None,
                text: Some(text.clone()),
            },
        }
    }
}

fn print_progress(event: &AgentEvent) {
    use std::io::Write;
    let stderr = std::io::stderr();
    let mut err = stderr.lock();

    match event {
        AgentEvent::TurnStart { turn, max_turns } => {
            let _ = writeln!(err, "\x1b[90m[turn {}/{}]\x1b[0m", turn, max_turns);
        }
        AgentEvent::ToolCall { name, args } => {
            let short_args = if args.len() > 80 {
                format!("{}...", &args[..77])
            } else {
                args.clone()
            };
            let _ = writeln!(err, "\x1b[36m▶ {}({})\x1b[0m", name, short_args);
        }
        AgentEvent::ToolResult { name, output } => {
            let lines: Vec<&str> = output.lines().take(3).collect();
            let preview = lines.join("\n  ");
            let _ = writeln!(err, "\x1b[32m✓ {}\x1b[0m\n  {}", name, preview);
        }
        AgentEvent::ModelText { text } => {
            let preview = if text.len() > 100 {
                format!("{}...", &text[..97])
            } else {
                text.clone()
            };
            let _ = writeln!(err, "\x1b[33m● {}\x1b[0m", preview);
        }
    }
}
