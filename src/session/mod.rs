use crate::api::Message;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// A persisted agent session that can be saved and resumed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Session identifier.
    pub id: String,
    /// The original user prompt.
    pub prompt: String,
    /// Conversation history.
    pub messages: Vec<Message>,
    /// Model used.
    pub model: String,
    /// Number of turns completed.
    pub turns_completed: usize,
    /// Whether the session is complete.
    pub completed: bool,
    /// Timestamp of last update (Unix seconds).
    pub updated_at: u64,
}

impl Session {
    pub fn new(id: String, prompt: String, model: String) -> Self {
        Self {
            id,
            prompt,
            messages: Vec::new(),
            model,
            turns_completed: 0,
            completed: false,
            updated_at: now(),
        }
    }

    /// Save session to disk.
    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir).context("failed to create session directory")?;

        let path = dir.join(format!("{}.json", self.id));
        let json = serde_json::to_string_pretty(self).context("failed to serialize session")?;

        // Atomic write
        let temp = tempfile::NamedTempFile::new_in(dir)?;
        std::fs::write(temp.path(), json.as_bytes())?;
        temp.persist(&path)
            .with_context(|| format!("failed to save session to {}", path.display()))?;

        tracing::info!(
            "Session {} saved ({} messages)",
            self.id,
            self.messages.len()
        );
        Ok(())
    }

    /// Load a session from disk.
    pub fn load(dir: &Path, id: &str) -> Result<Self> {
        let path = dir.join(format!("{}.json", id));
        let json =
            std::fs::read_to_string(&path).with_context(|| format!("session not found: {}", id))?;

        serde_json::from_str(&json).context("failed to parse session file")
    }

    /// Load the most recent session from the session directory.
    /// Uses the `updated_at` field inside the session JSON (not filesystem mtime).
    pub fn load_latest(dir: &Path) -> Result<Self> {
        let mut sessions: Vec<Session> = std::fs::read_dir(dir)
            .context("no session directory")?
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| ext == "json")
                    .unwrap_or(false)
            })
            .filter_map(|e| {
                std::fs::read_to_string(e.path())
                    .ok()
                    .and_then(|json| serde_json::from_str::<Session>(&json).ok())
            })
            .collect();

        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

        sessions.into_iter().next().context("no sessions found")
    }

    /// List all sessions in the directory.
    pub fn list(dir: &Path) -> Result<Vec<SessionSummary>> {
        let mut summaries = Vec::new();

        if !dir.exists() {
            return Ok(summaries);
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json") {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<Session>(&json) {
                        summaries.push(SessionSummary {
                            id: session.id,
                            prompt: session.prompt[..session.prompt.len().min(80)].to_string(),
                            turns: session.turns_completed,
                            completed: session.completed,
                            updated_at: session.updated_at,
                        });
                    }
                }
            }
        }

        summaries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        Ok(summaries)
    }
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub id: String,
    pub prompt: String,
    pub turns: usize,
    pub completed: bool,
    pub updated_at: u64,
}

impl std::fmt::Display for SessionSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let status = if self.completed { "done" } else { "paused" };
        write!(
            f,
            "[{}] {} (turns: {}, {})",
            self.id, self.prompt, self.turns, status
        )
    }
}

/// Default session directory: ~/.deepagent/sessions/
pub fn default_session_dir() -> PathBuf {
    dirs_or_default().join("sessions")
}

fn dirs_or_default() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".deepagent")
    } else {
        PathBuf::from(".deepagent")
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Generate a short session ID.
pub fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = (0..8)
        .map(|_| {
            let idx = rng.gen_range(0..36);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect();
    chars.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::MessagePart;

    #[test]
    fn test_session_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let mut session = Session::new("test1".into(), "hello".into(), "flash".into());
        session.messages.push(Message {
            role: "user".into(),
            parts: vec![MessagePart::Text {
                text: "hello".into(),
            }],
        });
        session.turns_completed = 2;

        session.save(dir.path()).unwrap();

        let loaded = Session::load(dir.path(), "test1").unwrap();
        assert_eq!(loaded.id, "test1");
        assert_eq!(loaded.prompt, "hello");
        assert_eq!(loaded.turns_completed, 2);
        assert_eq!(loaded.messages.len(), 1);
    }

    #[test]
    fn test_session_load_latest() {
        let dir = tempfile::tempdir().unwrap();

        let mut s1 = Session::new("old".into(), "first".into(), "flash".into());
        s1.updated_at = 1000; // older timestamp
        s1.save(dir.path()).unwrap();

        let mut s2 = Session::new("new".into(), "second".into(), "flash".into());
        s2.updated_at = 2000; // newer timestamp
        s2.save(dir.path()).unwrap();

        let latest = Session::load_latest(dir.path()).unwrap();
        assert_eq!(latest.id, "new");
    }

    #[test]
    fn test_session_list() {
        let dir = tempfile::tempdir().unwrap();

        Session::new("a".into(), "task a".into(), "flash".into())
            .save(dir.path())
            .unwrap();
        Session::new("b".into(), "task b".into(), "flash".into())
            .save(dir.path())
            .unwrap();

        let list = Session::list(dir.path()).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_session_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let result = Session::load(dir.path(), "nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_session_id() {
        let id = generate_session_id();
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
