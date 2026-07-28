// Ollama 相关 Tauri Command

use super::client::{DownloadRateTracker, OllamaClientConfig};
use super::process::OllamaManager;
use super::{DEFAULT_EMBEDDING_MODEL};
use crate::config::settings::AppSettings;
use crate::config::SettingsState;
use crate::error::AppError;
use crate::models::system::SystemInfo;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

/// 全局 OllamaManager 单例 + 取消令牌管理
#[derive(Clone, Default)]
pub struct OllamaState {
    inner: Arc<OllamaStateInner>,
}

#[derive(Default)]
struct OllamaStateInner {
    manager: Mutex<Option<OllamaManager>>,
    /// 当前下载取消令牌(按 model 维度)
    cancel_tokens: Arc<Mutex<HashMap<String, CancellationToken>>>,
}

impl OllamaState {
    /// 用 settings 初始化(在 lib.rs setup 中调用)
    pub async fn init_with_settings(&self, settings: AppSettings) {
        let manager = build_manager(&settings);
        *self.inner.manager.lock().await = Some(manager);
    }

    pub async fn get(&self) -> OllamaManager {
        let mut guard = self.inner.manager.lock().await;
        if guard.is_none() {
            *guard = Some(OllamaManager::default());
        }
        guard.as_ref().unwrap().clone()
    }

    pub async fn register_cancel(&self, model: &str, token: CancellationToken) {
        self.inner
            .cancel_tokens
            .lock()
            .await
            .insert(model.to_string(), token);
    }

    pub async fn cancel(&self, model: &str) -> bool {
        if let Some(t) = self.inner.cancel_tokens.lock().await.remove(model) {
            t.cancel();
            true
        } else {
            false
        }
    }

    pub async fn remove_cancel(&self, model: &str) {
        self.inner.cancel_tokens.lock().await.remove(model);
    }
}

/// 用 settings 构造 OllamaManager
fn build_manager(settings: &AppSettings) -> OllamaManager {
    let cfg = OllamaClientConfig {
        base_url: settings.effective_ollama_url().to_string(),
        proxy: if settings.proxy_enabled() {
            Some(settings.https_proxy.clone())
        } else {
            None
        },
        connect_timeout: Duration::from_secs(settings.download_connect_timeout_secs),
        request_timeout: Duration::from_secs(settings.download_request_timeout_secs),
    };
    let client = super::OllamaClient::new_with_config(cfg);
    OllamaManager::new(client, settings.clone())
}

/// 进度事件 payload(增强版)
#[derive(serde::Serialize, Clone)]
pub struct DownloadProgressPayload {
    pub model: String,
    pub status: String, // pulling / success / error / cancelled / retrying
    pub completed: Option<u64>,
    pub total: Option<u64>,
    pub percent: Option<f64>, // 0.0 ~ 1.0
    pub rate_bps: Option<f64>, // 字节/秒
    pub eta_secs: Option<u64>,
    pub attempt: u32,
    pub max_attempts: u32,
    pub error: Option<String>,
}

/// 检查系统环境(应用启动时调用)
#[tauri::command]
pub async fn get_system_info(state: State<'_, OllamaState>) -> Result<SystemInfo, AppError> {
    let manager = state.get().await;
    let status = manager.detect_startup_state().await;
    Ok(SystemInfo {
        ollama_installed: status.installed,
        ollama_running: status.running,
        default_models_available: status.default_models_available,
    })
}

/// 安装 Ollama
#[tauri::command]
pub async fn install_ollama(state: State<'_, OllamaState>) -> Result<String, AppError> {
    let manager = state.get().await;
    manager.install().await
}

/// 启动 Ollama 服务(若未运行)
#[tauri::command]
pub async fn start_ollama(state: State<'_, OllamaState>) -> Result<(), AppError> {
    let manager = state.get().await;
    manager.start().await
}

