//! PureTerm 的 AI 集成客户端。
//!
//! 为 OpenAI、Claude 和 Ollama 后端提供实现。

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// AI 对话中的消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: AiRole,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiRole {
    System,
    User,
    Assistant,
}

/// AI 后端的配置。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiConfig {
    pub backend: AiBackend,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub base_url: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiBackend {
    OpenAi,
    Claude,
    Ollama,
    Disabled,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            backend: AiBackend::Disabled,
            api_key: None,
            model: None,
            base_url: None,
            max_tokens: Some(1024),
            temperature: Some(0.7),
        }
    }
}

impl AiConfig {
    /// 返回后端的有效基础 URL。
    pub fn effective_base_url(&self) -> String {
        match self.backend {
            AiBackend::OpenAi => self.base_url.clone().unwrap_or_else(|| "https://api.openai.com".into()),
            AiBackend::Claude => self.base_url.clone().unwrap_or_else(|| "https://api.anthropic.com".into()),
            AiBackend::Ollama => self.base_url.clone().unwrap_or_else(|| "http://localhost:11434".into()),
            AiBackend::Disabled => String::new(),
        }
    }

    /// 返回有效的模型名称。
    pub fn effective_model(&self) -> String {
        match self.backend {
            AiBackend::OpenAi => self.model.clone().unwrap_or_else(|| "gpt-4".into()),
            AiBackend::Claude => self.model.clone().unwrap_or_else(|| "claude-3-sonnet-20240229".into()),
            AiBackend::Ollama => self.model.clone().unwrap_or_else(|| "llama3".into()),
            AiBackend::Disabled => String::new(),
        }
    }
}

/// AI 后端的响应。
#[derive(Debug, Clone)]
pub struct AiResponse {
    pub content: String,
    pub model: String,
    pub usage: Option<TokenUsage>,
}

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// AI 服务后端的 trait。
pub trait AiService: Send + Sync {
    /// 发送聊天补全请求。
    fn chat(&self, messages: &[AiMessage]) -> Result<AiResponse, AiError>;

    /// 获取给定上下文的命令建议。
    fn suggest_command(&self, context: &str, history: &[String]) -> Result<Vec<String>, AiError>;

    /// 解释错误消息。
    fn explain_error(&self, error_output: &str) -> Result<String, AiError>;

    /// 检查服务是否已配置且可用。
    fn is_available(&self) -> bool;
}

#[derive(Debug)]
pub enum AiError {
    NotConfigured,
    ApiError(String),
    NetworkError(String),
    RateLimited,
    InvalidResponse,
}

impl From<reqwest::Error> for AiError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_timeout() {
            AiError::NetworkError("Request timed out".into())
        } else if err.is_connect() {
            AiError::NetworkError("Failed to connect to AI service".into())
        } else if err.status() == Some(reqwest::StatusCode::TOO_MANY_REQUESTS) {
            AiError::RateLimited
        } else {
            AiError::ApiError(err.to_string())
        }
    }
}

// 手动实现而非派生,因为 reqwest::Error 不支持 thiserror
impl std::fmt::Display for AiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiError::NotConfigured => write!(f, "AI service not configured"),
            AiError::ApiError(s) => write!(f, "API error: {}", s),
            AiError::NetworkError(s) => write!(f, "Network error: {}", s),
            AiError::RateLimited => write!(f, "Rate limited"),
            AiError::InvalidResponse => write!(f, "Invalid response format"),
        }
    }
}

impl std::error::Error for AiError {}

// ============================================================================
// OpenAI 服务
// ============================================================================

pub struct OpenAiService {
    client: reqwest::Client,
    config: AiConfig,
}

impl OpenAiService {
    pub fn new(config: AiConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    fn build_request(&self, messages: &[AiMessage]) -> serde_json::Value {
        let messages_json: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        AiRole::System => "system",
                        AiRole::User => "user",
                        AiRole::Assistant => "assistant",
                    },
                    "content": m.content,
                })
            })
            .collect();

        let mut request_body = serde_json::json!({
            "model": self.config.effective_model(),
            "messages": messages_json,
        });

        if let Some(max_tokens) = self.config.max_tokens {
            request_body["max_tokens"] = serde_json::json!(max_tokens);
        }
        if let Some(temperature) = self.config.temperature {
            request_body["temperature"] = serde_json::json!(temperature);
        }

        request_body
    }

    async fn make_request(&self, messages: &[AiMessage]) -> Result<serde_json::Value, AiError> {
        let url = format!("{}/v1/chat/completions", self.config.effective_base_url());
        let request_body = self.build_request(messages);

        let mut request = self.client
            .post(&url)
            .header("Content-Type", "application/json");

        if let Some(api_key) = &self.config.api_key {
            request = request.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request
            .json(&request_body)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AiError::RateLimited);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!("Status {}: {}", status, body)));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }
}

