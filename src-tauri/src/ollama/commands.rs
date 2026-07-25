// Ollama 相关 Tauri Command

use super::client::{DownloadRateTracker, OllamaClientConfig};
use super::process::OllamaManager;
use super::{DEFAULT_CHAT_MODEL, DEFAULT_EMBEDDING_MODEL};
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

    let models = [DEFAULT_CHAT_MODEL, DEFAULT_EMBEDDING_MODEL];
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
    /// 名称特征判断是否为嵌入模型(不支持 generate)
    fn is_embedding_model(name: &str) -> bool {
        let n = name.to_lowercase();
        n.contains("embed") || n.starts_with("bge") || n.contains("minilm") || n.starts_with("e5")
    }

    let manager = state.get().await;
    if !manager.is_running().await {
        return Ok(vec![]);
    }
    let models = manager.client_clone().list_models().await?;
    Ok(models.into_iter().filter(|m| !is_embedding_model(m)).collect())
}

/// 模型存在性检查结果(按模型名)
#[derive(serde::Serialize)]
pub struct ModelsOnDisk {
    pub qwen3_1_7b: bool,
    pub nomic_embed_text: bool,
    pub all_installed: bool,
    /// 被扫描的候选目录
    pub scanned_dirs: Vec<String>,
}

/// 在文件系统层检查默认模型是否已下载(不依赖 Ollama 服务运行)
///
/// Ollama 模型存储结构:
///   `<models_dir>/manifests/<registry>/<namespace>/<name>/<tag>`
/// 本函数递归扫描 manifests 目录,匹配 model_name 对应的 manifest 文件。
#[tauri::command]
pub async fn check_models_on_disk() -> Result<ModelsOnDisk, AppError> {
    use std::path::Path;

    /// 候选 models 目录:默认 ~/.ollama/models,自定义 ~/Documents/Zhishiku/models
    fn candidate_dirs() -> Vec<std::path::PathBuf> {
        let mut dirs = Vec::new();
        if let Some(home) = dirs::home_dir() {
            dirs.push(home.join(".ollama").join("models"));
        }
        dirs.push(crate::config::AppPaths::models_dir());
        dirs
    }

    /// 在 manifests 目录递归查找 `<name>/<tag>` 文件
    fn find_manifest(dir: &Path, name: &str, tag: &str) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // 当前目录名匹配 name 时,检查同级是否有 <tag> 文件
                if path.file_name().and_then(|n| n.to_str()) == Some(name) {
                    let tag_file = path.join(tag);
                    if tag_file.is_file() {
                        return true;
                    }
                }
                // 否则继续递归
                if find_manifest(&path, name, tag) {
                    return true;
                }
            }
        }
        false
    }

    /// 检查单个模型在任一候选目录是否存在
    fn model_exists(model_name: &str) -> bool {
        let (name, tag) = match model_name.split_once(':') {
            Some((n, t)) => (n, t),
            None => (model_name, "latest"),
        };
        for dir in candidate_dirs() {
            let manifests = dir.join("manifests");
            if manifests.is_dir() && find_manifest(&manifests, name, tag) {
                return true;
            }
        }
        false
    }

    let scanned_dirs = candidate_dirs()
        .into_iter()
        .filter_map(|d| d.to_str().map(|s| s.to_string()))
        .collect();

    let qwen = model_exists(super::DEFAULT_CHAT_MODEL);
    let nomic = model_exists(super::DEFAULT_EMBEDDING_MODEL);
    Ok(ModelsOnDisk {
        qwen3_1_7b: qwen,
        nomic_embed_text: nomic,
        all_installed: qwen && nomic,
        scanned_dirs,
    })
}
