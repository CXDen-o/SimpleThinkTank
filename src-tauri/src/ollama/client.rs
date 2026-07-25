// Ollama HTTP 客户端
// 调用 Ollama RESTful API 完成文本生成、向量化、模型管理

use crate::error::{AppError, AppResult};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Ollama 客户端
#[derive(Debug, Clone)]
pub struct OllamaClient {
    base_url: String,
    http: Client,
}

/// 构造参数(支持代理、超时)
pub struct OllamaClientConfig {
    pub base_url: String,
    pub proxy: Option<String>,
    pub connect_timeout: Duration,
    pub request_timeout: Duration,
}

impl Default for OllamaClientConfig {
    fn default() -> Self {
        Self {
            base_url: super::DEFAULT_OLLAMA_URL.to_string(),
            proxy: None,
            connect_timeout: Duration::from_secs(30),
            request_timeout: Duration::from_secs(300),
        }
    }
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self::new_with_config(OllamaClientConfig::default())
    }
}

impl OllamaClient {
    pub fn new(base_url: &str) -> Self {
        Self::new_with_config(OllamaClientConfig {
            base_url: base_url.to_string(),
            ..Default::default()
        })
    }

    pub fn new_with_config(cfg: OllamaClientConfig) -> Self {
        let mut builder = Client::builder()
            .connect_timeout(cfg.connect_timeout)
            .timeout(cfg.request_timeout);
        if let Some(proxy_url) = cfg.proxy.as_ref().filter(|p| !p.is_empty()) {
            // reqwest::Proxy 同时支持 http/https/socks5
            match reqwest::Proxy::all(proxy_url) {
                Ok(p) => {
                    builder = builder.proxy(p);
                }
                Err(e) => tracing::warn!("代理配置失败,回退直连: {}", e),
            }
        }
        let http = builder.build().expect("无法构造 HTTP 客户端");
        Self {
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            http,
        }
    }

    /// 检测 Ollama 服务是否在运行
    pub async fn is_alive(&self) -> bool {
        let url = format!("{}/api/tags", self.base_url);
        self.http.get(&url).send().await.map(|r| r.status().is_success()).unwrap_or(false)
    }

    /// 列出已安装的模型
    pub async fn list_models(&self) -> AppResult<Vec<String>> {
        let url = format!("{}/api/tags", self.base_url);
        let resp = self.http.get(&url).send().await?;
        let body: TagsResponse = resp.json().await?;
        Ok(body.models.into_iter().map(|m| m.name).collect())
    }

    /// 检查模型是否已安装
    pub async fn has_model(&self, model: &str) -> AppResult<bool> {
        let models = self.list_models().await?;
        Ok(models.iter().any(|m| m == model || m.starts_with(&format!("{}:", model))))
    }

    /// 拉取模型(返回 true 表示触发拉取,实际进度通过 emit 事件推送)
    pub async fn pull_model(&self, model: &str) -> AppResult<()> {
        let url = format!("{}/api/pull", self.base_url);
        let req = serde_json::json!({ "name": model });
        // 不等待流式响应完成(可能耗时很长),由调用方用流式接口
        let resp = self.http.post(&url).json(&req).send().await?;
        if !resp.status().is_success() {
            return Err(AppError::Other(anyhow::anyhow!(
                "拉取模型失败: HTTP {}",
                resp.status()
            )));
        }
        Ok(())
    }