impl AiService for OpenAiService {
    fn chat(&self, messages: &[AiMessage]) -> Result<AiResponse, AiError> {
        if !self.is_available() {
            return Err(AiError::NotConfigured);
        }

        let rt = tokio::runtime::Handle::current();
        let messages = messages.to_vec();

        let response = rt.block_on(async {
            self.make_request(&messages).await
        })?;

        let content = response["choices"][0]["message"]["content"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .to_string();

        let model = response["model"]
            .as_str()
            .unwrap_or(&self.config.effective_model())
            .to_string();

        let usage = if let Some(u) = response.get("usage") {
            Some(TokenUsage {
                prompt_tokens: u.get("prompt_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                completion_tokens: u.get("completion_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                total_tokens: u.get("total_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            })
        } else {
            None
        };

        Ok(AiResponse { content, model, usage })
    }

    fn suggest_command(&self, context: &str, history: &[String]) -> Result<Vec<String>, AiError> {
        let history_text = history.iter().rev().take(10).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "Based on the current directory context and recent command history, \
            suggest 3 possible commands to run next. Respond ONLY with a JSON array of strings, \
            nothing else.\n\nCurrent context:\n{}\n\nRecent history:\n{}",
            context, history_text
        );

        let messages = vec![
            AiMessage { role: AiRole::System, content: "You are a shell command expert. Respond ONLY with a JSON array of suggested commands.".into() },
            AiMessage { role: AiRole::User, content: prompt },
        ];

        let response = self.chat(&messages)?;
        let commands: Vec<String> = serde_json::from_str(&response.content)
            .or_else(|_| {
                // Try to parse from markdown code blocks
                let cleaned = response.content
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```");
                serde_json::from_str(cleaned)
            })
            .unwrap_or_else(|_| vec![response.content]);

        Ok(commands)
    }

    fn explain_error(&self, error_output: &str) -> Result<String, AiError> {
        let messages = vec![
            AiMessage {
                role: AiRole::System,
                content: "You are a helpful terminal assistant. Explain errors in plain language and suggest fixes.".into(),
            },
            AiMessage {
                role: AiRole::User,
                content: format!("Explain this error and suggest how to fix it:\n{}", error_output),
            },
        ];

        self.chat(&messages).map(|r| r.content)
    }

    fn is_available(&self) -> bool {
        self.config.api_key.as_ref().map_or(false, |k| !k.is_empty())
    }
}

// ============================================================================
// Claude 服务 (Anthropic)
// ============================================================================

pub struct ClaudeService {
    client: reqwest::Client,
    config: AiConfig,
}

impl ClaudeService {
    pub fn new(config: AiConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    async fn make_request(&self, messages: &[AiMessage]) -> Result<serde_json::Value, AiError> {
        let url = format!("{}/v1/messages", self.config.effective_base_url());

        let mut request_body = serde_json::json!({
            "model": self.config.effective_model(),
            "max_tokens": self.config.max_tokens.unwrap_or(1024),
        });

        // 将消息转换为 Claude 格式
        let anthropic_messages: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.role != AiRole::System) // Claude 不在 messages 数组中使用系统消息
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        AiRole::User => "user",
                        AiRole::Assistant => "assistant",
                        _ => "user",
                    },
                    "content": m.content,
                })
            })
            .collect();

        request_body["messages"] = serde_json::json!(anthropic_messages);

        if let Some(temperature) = self.config.temperature {
            request_body["temperature"] = serde_json::json!(temperature);
        }

        let mut request = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("anthropic-version", "2023-06-01");

        if let Some(api_key) = &self.config.api_key {
            request = request.header("x-api-key", api_key);
        }

        let response = request
            .json(&request_body)
            .send()
            .await?;

        if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(AiError::RateLimited);
        }

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!("Status {}: {}", status, body)));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }
}

impl AiService for ClaudeService {
    fn chat(&self, messages: &[AiMessage]) -> Result<AiResponse, AiError> {
        if !self.is_available() {
            return Err(AiError::NotConfigured);
        }

        let rt = tokio::runtime::Handle::current();
        let messages = messages.to_vec();

        let response = rt.block_on(async {
            self.make_request(&messages).await
        })?;

        let content = response["content"][0]["text"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .to_string();

        let model = response["model"]
            .as_str()
            .unwrap_or(&self.config.effective_model())
            .to_string();

        let usage = if let Some(u) = response.get("usage") {
            Some(TokenUsage {
                prompt_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                completion_tokens: u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                total_tokens: u.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32 + u.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
            })
        } else {
            None
        };

        Ok(AiResponse { content, model, usage })
    }

    fn suggest_command(&self, context: &str, history: &[String]) -> Result<Vec<String>, AiError> {
        let history_text = history.iter().rev().take(10).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "Based on the current directory context and recent command history, \
            suggest 3 possible commands to run next. Respond ONLY with a JSON array of strings, \
            nothing else.\n\nCurrent context:\n{}\n\nRecent history:\n{}",
            context, history_text
        );

        let messages = vec![
            AiMessage { role: AiRole::System, content: "You are a shell command expert. Respond ONLY with a JSON array of suggested commands.".into() },
            AiMessage { role: AiRole::User, content: prompt },
        ];

        let response = self.chat(&messages)?;
        let commands: Vec<String> = serde_json::from_str(&response.content)
            .or_else(|_| {
                let cleaned = response.content
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```");
                serde_json::from_str(cleaned)
            })
            .unwrap_or_else(|_| vec![response.content]);

        Ok(commands)
    }

    fn explain_error(&self, error_output: &str) -> Result<String, AiError> {
        let messages = vec![
            AiMessage {
                role: AiRole::System,
                content: "You are a helpful terminal assistant. Explain errors in plain language and suggest fixes.".into(),
            },
            AiMessage {
                role: AiRole::User,
                content: format!("Explain this error and suggest how to fix it:\n{}", error_output),
            },
        ];

        self.chat(&messages).map(|r| r.content)
    }

    fn is_available(&self) -> bool {
        self.config.api_key.as_ref().map_or(false, |k| !k.is_empty())
    }
}

// ============================================================================
// Ollama 服务 (本地模型)
// ============================================================================

pub struct OllamaService {
    client: reqwest::Client,
    config: AiConfig,
}

impl OllamaService {
    pub fn new(config: AiConfig) -> Self {
        Self {
            client: reqwest::Client::new(),
            config,
        }
    }

    async fn make_request(&self, messages: &[AiMessage]) -> Result<serde_json::Value, AiError> {
        let url = format!("{}/api/chat", self.config.effective_base_url());

        let ollama_messages: Vec<serde_json::Value> = messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": match m.role {
                        AiRole::System => "system",
                        AiRole::User => "user",
                        AiRole::Assistant => "assistant",
                    },
                    "content": m.content,
                })
            })
            .collect();

