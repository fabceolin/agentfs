//! OpenAI API client for LLM-based document conversion
//!
//! This module provides an implementation of the `LLMClient` trait
//! for the OpenAI Chat Completions API.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::graphdocs::llm_converter::{LLMClient, LLMError};

/// OpenAI API client for chat completions
pub struct OpenAIClient {
    api_key: String,
    base_url: String,
}

#[derive(Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    temperature: f32,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Deserialize)]
struct Choice {
    message: MessageContent,
}

#[derive(Deserialize)]
struct MessageContent {
    content: String,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: ApiError,
}

#[derive(Deserialize)]
struct ApiError {
    message: String,
}

impl OpenAIClient {
    /// Create a new OpenAI client with the given API key
    pub fn new(api_key: String) -> Self {
        Self {
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
        }
    }

    /// Create a new OpenAI client with a custom base URL (e.g., for Azure OpenAI)
    pub fn with_base_url(api_key: String, base_url: String) -> Self {
        Self { api_key, base_url }
    }
}

#[async_trait]
impl LLMClient for OpenAIClient {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, LLMError> {
        let request = ChatRequest {
            model: model.to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
            temperature: 0.1, // Low temperature for consistent structured output
        };

        let client = reqwest::Client::new();
        let response = client
            .post(format!("{}/chat/completions", self.base_url))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    LLMError::Timeout
                } else {
                    LLMError::ApiError(e.to_string())
                }
            })?;

        let status = response.status();

        if status == 429 {
            return Err(LLMError::RateLimited);
        }

        if !status.is_success() {
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            // Try to parse as error response
            if let Ok(error_response) = serde_json::from_str::<ErrorResponse>(&error_text) {
                return Err(LLMError::ApiError(error_response.error.message));
            }

            return Err(LLMError::ApiError(format!(
                "HTTP {}: {}",
                status, error_text
            )));
        }

        let chat_response: ChatResponse = response
            .json()
            .await
            .map_err(|e| LLMError::ApiError(format!("Failed to parse response: {}", e)))?;

        chat_response
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .ok_or_else(|| LLMError::ApiError("No response choices returned".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = OpenAIClient::new("test-key".to_string());
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_client_with_custom_base_url() {
        let client = OpenAIClient::with_base_url(
            "test-key".to_string(),
            "https://custom.openai.azure.com".to_string(),
        );
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.base_url, "https://custom.openai.azure.com");
    }

    #[test]
    fn test_chat_request_serialization() {
        let request = ChatRequest {
            model: "gpt-4".to_string(),
            messages: vec![Message {
                role: "user".to_string(),
                content: "Hello".to_string(),
            }],
            temperature: 0.1,
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("\"model\":\"gpt-4\""));
        assert!(json.contains("\"role\":\"user\""));
        assert!(json.contains("\"content\":\"Hello\""));
        assert!(json.contains("\"temperature\":0.1"));
    }

    #[test]
    fn test_chat_response_deserialization() {
        let json = r#"{
            "choices": [
                {
                    "message": {
                        "content": "Hello back!"
                    }
                }
            ]
        }"#;

        let response: ChatResponse = serde_json::from_str(json).unwrap();
        assert_eq!(response.choices.len(), 1);
        assert_eq!(response.choices[0].message.content, "Hello back!");
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{
            "error": {
                "message": "Invalid API key"
            }
        }"#;

        let error: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(error.error.message, "Invalid API key");
    }
}