/// 规范化 Ollama pull 状态字符串
/// Ollama 会返回 "pulling 3d0b790534fe"(含 layer digest),前端只需看到 "pulling"
fn normalize_pull_status(status: &str) -> String {
    if status.starts_with("pulling ") {
        "pulling".to_string()
    } else {
        status.to_string()
    }
}

/// 进度事件节流间隔(毫秒)
const PROGRESS_THROTTLE_MS: u128 = 500;

/// 重试下载单模型(指数退避 1/2/4s)
async fn pull_with_retry(
    app: AppHandle,
    client: super::OllamaClient,
    model: String,
    max_attempts: u32,
    cancel: CancellationToken,
) -> Result<(), AppError> {
    use std::time::Instant;
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let mut tracker = DownloadRateTracker::default();
        let mut last_emit_ts: Option<Instant> = None;
        let app_c = app.clone();
        let model_c = model.clone();
        let attempt_c = attempt;
        let max_c = max_attempts;

        let result = client
            .pull_model_stream(&model, cancel.clone(), move |p| {
                let normalized = normalize_pull_status(&p.status);
                let now = Instant::now();
                // 节流:仅在距上次发送 >= 500ms,或状态终结(success)时发送
                let should_emit = normalized == "success"
                    || last_emit_ts
                        .map(|t| now.duration_since(t).as_millis() >= PROGRESS_THROTTLE_MS)
                        .unwrap_or(true);
                if !should_emit {
                    return;
                }
                last_emit_ts = Some(now);

                let percent = match (p.completed, p.total) {
                    (Some(c), Some(t)) if t > 0 => Some(c as f64 / t as f64),
                    _ => None,
                };
                let (rate, eta) = match (p.completed, p.total) {
                    (Some(c), t_opt) => tracker.update(c, t_opt),
                    _ => (0.0, None),
                };
                let _ = app_c.emit(
                    "model-download-progress",
                    DownloadProgressPayload {
                        model: model_c.clone(),
                        status: normalized,
                        completed: p.completed,
                        total: p.total,
                        percent,
                        rate_bps: if rate > 0.0 { Some(rate) } else { None },
                        eta_secs: eta,
                        attempt: attempt_c,
                        max_attempts: max_c,
                        error: None,
                    },
                );
            })
            .await;

        match result {
            Ok(()) => return Ok(()),
            Err(e) if cancel.is_cancelled() => {
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgressPayload {
                        model: model.clone(),
                        status: "cancelled".into(),
                        completed: None,
                        total: None,
                        percent: None,
                        rate_bps: None,
                        eta_secs: None,
                        attempt,
                        max_attempts,
                        error: Some("用户取消".into()),
                    },
                );
                return Err(e);
            }
            Err(e) if attempt >= max_attempts => {
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgressPayload {
                        model: model.clone(),
                        status: "error".into(),
                        completed: None,
                        total: None,
                        percent: None,
                        rate_bps: None,
                        eta_secs: None,
                        attempt,
                        max_attempts,
                        error: Some(e.to_string()),
                    },
                );
                return Err(e);
            }
            Err(e) => {
                // 退避:1s, 2s, 4s, ...
                let backoff = Duration::from_secs(1u64 << (attempt - 1));
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgressPayload {
                        model: model.clone(),
                        status: "retrying".into(),
                        completed: None,
                        total: None,
                        percent: None,
                        rate_bps: None,
                        eta_secs: None,
                        attempt,
                        max_attempts,
                        error: Some(format!(
                            "第 {} 次失败,{}秒后重试: {}",
                            attempt,
                            backoff.as_secs(),
                            e
                        )),
                    },
                );
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = cancel.cancelled() => return Err(AppError::Other(anyhow::anyhow!("下载已取消"))),
                }
            }
        }
    }
}

