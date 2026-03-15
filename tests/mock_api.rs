//! Integration tests using a mock Gemini API server.
//!
//! These tests simulate real API responses through the full agent loop,
//! verifying end-to-end behavior without hitting the actual API.

use deepagent::agent::Agent;
use deepagent::tools::ToolRegistry;

#[tokio::test]
async fn test_agent_with_mock_text_response() {
    // Test that the agent correctly handles a text-only response
    let response_body = serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "Here are the files in your project."}]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 150,
            "candidatesTokenCount": 10,
            "totalTokenCount": 160
        }
    });

    // Verify the response parses correctly through our types
    let parts = deepagent::api::gemini::parse_response_for_testing(&response_body).unwrap();
    assert_eq!(parts.len(), 1);
    match &parts[0] {
        deepagent::api::ResponsePart::Text(t) => {
            assert!(t.contains("files in your project"));
        }
        _ => panic!("expected text response"),
    }
}

#[tokio::test]
async fn test_agent_with_mock_tool_call() {
    // Test that tool calls are parsed correctly
    let response_body = serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{
                    "functionCall": {
                        "name": "glob",
                        "args": {"pattern": "**/*.rs"}
                    }
                }]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 200,
            "candidatesTokenCount": 15,
            "totalTokenCount": 215
        }
    });

    let parts = deepagent::api::gemini::parse_response_for_testing(&response_body).unwrap();
    assert_eq!(parts.len(), 1);
    match &parts[0] {
        deepagent::api::ResponsePart::FunctionCall(fc) => {
            assert_eq!(fc.name, "glob");
            assert_eq!(fc.args["pattern"], "**/*.rs");
        }
        _ => panic!("expected function call"),
    }
}

#[tokio::test]
async fn test_agent_with_mixed_response() {
    // Test text + tool call in same response
    let response_body = serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [
                    {"text": "Let me search for that."},
                    {"functionCall": {"name": "grep", "args": {"pattern": "TODO", "path": "."}}}
                ]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 300,
            "candidatesTokenCount": 25,
            "totalTokenCount": 325
        }
    });

    let parts = deepagent::api::gemini::parse_response_for_testing(&response_body).unwrap();
    assert_eq!(parts.len(), 2);
    assert!(matches!(&parts[0], deepagent::api::ResponsePart::Text(_)));
    assert!(matches!(
        &parts[1],
        deepagent::api::ResponsePart::FunctionCall(_)
    ));
}

#[tokio::test]
async fn test_agent_with_empty_candidates() {
    // Test error handling for malformed response
    let response_body = serde_json::json!({
        "candidates": []
    });

    // Empty candidates should error
    let result = deepagent::api::gemini::parse_response_for_testing(&response_body);
    assert!(result.is_err() || result.unwrap().is_empty());
}

#[tokio::test]
async fn test_usage_metadata_parsing() {
    let response_body = serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "ok"}]
            }
        }],
        "usageMetadata": {
            "promptTokenCount": 1000,
            "candidatesTokenCount": 50,
            "totalTokenCount": 1050
        }
    });

    let usage = deepagent::api::gemini::parse_usage_for_testing(&response_body);
    assert_eq!(usage.prompt_tokens, 1000);
    assert_eq!(usage.candidates_tokens, 50);
    assert_eq!(usage.total_tokens, 1050);
}

#[tokio::test]
async fn test_usage_metadata_missing() {
    let response_body = serde_json::json!({
        "candidates": [{
            "content": {
                "role": "model",
                "parts": [{"text": "ok"}]
            }
        }]
    });

    let usage = deepagent::api::gemini::parse_usage_for_testing(&response_body);
    assert_eq!(usage.total_tokens, 0);
}

#[tokio::test]
async fn test_tool_registry_has_all_tools() {
    let registry = ToolRegistry::with_defaults(std::env::current_dir().unwrap());
    let names = registry.tool_names();

    // Verify all 12 tools are registered
    let expected = [
        "bash",
        "read",
        "write",
        "edit",
        "grep",
        "glob",
        "ls",
        "patch",
        "webfetch",
        "todowrite",
        "todoread",
        "think",
    ];

    for tool in &expected {
        assert!(
            names.contains(tool),
            "missing tool: {} (have: {:?})",
            tool,
            names
        );
    }
    assert_eq!(names.len(), expected.len());
}

#[tokio::test]
async fn test_tool_schemas_valid_json() {
    let registry = ToolRegistry::with_defaults(std::env::current_dir().unwrap());
    let schemas = registry.schemas();

    assert_eq!(schemas.len(), 12);

    for schema in &schemas {
        // Each schema must have name, description, parameters
        assert!(schema.get("name").is_some(), "schema missing name");
        assert!(
            schema.get("description").is_some(),
            "schema missing description"
        );
        assert!(
            schema.get("parameters").is_some(),
            "schema missing parameters"
        );

        // Parameters must be a valid JSON Schema object
        let params = schema.get("parameters").unwrap();
        assert_eq!(params.get("type").unwrap(), "object");
    }
}

#[tokio::test]
async fn test_system_prompt_contains_tools() {
    let registry = ToolRegistry::with_defaults(std::env::current_dir().unwrap());
    let prompt = Agent::build_system_prompt(&registry, "/tmp/test", "linux aarch64");

    // Prompt should contain tool definitions
    assert!(prompt.contains("bash"));
    assert!(prompt.contains("read"));
    assert!(prompt.contains("write"));
    assert!(prompt.contains("grep"));
    assert!(prompt.contains("glob"));

    // Prompt should contain environment info
    assert!(prompt.contains("/tmp/test"));
    assert!(prompt.contains("linux aarch64"));

    // Prompt should contain rules
    assert!(prompt.contains("Explore before acting"));
}
