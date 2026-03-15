use clap::Parser;

/// deepagent — A coding agent powered by Gemini, for Raspberry Pi
#[derive(Parser, Debug)]
#[command(name = "deepagent", version, about)]
pub struct Cli {
    /// Prompt to send to the agent
    #[arg(short, long)]
    pub prompt: Option<String>,

    /// Read prompt from stdin
    #[arg(long, default_value_t = false)]
    pub stdin: bool,

    /// Model to use (default: gemini-2.5-flash-preview-04-17, the latest Flash 3.1)
    #[arg(
        long,
        env = "DEEPAGENT_MODEL",
        default_value = "gemini-2.5-flash-preview-04-17"
    )]
    pub model: String,

    /// Maximum agent loop iterations
    #[arg(long, env = "DEEPAGENT_MAX_TURNS", default_value_t = 25)]
    pub max_turns: usize,

    /// Tool execution timeout in seconds
    #[arg(long, env = "DEEPAGENT_TIMEOUT", default_value_t = 120)]
    pub timeout: u64,

    /// Log level
    #[arg(long, env = "DEEPAGENT_LOG", default_value = "warn")]
    pub log_level: String,

    /// Show progress (tool calls and results) on stderr
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Output result as JSON (structured output for piping)
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

impl Cli {
    /// Get the prompt from -p flag, stdin, or both combined.
    pub fn get_prompt(&self, stdin_content: Option<String>) -> Option<String> {
        match (&self.prompt, stdin_content) {
            (Some(p), Some(s)) => Some(format!("{}\n\n---\n\n{}", p, s)),
            (Some(p), None) => Some(p.clone()),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    }
}

/// Daily request limits per model (Google AI Studio free tier).
pub fn daily_limit_for_model(model: &str) -> u64 {
    if model.contains("pro") {
        100
    } else if model.contains("lite") {
        1000
    } else {
        // Flash models (2.5-flash, 2.5-flash-preview-04-17, etc.)
        250
    }
}

/// RPM (requests per minute) limits per model (Google AI Studio free tier).
pub fn rpm_for_model(model: &str) -> u32 {
    if model.contains("pro") {
        5
    } else if model.contains("lite") {
        15
    } else {
        10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_prompt_flag_only() {
        let cli = Cli {
            prompt: Some("hello".into()),
            stdin: false,
            model: "gemini-2.5-flash-preview-04-17".into(),
            max_turns: 25,
            timeout: 120,
            log_level: "warn".into(),
            verbose: false,
            json: false,
        };
        assert_eq!(cli.get_prompt(None), Some("hello".into()));
    }

    #[test]
    fn test_get_prompt_stdin_only() {
        let cli = Cli {
            prompt: None,
            stdin: false,
            model: "gemini-2.5-flash-preview-04-17".into(),
            max_turns: 25,
            timeout: 120,
            log_level: "warn".into(),
            verbose: false,
            json: false,
        };
        assert_eq!(
            cli.get_prompt(Some("from stdin".into())),
            Some("from stdin".into())
        );
    }

    #[test]
    fn test_get_prompt_combined() {
        let cli = Cli {
            prompt: Some("explain this".into()),
            stdin: false,
            model: "gemini-2.5-flash-preview-04-17".into(),
            max_turns: 25,
            timeout: 120,
            log_level: "warn".into(),
            verbose: false,
            json: false,
        };
        let result = cli.get_prompt(Some("code here".into())).unwrap();
        assert!(result.contains("explain this"));
        assert!(result.contains("code here"));
    }

    #[test]
    fn test_get_prompt_none() {
        let cli = Cli {
            prompt: None,
            stdin: false,
            model: "gemini-2.5-flash-preview-04-17".into(),
            max_turns: 25,
            timeout: 120,
            log_level: "warn".into(),
            verbose: false,
            json: false,
        };
        assert!(cli.get_prompt(None).is_none());
    }

    #[test]
    fn test_daily_limits() {
        assert_eq!(daily_limit_for_model("gemini-2.5-flash-preview-04-17"), 250);
        assert_eq!(daily_limit_for_model("gemini-2.5-pro"), 100);
        assert_eq!(daily_limit_for_model("gemini-2.5-flash-lite"), 1000);
    }

    #[test]
    fn test_rpm_limits() {
        assert_eq!(rpm_for_model("gemini-2.5-flash-preview-04-17"), 10);
        assert_eq!(rpm_for_model("gemini-2.5-pro"), 5);
        assert_eq!(rpm_for_model("gemini-2.5-flash-lite"), 15);
    }
}