    /// 流式拉取模型,通过回调推送进度。支持取消令牌。
    pub async fn pull_model_stream<F>(
        &self,
        model: &str,
        cancel: CancellationToken,
        mut on_progress: F,
    ) -> AppResult<()>
    where
        F: FnMut(PullProgress),
    {
        use futures_util::StreamExt;
        let url = format!("{}/api/pull", self.base_url);
        let req = serde_json::json!({ "name": model, "stream": true });
        let resp = self.http.post(&url).json(&req).send().await?;
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    return Err(AppError::Other(anyhow::anyhow!("下载已取消")));
                }
                chunk = stream.next() => {
                    match chunk {
                        Some(Ok(bytes)) => {
                            buf.push_str(&String::from_utf8_lossy(&bytes));
                            while let Some(idx) = buf.find('\n') {
                                let line = buf[..idx].trim().to_string();
                                buf.drain(..=idx);
                                if line.is_empty() {
                                    continue;
                                }
                                if let Ok(p) = serde_json::from_str::<PullProgress>(&line) {
                                    on_progress(p.clone());
                                    if p.status == "success" {
                                        return Ok(());
                                    }
                                }
                            }
                        }
                        Some(Err(e)) => return Err(AppError::Http(e)),
                        None => return Ok(()),
                    }
                }
            }
        }
    }

    /// 生成文本(非流式)
    pub async fn generate(&self, req: &GenerateRequest) -> AppResult<GenerateResponse> {
        let url = format!("{}/api/generate", self.base_url);
        let resp = self.http.post(&url).json(req).send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Other(anyhow::anyhow!(
                "Ollama generate 失败: {}",
                text
            )));
        }
        let body: GenerateResponse = resp.json().await?;
        Ok(body)
    }

    /// 流式生成文本,通过回调推送每个 token
    /// 回调返回 false 可提前终止生成
    pub async fn generate_stream<F>(
        &self,
        req: &GenerateRequest,
        mut on_token: F,
    ) -> AppResult<GenerateResponse>
    where
        F: FnMut(&str) -> bool,
    {
        use futures_util::StreamExt;
        let url = format!("{}/api/generate", self.base_url);
        // 强制 stream=true
        let mut req_with_stream = req.clone();
        req_with_stream.stream = Some(true);
        let resp = self.http.post(&url).json(&req_with_stream).send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Other(anyhow::anyhow!(
                "Ollama generate 失败: {}",
                text
            )));
        }
        let mut stream = resp.bytes_stream();
        let mut buf = String::new();
        let mut full_response = String::new();
        let mut final_chunk: Option<GenerateResponse> = None;
        loop {
            match stream.next().await {
                Some(Ok(bytes)) => {
                    buf.push_str(&String::from_utf8_lossy(&bytes));
                    while let Some(idx) = buf.find('\n') {
                        let line = buf[..idx].trim().to_string();
                        buf.drain(..=idx);
                        if line.is_empty() {
                            continue;
                        }
                        match serde_json::from_str::<GenerateResponse>(&line) {
                            Ok(chunk) => {
                                if !chunk.response.is_empty() {
                                    full_response.push_str(&chunk.response);
                                    if !on_token(&chunk.response) {
                                        // 调用方请求终止
                                        return Ok(GenerateResponse {
                                            response: full_response,
                                            done: false,
                                            ..chunk
                                        });
                                    }
                                }
                                if chunk.done {
                                    final_chunk = Some(chunk);
                                }
                            }
                            Err(_) => continue,
                        }
                    }
                }
                Some(Err(e)) => return Err(AppError::Http(e)),
                None => break,
            }
        }
        let mut final_resp = final_chunk.unwrap_or(GenerateResponse {
            model: req.model.clone(),
            response: String::new(),
            done: true,
            context: None,
            total_duration: None,
            load_duration: None,
            prompt_eval_count: None,
            eval_count: None,
        });
        // done chunk 的 response 字段通常为空串,用累积的 full_response 补全
        if final_resp.response.is_empty() {
            final_resp.response = full_response;
        }
        Ok(final_resp)
    }

    /// 生成嵌入向量
    pub async fn embed(&self, req: &EmbeddingRequest) -> AppResult<EmbeddingResponse> {
        let url = format!("{}/api/embeddings", self.base_url);
        let resp = self.http.post(&url).json(req).send().await?;
        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AppError::Other(anyhow::anyhow!(
                "Ollama embeddings 失败: {}",
                text
            )));
        }
        let body: EmbeddingResponse = resp.json().await?;
        Ok(body)
    }

    /// 批量生成嵌入向量
    pub async fn embed_batch(
        &self,
        model: &str,
        texts: &[String],
    ) -> AppResult<Vec<Vec<f32>>> {
        // Ollama 没有原生批量接口,顺序请求(可优化为并发)
        let mut vectors = Vec::with_capacity(texts.len());
        for text in texts {
            let req = EmbeddingRequest {
                model: model.to_string(),
                prompt: text.clone(),
            };
            let resp = self.embed(&req).await?;
            vectors.push(resp.embedding);
        }
        Ok(vectors)
    }

    /// 卸载模型(keep_alive=0)
    pub async fn unload_model(&self, model: &str) -> AppResult<()> {
        let url = format!("{}/api/generate", self.base_url);
        let req = serde_json::json!({
            "model": model,
            "keep_alive": 0
        });
        let _ = self.http.post(&url).json(&req).send().await?;
        Ok(())
    }
}

// ============ 请求/响应结构 ============

#[derive(Debug, Clone, Serialize)]
pub struct GenerateRequest {
    pub model: String,
    pub prompt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<Vec<i32>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// qwen3 等推理模型是否启用 thinking;RAG 问答/切分场景置 false:
    /// thinking 输出在独立字段前端不可见,且与 response 共享 num_predict 额度,
    /// 会拖慢首 token 并截断正式回答
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<GenerateOptions>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct GenerateOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub num_predict: Option<i32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GenerateResponse {
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub response: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub context: Option<Vec<i32>>,
    #[serde(default)]
    pub total_duration: Option<u64>,
    #[serde(default)]
    pub load_duration: Option<u64>,
    #[serde(default)]
    pub prompt_eval_count: Option<u64>,
    #[serde(default)]
    pub eval_count: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingRequest {
    pub model: String,
    pub prompt: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingResponse {
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, Deserialize)]
struct TagsResponse {
    #[serde(default)]
    models: Vec<TagModel>,
}

#[derive(Debug, Clone, Deserialize)]
struct TagModel {
    name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PullProgress {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub digest: Option<String>,
    #[serde(default)]
    pub total: Option<u64>,
    #[serde(default)]
    pub completed: Option<u64>,
}

/// 速率与 ETA 计算上下文(按 model+digest 维度)
#[derive(Debug, Clone, Default)]
pub struct DownloadRateTracker {
    pub last_completed: u64,
    pub last_ts: Option<Instant>,
}

impl DownloadRateTracker {
    /// 返回 (bytes_per_sec, eta_secs_or_none)
    pub fn update(&mut self, completed: u64, total: Option<u64>) -> (f64, Option<u64>) {
        let now = Instant::now();
        let (rate, eta) = match self.last_ts {
            Some(last) if completed >= self.last_completed => {
                let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
                let diff = completed - self.last_completed;
                let rate = diff as f64 / elapsed;
                let eta = total.and_then(|t| {
                    if rate > 0.0 && completed < t {
                        Some(((t - completed) as f64 / rate) as u64)
                    } else {
                        None
                    }
                });
                (rate, eta)
            }
            _ => (0.0, None),
        };
        self.last_completed = completed;
        self.last_ts = Some(now);
        (rate, eta)
    }
}