/// 下载默认模型(chat + embedding),通过事件推送进度
#[tauri::command]
pub async fn download_default_models(
    app: AppHandle,
    state: State<'_, OllamaState>,
) -> Result<(), AppError> {
    let manager = state.get().await;
    // 启动 Ollama(若未运行)
    manager.start().await?;
    let client = manager.client_clone();
    let settings = manager.settings_snapshot();
    let max_attempts = settings.download_max_retries.max(1);

    // 对话模型取 settings 中的生效值(用户可在设置中切换候选模型)
    let chat_model = super::effective_chat_model(&settings);
    let models = [chat_model.as_str(), DEFAULT_EMBEDDING_MODEL];
    let mut handles = Vec::new();

    for model in models {
        // 预检查:已安装模型直接 emit "installed" 状态,跳过下载
        match client.has_model(model).await {
            Ok(true) => {
                let _ = app.emit(
                    "model-download-progress",
                    DownloadProgressPayload {
                        model: model.to_string(),
                        status: "installed".into(),
                        completed: None,
                        total: None,
                        percent: Some(1.0),
                        rate_bps: None,
                        eta_secs: None,
                        attempt: 0,
                        max_attempts,
                        error: None,
                    },
                );
                tracing::info!("模型 {} 已安装,跳过下载", model);
                continue;
            }
            Ok(false) => {}
            Err(e) => {
                tracing::warn!("检查模型 {} 是否已安装失败: {},将尝试下载", model, e);
            }
        }

        let token = CancellationToken::new();
        state.register_cancel(model, token.clone()).await;
        let app_c = app.clone();
        let client_c = client.clone();
        let model_s = model.to_string();
        let handle = tokio::spawn(async move {
            pull_with_retry(app_c, client_c, model_s.clone(), max_attempts, token).await
        });
        handles.push((model.to_string(), handle));
    }

    // 并发等待所有模型完成,任一失败则返回错误
    let mut first_err: Option<AppError> = None;
    for (model, h) in handles {
        match h.await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                if first_err.is_none() {
                    first_err = Some(e);
                }
            }
            Err(e) => {
                if first_err.is_none() {
                    first_err = Some(AppError::Other(anyhow::anyhow!("任务异常: {}", e)));
                }
            }
        }
        state.remove_cancel(&model).await;
    }

    match first_err {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// 取消指定模型下载
#[tauri::command]
pub async fn cancel_model_download(
    model: String,
    state: State<'_, OllamaState>,
) -> Result<bool, AppError> {
    Ok(state.cancel(&model).await)
}

/// 读取应用设置
#[tauri::command]
pub async fn get_app_settings(
    settings: State<'_, SettingsState>,
) -> Result<AppSettings, AppError> {
    Ok(settings.get().await)
}

/// 更新应用设置(持久化 + 重建 OllamaState)
#[tauri::command]
pub async fn update_app_settings(
    settings: State<'_, SettingsState>,
    ollama_state: State<'_, OllamaState>,
    req: AppSettings,
) -> Result<(), AppError> {
    // 1. 持久化
    settings.update(req.clone()).await?;
    // 2. 重建 OllamaManager(应用新 URL/代理/超时)
    ollama_state.init_with_settings(req).await;
    Ok(())
}

/// 测试下载源连通性
#[derive(serde::Deserialize)]
pub struct TestSourceRequest {
    pub registry: Option<String>,
    pub proxy: Option<String>,
}

#[derive(serde::Serialize)]
pub struct TestSourceResult {
    pub ok: bool,
    pub latency_ms: u64,
    pub message: String,
}

/// 测试下载源/代理连通性
#[tauri::command]
pub async fn test_download_source(req: TestSourceRequest) -> Result<TestSourceResult, AppError> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(10));
    if let Some(p) = req.proxy.as_ref().filter(|s| !s.is_empty()) {
        if let Ok(proxy) = reqwest::Proxy::all(p) {
            builder = builder.proxy(proxy);
        }
    }
    let client = builder.build().map_err(|e| AppError::Other(anyhow::anyhow!(e)))?;

    // 默认探测 ollama registry,或镜像源根路径
    let url = req
        .registry
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or("https://registry.ollama.ai");
    let start = std::time::Instant::now();
    let resp = client.get(url).send().await;
    let latency_ms = start.elapsed().as_millis() as u64;

    match resp {
        Ok(r) if r.status().is_success() || r.status().is_redirection() => Ok(TestSourceResult {
            ok: true,
            latency_ms,
            message: format!("HTTP {}", r.status()),
        }),
        Ok(r) => Ok(TestSourceResult {
            ok: false,
            latency_ms,
            message: format!("HTTP {}", r.status()),
        }),
        Err(e) => Ok(TestSourceResult {
            ok: false,
            latency_ms,
            message: e.to_string(),
        }),
    }
}

