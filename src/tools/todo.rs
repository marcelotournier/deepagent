use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::{Arc, Mutex};

/// A single TODO item managed by the agent during execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub id: usize,
    pub text: String,
    pub status: TodoStatus,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Done,
}

impl std::fmt::Display for TodoStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TodoStatus::Pending => write!(f, "pending"),
            TodoStatus::InProgress => write!(f, "in_progress"),
            TodoStatus::Done => write!(f, "done"),
        }
    }
}

/// Shared TODO list state for the agent session.
pub type TodoList = Arc<Mutex<Vec<TodoItem>>>;

pub fn new_todo_list() -> TodoList {
    Arc::new(Mutex::new(Vec::new()))
}

/// Tool to write/update TODO items.
pub struct TodoWriteTool {
    todos: TodoList,
}

impl TodoWriteTool {
    pub fn new(todos: TodoList) -> Self {
        Self { todos }
    }
}

#[async_trait]
impl super::Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todowrite"
    }

    fn description(&self) -> &str {
        "Manage a task list during execution. Can add new items, update status (pending/in_progress/done), or remove items by ID."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'add', 'update', or 'remove'",
                    "enum": ["add", "update", "remove"]
                },
                "text": {
                    "type": "string",
                    "description": "Task description (required for 'add')"
                },
                "id": {
                    "type": "integer",
                    "description": "Task ID (required for 'update' and 'remove')"
                },
                "status": {
                    "type": "string",
                    "description": "New status (for 'update'): 'pending', 'in_progress', or 'done'",
                    "enum": ["pending", "in_progress", "done"]
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: Value) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .context("missing 'action' parameter")?;

        let mut todos = self.todos.lock().unwrap();

        match action {
            "add" => {
                let text = args
                    .get("text")
                    .and_then(|v| v.as_str())
                    .context("missing 'text' for add action")?;

                let id = todos.len() + 1;
                todos.push(TodoItem {
                    id,
                    text: text.to_string(),
                    status: TodoStatus::Pending,
                });

                Ok(format!("Added task #{}: {}", id, text))
            }
            "update" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .context("missing 'id' for update action")? as usize;

                let status_str = args
                    .get("status")
                    .and_then(|v| v.as_str())
                    .context("missing 'status' for update action")?;

                let status = match status_str {
                    "pending" => TodoStatus::Pending,
                    "in_progress" => TodoStatus::InProgress,
                    "done" => TodoStatus::Done,
                    _ => anyhow::bail!("invalid status: {}", status_str),
                };

                if let Some(item) = todos.iter_mut().find(|t| t.id == id) {
                    item.status = status.clone();
                    Ok(format!("Updated task #{} to {}", id, status))
                } else {
                    anyhow::bail!("task #{} not found", id)
                }
            }
            "remove" => {
                let id = args
                    .get("id")
                    .and_then(|v| v.as_u64())
                    .context("missing 'id' for remove action")? as usize;

                let before = todos.len();
                todos.retain(|t| t.id != id);
                if todos.len() < before {
                    Ok(format!("Removed task #{}", id))
                } else {
                    anyhow::bail!("task #{} not found", id)
                }
            }
            _ => anyhow::bail!("unknown action: {}", action),
        }
    }
}

/// Tool to read the current TODO list.
pub struct TodoReadTool {
    todos: TodoList,
}

impl TodoReadTool {
    pub fn new(todos: TodoList) -> Self {
        Self { todos }
    }
}

#[async_trait]
impl super::Tool for TodoReadTool {
    fn name(&self) -> &str {
        "todoread"
    }

    fn description(&self) -> &str {
        "Read the current task list. Returns all tasks with their IDs, descriptions, and statuses."
    }

    fn parameters_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, _args: Value) -> Result<String> {
        let todos = self.todos.lock().unwrap();

        if todos.is_empty() {
            return Ok("No tasks.".to_string());
        }

        let mut output = String::new();
        for item in todos.iter() {
            let marker = match item.status {
                TodoStatus::Pending => "[ ]",
                TodoStatus::InProgress => "[~]",
                TodoStatus::Done => "[x]",
            };
            output.push_str(&format!(
                "{} #{}: {} ({})\n",
                marker, item.id, item.text, item.status
            ));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tool;

    #[tokio::test]
    async fn test_todo_add_and_read() {
        let todos = new_todo_list();
        let write_tool = TodoWriteTool::new(todos.clone());
        let read_tool = TodoReadTool::new(todos);

        // Add a task
        let result = write_tool
            .execute(serde_json::json!({"action": "add", "text": "Fix bug"}))
            .await
            .unwrap();
        assert!(result.contains("#1"));

        // Read tasks
        let result = read_tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("Fix bug"));
        assert!(result.contains("pending"));
    }

    #[tokio::test]
    async fn test_todo_update() {
        let todos = new_todo_list();
        let write_tool = TodoWriteTool::new(todos);

        write_tool
            .execute(serde_json::json!({"action": "add", "text": "Task 1"}))
            .await
            .unwrap();

        let result = write_tool
            .execute(serde_json::json!({"action": "update", "id": 1, "status": "done"}))
            .await
            .unwrap();
        assert!(result.contains("done"));
    }

    #[tokio::test]
    async fn test_todo_remove() {
        let todos = new_todo_list();
        let write_tool = TodoWriteTool::new(todos.clone());
        let read_tool = TodoReadTool::new(todos);

        write_tool
            .execute(serde_json::json!({"action": "add", "text": "Task 1"}))
            .await
            .unwrap();

        write_tool
            .execute(serde_json::json!({"action": "remove", "id": 1}))
            .await
            .unwrap();

        let result = read_tool.execute(serde_json::json!({})).await.unwrap();
        assert!(result.contains("No tasks"));
    }

    #[tokio::test]
    async fn test_todo_empty_list() {
        let todos = new_todo_list();
        let read_tool = TodoReadTool::new(todos);

        let result = read_tool.execute(serde_json::json!({})).await.unwrap();
        assert_eq!(result, "No tasks.");
    }

    #[tokio::test]
    async fn test_todo_update_not_found() {
        let todos = new_todo_list();
        let write_tool = TodoWriteTool::new(todos);

        let result = write_tool
            .execute(serde_json::json!({"action": "update", "id": 99, "status": "done"}))
            .await;
        assert!(result.is_err());
    }
}