        let mut request_body = serde_json::json!({
            "model": self.config.effective_model(),
            "messages": ollama_messages,
            "stream": false,
        });

        if let Some(temperature) = self.config.temperature {
            request_body["temperature"] = serde_json::json!(temperature);
        }
        if let Some(max_tokens) = self.config.max_tokens {
            request_body["options"] = serde_json::json!({ "num_predict": max_tokens });
        }

        let response = self.client
            .post(&url)
            .json(&request_body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AiError::ApiError(format!("Status {}: {}", status, body)));
        }

        let json: serde_json::Value = response.json().await?;
        Ok(json)
    }
}

impl AiService for OllamaService {
    fn chat(&self, messages: &[AiMessage]) -> Result<AiResponse, AiError> {
        if !self.is_available() {
            return Err(AiError::NotConfigured);
        }

        let rt = tokio::runtime::Handle::current();
        let messages = messages.to_vec();

        let response = rt.block_on(async {
            self.make_request(&messages).await
        })?;

        let content = response["message"]["content"]
            .as_str()
            .ok_or(AiError::InvalidResponse)?
            .to_string();

        let model = response["model"]
            .as_str()
            .unwrap_or(&self.config.effective_model())
            .to_string();

        Ok(AiResponse {
            content,
            model,
            usage: None,
        })
    }

    fn suggest_command(&self, context: &str, history: &[String]) -> Result<Vec<String>, AiError> {
        let history_text = history.iter().rev().take(10).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "Based on the current directory context and recent command history, \
            suggest 3 possible commands to run next. Respond ONLY with a JSON array of strings, \
            nothing else.\n\nCurrent context:\n{}\n\nRecent history:\n{}",
            context, history_text
        );

        let messages = vec![
            AiMessage { role: AiRole::System, content: "You are a shell command expert. Respond ONLY with a JSON array of suggested commands.".into() },
            AiMessage { role: AiRole::User, content: prompt },
        ];

        let response = self.chat(&messages)?;
        let commands: Vec<String> = serde_json::from_str(&response.content)
            .or_else(|_| {
                let cleaned = response.content
                    .trim()
                    .trim_start_matches("```json")
                    .trim_start_matches("```")
                    .trim_end_matches("```");
                serde_json::from_str(cleaned)
            })
            .unwrap_or_else(|_| vec![response.content]);