/// 手动触发资源释放(也可由 Tauri 退出事件调用)
#[tauri::command]
pub async fn shutdown_cleanup(
    state: State<'_, OllamaState>,
    force_ollama_stop: bool,
) -> Result<Vec<String>, AppError> {
    let manager = state.get().await;
    manager.shutdown(force_ollama_stop).await
}

/// 列出本机已安装的 Ollama 对话模型(供切分策略等下拉选择)
/// Ollama 未运行时返回空列表而非报错,由前端决定提示
///
/// 注意:嵌入模型(nomic-embed-text 等)不支持 /api/generate,需过滤。
/// /api/tags 不返回模型能力信息,只能按名称特征排除常见嵌入模型。
#[tauri::command]
pub async fn list_local_models(state: State<'_, OllamaState>) -> Result<Vec<String>, AppError> {
    let manager = state.get().await;
    if !manager.is_running().await {
        return Ok(vec![]);
    }
    let models = manager.client_clone().list_models().await?;
    Ok(models
        .into_iter()
        .filter(|m| !is_embedding_model_name(m))
        .collect())
}

/// 模型存在性检查结果
#[derive(serde::Serialize)]
pub struct ModelsOnDisk {
    /// 当前生效的对话模型名(settings.chat_model 或默认)
    pub chat_model: String,
    /// 生效对话模型是否已在磁盘
    pub chat_model_installed: bool,
    /// 嵌入模型(锁定)是否已在磁盘
    pub embedding_model_installed: bool,
    pub all_installed: bool,
    /// 文件系统层发现的全部对话模型(已按名称特征过滤嵌入模型)
    pub local_chat_models: Vec<String>,
    /// 被扫描的候选目录
    pub scanned_dirs: Vec<String>,
}

/// 获取推荐对话模型候选表(静态配置)
#[tauri::command]
pub fn get_recommended_chat_models() -> Vec<super::RecommendedChatModel> {
    super::RECOMMENDED_CHAT_MODELS.to_vec()
}

/// 名称特征判断是否为嵌入模型(不支持 generate)
fn is_embedding_model_name(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("embed") || n.starts_with("bge") || n.contains("minilm") || n.starts_with("e5")
}

/// 归一化模型引用:无 tag 时补 ":latest"
/// (Ollama 对 `ollama pull nomic-embed-text` 存储的 manifest tag 为 latest,
/// 与 has_model 的容错比较语义保持一致)
fn normalize_model_ref(model: &str) -> String {
    if model.contains(':') {
        model.to_string()
    } else {
        format!("{}:latest", model)
    }
}

