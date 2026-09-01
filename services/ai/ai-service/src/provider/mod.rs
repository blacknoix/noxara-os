//! LLM providers — mock when AI_API_KEY unset, OpenAI-compatible when set.

use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::Citation;

pub const PROMPT_TEMPLATE_VERSION: &str = "ai.chat.v1";

#[derive(Debug, Clone)]
pub struct CompletionRequest {
    pub system: String,
    pub user_message: String,
    pub context: String,
    pub citations: Vec<Citation>,
}

#[derive(Debug, Clone)]
pub struct CompletionResult {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub latency_ms: u32,
    pub cost_estimate_minor: i64,
    pub model: String,
    pub suggested_tools: Vec<String>,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResult, String>;
    async fn stream_tokens(&self, req: CompletionRequest) -> Result<Vec<String>, String>;
}

pub struct MockProvider;

impl MockProvider {
    fn detect_tools(message: &str) -> Vec<String> {
        let lower = message.to_ascii_lowercase();
        let mut tools = Vec::new();
        if lower.contains("create invoice")
            || lower.contains("create_invoice")
            || lower.contains("new invoice")
        {
            tools.push("create_invoice".into());
        }
        if lower.contains("create task")
            || lower.contains("create_task")
            || lower.contains("new task")
        {
            tools.push("create_task".into());
        }
        if lower.contains("create expense")
            || lower.contains("create_expense")
            || lower.contains("new expense")
        {
            tools.push("create_expense".into());
        }
        if lower.contains("follow up") || lower.contains("follow-up") {
            tools.push("draft_follow_up_activity".into());
        }
        if lower.contains("deal note") || lower.contains("note on deal") {
            tools.push("create_deal_note".into());
        }
        tools
    }

    fn build_content(req: &CompletionRequest) -> String {
        let cite_refs: Vec<String> = req
            .citations
            .iter()
            .take(3)
            .map(|c| format!("[{}: {}]", c.record_type, c.title))
            .collect();
        let cite_line = if cite_refs.is_empty() {
            "No workspace records retrieved.".to_string()
        } else {
            format!("Based on: {}", cite_refs.join(", "))
        };
        format!(
            "{cite_line}\n\nI reviewed the provided context and can help with your request about \"{}\". \
             Ask me to create invoices, tasks, or follow-ups and I'll prepare a proposal for your review.",
            req.user_message.chars().take(80).collect::<String>()
        )
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResult, String> {
        let start = Instant::now();
        let suggested_tools = Self::detect_tools(&req.user_message);
        let content = Self::build_content(&req);
        let input_tokens = (req.system.len() + req.user_message.len() + req.context.len()) / 4;
        let output_tokens = content.len() / 4;
        Ok(CompletionResult {
            content,
            input_tokens: input_tokens as u32,
            output_tokens: output_tokens as u32,
            latency_ms: start.elapsed().as_millis() as u32,
            cost_estimate_minor: 0,
            model: "mock".into(),
            suggested_tools,
        })
    }

    async fn stream_tokens(&self, req: CompletionRequest) -> Result<Vec<String>, String> {
        let result = self.complete(req).await?;
        Ok(result
            .content
            .split_whitespace()
            .map(|w| format!("{w} "))
            .collect())
    }
}

pub struct OpenAiCompatibleProvider {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    client: reqwest::Client,
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
    usage: Option<Usage>,
    model: Option<String>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageOut,
}

#[derive(Deserialize)]
struct ChatMessageOut {
    content: String,
}

#[derive(Deserialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionResult, String> {
        let start = Instant::now();
        let user_content = format!(
            "{}\n\nUser message: {}\n\nContext:\n{}",
            req.system, req.user_message, req.context
        );
        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".into(),
                    content:
                        "You are CompanyOS copilot. Cite retrieved records. Never execute writes."
                            .into(),
                },
                ChatMessage {
                    role: "user".into(),
                    content: user_content,
                },
            ],
        };
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let resp = self
            .client
            .post(&url)
            .header(
                axum::http::header::AUTHORIZATION,
                format!("Bearer {}", self.api_key),
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(format!("openai error: {}", text));
        }
        let parsed: ChatCompletionResponse = resp.json().await.map_err(|e| e.to_string())?;
        let content = parsed
            .choices
            .first()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let input_tokens = parsed.usage.as_ref().map(|u| u.prompt_tokens).unwrap_or(0);
        let output_tokens = parsed
            .usage
            .as_ref()
            .map(|u| u.completion_tokens)
            .unwrap_or(0);
        let suggested_tools = MockProvider::detect_tools(&req.user_message);
        Ok(CompletionResult {
            content,
            input_tokens,
            output_tokens,
            latency_ms: start.elapsed().as_millis() as u32,
            cost_estimate_minor: estimate_cost_minor(&self.model, input_tokens, output_tokens),
            model: parsed.model.unwrap_or_else(|| self.model.clone()),
            suggested_tools,
        })
    }

    async fn stream_tokens(&self, req: CompletionRequest) -> Result<Vec<String>, String> {
        let result = self.complete(req).await?;
        Ok(result
            .content
            .split_whitespace()
            .map(|w| format!("{w} "))
            .collect())
    }
}

fn estimate_cost_minor(model: &str, input: u32, output: u32) -> i64 {
    let (in_rate, out_rate): (f64, f64) = if model.contains("gpt-4") {
        (250.0, 1000.0)
    } else {
        (15.0, 60.0)
    };
    let cost = (input as f64 * in_rate + output as f64 * out_rate) / 1_000_000.0;
    (cost * 100.0).round() as i64
}

/// Game-day / ops: force provider failures without a live API key.
pub struct DownProvider;

#[async_trait]
impl LlmProvider for DownProvider {
    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResult, String> {
        Err("AI provider forced down (AI_PROVIDER_FORCE_DOWN=1)".into())
    }

    async fn stream_tokens(&self, _req: CompletionRequest) -> Result<Vec<String>, String> {
        Err("AI provider forced down (AI_PROVIDER_FORCE_DOWN=1)".into())
    }
}

pub fn build_provider() -> Arc<dyn LlmProvider> {
    if std::env::var("AI_PROVIDER_FORCE_DOWN").ok().as_deref() == Some("1") {
        return Arc::new(DownProvider);
    }
    let api_key = std::env::var("AI_API_KEY").unwrap_or_default();
    if api_key.is_empty() {
        Arc::new(MockProvider)
    } else {
        let base =
            std::env::var("AI_API_BASE").unwrap_or_else(|_| "https://api.openai.com/v1".into());
        let model = std::env::var("AI_MODEL").unwrap_or_else(|_| "gpt-4o-mini".into());
        Arc::new(OpenAiCompatibleProvider {
            base_url: base,
            api_key,
            model,
            client: reqwest::Client::new(),
        })
    }
}

pub fn wrap_untrusted(content: &str) -> String {
    format!("<<<UNTRUSTED_DOCUMENT>>>\n{content}\n<<<END_UNTRUSTED_DOCUMENT>>>")
}