        Ok(commands)
    }

    fn explain_error(&self, error_output: &str) -> Result<String, AiError> {
        let messages = vec![
            AiMessage {
                role: AiRole::System,
                content: "You are a helpful terminal assistant. Explain errors in plain language and suggest fixes.".into(),
            },
            AiMessage {
                role: AiRole::User,
                content: format!("Explain this error and suggest how to fix it:\n{}", error_output),
            },
        ];

        self.chat(&messages).map(|r| r.content)
    }

    fn is_available(&self) -> bool {
        // Ollama 不需要 API 密钥,只需要运行即可
        true
    }
}

// ============================================================================
// 禁用服务 (无操作)
// ============================================================================

pub struct DisabledAiService;

impl AiService for DisabledAiService {
    fn chat(&self, _messages: &[AiMessage]) -> Result<AiResponse, AiError> {
        Err(AiError::NotConfigured)
    }

    fn suggest_command(&self, _context: &str, _history: &[String]) -> Result<Vec<String>, AiError> {
        Err(AiError::NotConfigured)
    }

    fn explain_error(&self, _error_output: &str) -> Result<String, AiError> {
        Err(AiError::NotConfigured)
    }

    fn is_available(&self) -> bool {
        false
    }
}

// ============================================================================
// 工厂
// ============================================================================

/// 工厂,用于创建适当的 AI 服务。
pub fn create_ai_service(config: &AiConfig) -> Box<dyn AiService> {
    match config.backend {
        AiBackend::Disabled => Box::new(DisabledAiService),
        AiBackend::OpenAi => Box::new(OpenAiService::new(config.clone())),
        AiBackend::Claude => Box::new(ClaudeService::new(config.clone())),
        AiBackend::Ollama => Box::new(OllamaService::new(config.clone())),
    }
}

/// 创建包装在 Arc 中的新 AI 服务以供共享访问。
pub fn create_ai_service_arc(config: &AiConfig) -> Arc<dyn AiService> {
    match config.backend {
        AiBackend::Disabled => Arc::new(DisabledAiService) as Arc<dyn AiService>,
        AiBackend::OpenAi => Arc::new(OpenAiService::new(config.clone())) as Arc<dyn AiService>,
        AiBackend::Claude => Arc::new(ClaudeService::new(config.clone())) as Arc<dyn AiService>,
        AiBackend::Ollama => Arc::new(OllamaService::new(config.clone())) as Arc<dyn AiService>,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_disabled_service_is_not_available() {
        let service: Box<dyn AiService> = create_ai_service(&AiConfig::default());
        assert!(!service.is_available());
    }

    #[test]
    fn test_openai_service_requires_api_key() {
        let config = AiConfig {
            backend: AiBackend::OpenAi,
            api_key: None,
            ..Default::default()
        };
        let service: Box<dyn AiService> = create_ai_service(&config);
        assert!(!service.is_available());

        let config_with_key = AiConfig {
            backend: AiBackend::OpenAi,
            api_key: Some("test-key".into()),
            ..Default::default()
        };
        let service: Box<dyn AiService> = create_ai_service(&config_with_key);
        assert!(service.is_available());
    }

    #[test]
    fn test_ollama_service_always_available() {
        let config = AiConfig {
            backend: AiBackend::Ollama,
            api_key: None,
            ..Default::default()
        };
        let service: Box<dyn AiService> = create_ai_service(&config);
        assert!(service.is_available());
    }

    #[test]
    fn test_config_effective_urls() {
        let openai_config = AiConfig {
            backend: AiBackend::OpenAi,
            base_url: None,
            ..Default::default()
        };
        assert_eq!(openai_config.effective_base_url(), "https://api.openai.com");

        let custom_config = AiConfig {
            backend: AiBackend::OpenAi,
            base_url: Some("https://custom.openai.com".into()),
            ..Default::default()
        };
        assert_eq!(custom_config.effective_base_url(), "https://custom.openai.com");

        let ollama_config = AiConfig {
            backend: AiBackend::Ollama,
            base_url: None,
            ..Default::default()
        };
        assert_eq!(ollama_config.effective_base_url(), "http://localhost:11434");
    }

    #[test]
    fn test_config_effective_models() {
        let config = AiConfig {
            backend: AiBackend::OpenAi,
            model: None,
            ..Default::default()
        };
        assert_eq!(config.effective_model(), "gpt-4");

        let config = AiConfig {
            backend: AiBackend::Claude,
            model: None,
            ..Default::default()
        };
        assert_eq!(config.effective_model(), "claude-3-sonnet-20240229");

        let config = AiConfig {
            backend: AiBackend::Ollama,
            model: None,
            ..Default::default()
        };
        assert_eq!(config.effective_model(), "llama3");
    }
}