/// 在文件系统层检查模型下载状态(不依赖 Ollama 服务运行)
///
/// Ollama 模型存储结构:
///   `<models_dir>/manifests/<registry>/<namespace>/<name>/<tag>`
/// 本函数扫描 manifests 目录:
///   - 判断生效对话模型/嵌入模型是否存在
///   - 收集全部已下载的对话模型名(供设置页候选)
#[tauri::command]
pub async fn check_models_on_disk(
    settings: State<'_, SettingsState>,
) -> Result<ModelsOnDisk, AppError> {
    use std::path::Path;

    /// 候选 models 目录:默认 ~/.ollama/models,自定义 ~/Documents/SimpleThinkTank/models
    fn candidate_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".ollama").join("models"));
        }
        dirs.push(crate::config::AppPaths::models_dir());
        dirs
    }

    /// 扫描 manifests 收集全部模型名(name:tag)
    /// 结构固定为 4 层: manifests/<registry>/<namespace>/<name>/<tag>
    fn collect_models(manifests: &Path, out: &mut Vec<String>) {
        let Ok(registries) = std::fs::read_dir(manifests) else {
            return;
        };
        for reg in registries.flatten() {
            let Ok(namespaces) = std::fs::read_dir(reg.path()) else {
                continue;
            };
            for ns in namespaces.flatten() {
                let Ok(names) = std::fs::read_dir(ns.path()) else {
                    continue;
                };
                for name_ent in names.flatten() {
                    let name = name_ent.file_name();
                    let Some(name) = name.to_str() else { continue };
                    let Ok(tags) = std::fs::read_dir(name_ent.path()) else {
                        continue;
                    };
                    for tag_ent in tags.flatten() {
                        if tag_ent.path().is_file() {
                            if let Some(tag) = tag_ent.file_name().to_str() {
                                out.push(format!("{}:{}", name, tag));
                            }
                        }
                    }
                }
            }
        }
    }

    let dirs = candidate_dirs();
    let scanned_dirs: Vec<String> = dirs
        .iter()
        .filter_map(|d| d.to_str().map(|s| s.to_string()))
        .collect();

    // 汇总所有候选目录下的模型(去重)
    let mut all_models: Vec<String> = Vec::new();
    for dir in &dirs {
        let manifests = dir.join("manifests");
        if manifests.is_dir() {
            collect_models(&manifests, &mut all_models);
        }
    }
    all_models.sort();
    all_models.dedup();

    let settings = settings.get().await;
    let chat_model = super::effective_chat_model(&settings);
    let chat_ref = normalize_model_ref(&chat_model);
    let emb_ref = normalize_model_ref(super::DEFAULT_EMBEDDING_MODEL);
    let chat_installed = all_models.iter().any(|m| m == &chat_ref);
    let emb_installed = all_models.iter().any(|m| m == &emb_ref);
    let local_chat_models: Vec<String> = all_models
        .into_iter()
        .filter(|m| !is_embedding_model_name(m))
        .collect();

    Ok(ModelsOnDisk {
        chat_model,
        chat_model_installed: chat_installed,
        embedding_model_installed: emb_installed,
        all_installed: chat_installed && emb_installed,
        local_chat_models,
        scanned_dirs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_model_ref() {
        // 无 tag 补 latest
        assert_eq!(normalize_model_ref("nomic-embed-text"), "nomic-embed-text:latest");
        // 已有 tag 保持不变
        assert_eq!(normalize_model_ref("qwen3:1.7b"), "qwen3:1.7b");
        assert_eq!(normalize_model_ref("nomic-embed-text:latest"), "nomic-embed-text:latest");
    }

    #[test]
    fn test_embedding_model_match_with_implicit_latest() {
        // 回归:ollama pull nomic-embed-text 落盘的 manifest 为 nomic-embed-text:latest,
        // 精确比较会漏判为未安装
        let scanned = vec!["qwen3:1.7b".to_string(), "nomic-embed-text:latest".to_string()];
        let emb_ref = normalize_model_ref(super::DEFAULT_EMBEDDING_MODEL);
        assert!(scanned.iter().any(|m| m == &emb_ref));
        // 嵌入模型不得进入对话模型候选
        let chat_candidates: Vec<&String> = scanned
            .iter()
            .filter(|m| !is_embedding_model_name(m))
            .collect();
        assert_eq!(chat_candidates, vec!["qwen3:1.7b"]);
    }
}
