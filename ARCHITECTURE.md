# deepagent Architecture

This document describes the internal design and architecture of `deepagent`, a Rust-based coding agent optimized for the Gemini API and Raspberry Pi environments.

## 1. Overall Design and Module Structure

The project is structured as a Rust workspace with a clear separation of concerns:

- **`src/main.rs`**: The CLI entry point. It handles argument parsing (via `clap`), initializes logging, manages sessions, and orchestrates the high-level execution flow.
- **`src/agent/`**: Core agent logic.
    - `executor.rs`: Implements the ReAct (Reasoning and Acting) loop, manages conversation state, and handles context compaction.
- **`src/api/`**: LLM backend abstractions.
    - `mod.rs`: Defines the `LlmClient` trait and common message types.
    - `gemini.rs`: Implementation of the Gemini REST client, including fallback logic and smart routing.
    - `rate_limiter.rs`: Implements token-bucket rate limiting with exponential backoff for both RPM and daily limits.
- **`src/tools/`**: Extensible tool system.
    - `mod.rs`: Defines the `Tool` trait and `ToolRegistry`.
    - Individual tool implementations (e.g., `bash.rs`, `read.rs`, `edit.rs`, `grep.rs`, `glob.rs`, `ls.rs`, `patch.rs`, `think.rs`, `todo.rs`, `webfetch.rs`, `write.rs`).
- **`src/session/`**: Persistence layer for agent sessions, allowing for resumption of interrupted tasks.
- **`src/cli/`**: CLI-specific configurations and helper functions.

## 2. Agent Loop (ReAct)

The agent operates in a ReAct loop implemented in `Agent::run_with_progress` (`src/agent/executor.rs`):

1.  **Prompt Assembly**: The agent starts with a system prompt (containing rules and environment info) and the user's initial prompt.
2.  **Generation**: The agent sends the conversation history and tool definitions to the LLM.
3.  **Parsing**: The LLM's response is parsed into text and/or function calls.
4.  **Execution**: If function calls are present, the agent executes the corresponding tools.
5.  **Observation**: The results of tool executions are appended to the conversation as `functionResponse` parts.
6.  **Iteration**: The loop repeats until the LLM provides a final text response or the maximum number of turns is reached.

### Context Management
To handle long-running sessions, the agent implements **context compaction**:
- When the estimated token count exceeds a threshold (80% of the context window), the agent "compacts" older tool results in the middle of the conversation.
- Compaction replaces large tool outputs with a brief summary (e.g., `[Tool result compacted to save space]`), preserving the most recent turns and the initial context.

### Loop Detection
The agent tracks recent tool calls. If it detects the same tool being called with the same arguments three times in a row, it injects a warning into the conversation to help the LLM break out of the loop.

## 3. Tool Registration and Execution

Tools are designed to be modular and easy to add:

- **`Tool` Trait**: Every tool must implement the `Tool` trait, which requires:
    - `name()`: The name used by the LLM to call the tool.
    - `description()`: A clear description of what the tool does.
    - `parameters_schema()`: A JSON Schema defining the tool's input arguments.
    - `execute()`: An async function that performs the tool's action and returns a string result.
- **`ToolRegistry`**: Manages the collection of available tools. It provides methods to generate Gemini-compatible function declarations and to dispatch calls to the correct tool implementation.
- **Execution Safety**: Tools like `bash` have configurable timeouts, and file-writing tools use atomic operations (via `tempfile`) to prevent data corruption.

## 4. Rate Limiting and Model Fallback

A key feature of `deepagent` is its resilience to free-tier API limits:

- **`RateLimiter`**: Enforces both Requests Per Minute (RPM) and Requests Per Day (RPD) limits. It uses exponential backoff with jitter and respects the `Retry-After` header from the API.
- **Fallback Chain**: The `GeminiClient` can be configured with multiple models (e.g., `gemini-2.5-flash` followed by `gemini-2.5-flash-lite`). If the primary model hits rate limits repeatedly, the client automatically falls back to the next model in the chain.
- **Smart Routing**:
    - **Lite for Dispatch**: The agent can hint to the client to use a "lite" model for simple tool calls (where reasoning is less critical) to save the primary model's quota.
    - **Primary for Reasoning**: It switches back to the primary model when it needs to synthesize information or plan complex steps.
- **Budget Guard**: When a model reaches 90% of its daily quota, the client automatically switches to the next model in the chain (usually a "lite" version with higher limits).

## 5. Key Design Decisions and Trade-offs

- **Rust for Performance**: Rust was chosen for its performance and safety, which is crucial for a tool that performs heavy file I/O and regex searches on resource-constrained devices like the Raspberry Pi.
- **Maturin/Python Integration**: By using `maturin`, `deepagent` can be easily installed via `pip`, making it accessible to Python users while maintaining Rust's performance.
- **Gemini-First**: While the architecture is somewhat generic, it is heavily optimized for Gemini's specific features, such as its large context window (1M+ tokens) and "thought signatures" in function calling.
- **Stateless vs. Stateful**: The agent is designed to be stateful within a session (maintaining conversation history) but can persist and resume state via the `session` module.
- **Parallelism**: Tools like `grep` and `glob` use `rayon` to leverage multiple CPU cores for fast file system operations.
- **Atomic File Operations**: To ensure reliability, file modifications are performed by writing to a temporary file and then renaming it, ensuring that a crash during a write doesn't leave the file in a corrupted state.
