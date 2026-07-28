// Ollama 进程管理
// 负责检测、启动、关闭 Ollama 进程,并跟踪进程归属

use crate::config::settings::AppSettings;
use crate::config::AppPaths;
use crate::error::{AppError, AppResult};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Ollama 运行状态
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct OllamaStatus {
    pub installed: bool,
    pub running: bool,
    pub default_models_available: bool,
    pub managed_by_app: bool,
}

/// 进程归属跟踪
#[derive(Debug, Default)]
struct ProcessOwnership {
    /// 应用启动前 Ollama 是否已经在运行
    pre_existing: bool,
    /// 应用是否曾主动启动过 Ollama
    app_started: bool,
    /// 最后一次启动时间
    started_at: Option<Instant>,
}

/// Windows 创建子进程标志:不创建可见控制台窗口(GUI 应用启动控制台程序时避免弹 cmd 窗口)
/// 注意:CREATE_NO_WINDOW 与 DETACHED_PROCESS 互斥,不可同用(MSDN)
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x08000000;
#[cfg(windows)]
const CREATE_NEW_PROCESS_GROUP: u32 = 0x00000200;

static OWNERSHIP: Mutex<Option<ProcessOwnership>> = Mutex::new(None);

/// Ollama 进程管理器
#[derive(Debug, Clone)]
pub struct OllamaManager {
    client: super::OllamaClient,
    settings: AppSettings,
}

impl Default for OllamaManager {
    fn default() -> Self {
        Self::new(super::OllamaClient::default(), AppSettings::default())
    }
}

impl OllamaManager {
    pub fn new(client: super::OllamaClient, settings: AppSettings) -> Self {
        Self { client, settings }
    }

    /// 获取内部 settings 快照
    pub fn settings_snapshot(&self) -> AppSettings {
        self.settings.clone()
    }

    /// 克隆出 client
    pub fn client_clone(&self) -> super::OllamaClient {
        self.client.clone()
    }

    /// 检测 ollama 命令是否在 PATH 中
    pub fn is_installed(&self) -> bool {
        which_ollama().is_some()
    }

    /// 检测 Ollama 服务是否在运行
    pub async fn is_running(&self) -> bool {
        self.client.is_alive().await
    }

    /// 检测默认模型是否已安装(对话模型取 settings 中的生效值)
    pub async fn default_models_available(&self) -> bool {
        let chat_model = super::effective_chat_model(&self.settings);
        let chat = self
            .client
            .has_model(&chat_model)
            .await
            .unwrap_or(false);
        let emb = self
            .client
            .has_model(super::DEFAULT_EMBEDDING_MODEL)
            .await
            .unwrap_or(false);
        chat && emb
    }

    /// 获取状态(应用启动时调用,记录进程归属)
    pub async fn detect_startup_state(&self) -> OllamaStatus {
        let installed = self.is_installed();
        let running = self.is_running().await;
        let default_models_available = if running {
            self.default_models_available().await
        } else {
            false
        };

        // 记录启动前的运行状态
        let mut guard = OWNERSHIP.lock().unwrap();
        *guard = Some(ProcessOwnership {
            pre_existing: running,
            app_started: false,
            started_at: None,
        });

        OllamaStatus {
            installed,
            running,
            default_models_available,
            managed_by_app: false,
        }
    }

    /// 启动 Ollama 进程(若未运行)
    pub async fn start(&self) -> AppResult<()> {
        if self.is_running().await {
            return Ok(());
        }

        let exe = which_ollama().ok_or_else(|| {
            AppError::Other(anyhow::anyhow!(
                "未找到 ollama 可执行文件,请先安装"
            ))
        })?;

        // 启动 ollama serve(后台进程)
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("serve");
        cmd.stdin(std::process::Stdio::null());
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());

        // === 注入环境变量(关键改造点)===
        // 1. 监听地址固定本机
        cmd.env("OLLAMA_HOST", "127.0.0.1:11434");
        // 2. CORS 来源(允许 Tauri 前端直接调用)
        cmd.env("OLLAMA_ORIGINS", "*");
        // 3. 镜像源:OLLAMA_REGISTRY(Ollama 0.5+ 支持)
        if self.settings.registry_enabled() {
            cmd.env("OLLAMA_REGISTRY", &self.settings.ollama_registry);
            tracing::info!("注入 OLLAMA_REGISTRY={}", self.settings.ollama_registry);
        }
        // 4. 进程级 HTTPS 代理(影响 Ollama 服务本身的下载请求)
        if self.settings.proxy_enabled() {
            cmd.env("HTTPS_PROXY", &self.settings.https_proxy);
            cmd.env("HTTP_PROXY", &self.settings.https_proxy);
            // 关闭对 localhost 的代理(应用自身访问 Ollama 不走代理)
            cmd.env("NO_PROXY", "127.0.0.1,localhost");
            tracing::info!("注入 HTTPS_PROXY={}", self.settings.https_proxy);
        }
        // 5. 模型存储目录(指向 AppPaths::models_dir)
        if self.settings.use_custom_models_dir {
            let models_dir = AppPaths::models_dir();
            if let Some(s) = models_dir.to_str() {
                cmd.env("OLLAMA_MODELS", s);
                tracing::info!("注入 OLLAMA_MODELS={}", s);
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            unsafe {
                cmd.pre_exec(|| {
                    libc::setsid();
                    Ok(())
                });
            }
        }
        #[cfg(windows)]
        {
            // 隐藏 serve 进程的控制台窗口;DETACHED_PROCESS 实测仍弹窗,换 CREATE_NO_WINDOW
            cmd.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
        }

        let _child = cmd.spawn().map_err(|e| {
            AppError::Other(anyhow::anyhow!("启动 ollama 失败: {}", e))
        })?;

        // 记录应用启动了进程
        {
            let mut guard = OWNERSHIP.lock().unwrap();
            if let Some(o) = guard.as_mut() {
                o.app_started = true;
                o.started_at = Some(Instant::now());
            }
        }

        // 等待服务就绪(最多 30 秒)
        for _ in 0..60 {
            if self.client.is_alive().await {
                tracing::info!("Ollama 服务已就绪");
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Err(AppError::Other(anyhow::anyhow!(
            "Ollama 服务在 30 秒内未就绪"
        )))
    }

    /// 安装 Ollama(Windows:下载安装包)
    pub async fn install(&self) -> AppResult<String> {
        #[cfg(target_os = "windows")]
        {
            return self.install_windows().await;
        }
        #[cfg(not(target_os = "windows"))]
        {
            Err(AppError::Other(anyhow::anyhow!(
                "当前平台请手动安装 Ollama: https://ollama.com/download"
            )))
        }
    }

    #[cfg(target_os = "windows")]
    async fn install_windows(&self) -> AppResult<String> {
        // 通道 1(优先): winget —— Win10 1809+/Win11 自带,官方源,同步等待装完
        match install_via_winget().await {
            Ok(()) => {
                wait_install_detected().await?;
                return Ok("已通过 winget 静默安装 Ollama,请点击\"启动 Ollama\"".to_string());
            }
            Err(e) => {
                tracing::warn!("winget 通道不可用,回退官网安装包: {}", e);
            }
        }

        // 通道 2(回退): 官网安装包 + Inno Setup 静默参数
        let installer = download_installer().await?;
        run_silent_installer(&installer).await?;
        wait_install_detected().await?;
        Ok("已通过官方安装包静默安装 Ollama,请点击\"启动 Ollama\"".to_string())
    }

    /// 退出清理:卸载模型,根据进程归属决定是否终止进程
    pub async fn shutdown(&self, force_stop: bool) -> AppResult<Vec<String>> {
        let mut steps = Vec::new();

        if self.is_running().await {
            let chat_model = super::effective_chat_model(&self.settings);
            if let Err(e) = self.client.unload_model(&chat_model).await {
                tracing::warn!("卸载对话模型失败: {}", e);
            } else {
                steps.push(format!("已卸载模型: {}", chat_model));
            }
            if let Err(e) = self
                .client
                .unload_model(super::DEFAULT_EMBEDDING_MODEL)
                .await
            {
                tracing::warn!("卸载嵌入模型失败: {}", e);
            } else {
                steps.push(format!("已卸载模型: {}", super::DEFAULT_EMBEDDING_MODEL));
            }
        } else {
            steps.push("Ollama 未运行,跳过模型卸载".to_string());
        }

        let should_stop = {
            let guard = OWNERSHIP.lock().unwrap();
            match guard.as_ref() {
                Some(o) => force_stop || (o.app_started && !o.pre_existing),
                None => force_stop,
            }
        };

        if should_stop && self.is_running().await {
            match stop_ollama_process().await {
                Ok(()) => steps.push("已终止 Ollama 进程".to_string()),
                Err(e) => steps.push(format!("终止 Ollama 进程失败: {}", e)),
            }
        } else if !should_stop {
            steps.push("Ollama 为用户独立运行,不终止进程".to_string());
        }

        Ok(steps)
    }
}

/// 在 PATH 中查找 ollama 可执行文件,失败时探测默认安装路径
///
/// 静默安装(winget / Inno Setup)会更新系统 PATH,但当前进程的 PATH 是
/// 启动时的快照,不会自动刷新,因此必须补充默认安装路径探测。
fn which_ollama() -> Option<PathBuf> {
    let exe_name = if cfg!(windows) { "ollama.exe" } else { "ollama" };
    let from_path = std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(exe_name);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    });
    if from_path.is_some() {
        return from_path;
    }
    default_install_paths().into_iter().find(|p| p.is_file())
}

/// 各平台默认安装路径候选
fn default_install_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        // 官方安装包(Inno Setup)per-user 安装到 %LOCALAPPDATA%\Programs\Ollama
        if let Some(local) = dirs::data_local_dir() {
            v.push(local.join("Programs").join("Ollama").join("ollama.exe"));
        }
    }
    #[cfg(unix)]
    {
        v.push(PathBuf::from("/usr/local/bin/ollama"));
        #[cfg(not(target_os = "macos"))]
        v.push(PathBuf::from("/usr/bin/ollama"));
    }
    v
}

/// 通过 winget 静默安装 Ollama(同步等待安装完成)
#[cfg(target_os = "windows")]
async fn install_via_winget() -> AppResult<()> {
    let mut cmd = tokio::process::Command::new("winget");
    cmd.args([
        "install",
        "-e",
        "--id",
        "Ollama.Ollama",
        "--silent",
        "--accept-package-agreements",
        "--accept-source-agreements",
        "--disable-interactivity",
    ]);
    // winget 是控制台程序,GUI 进程直接拉起会弹 cmd 窗口
    cmd.creation_flags(CREATE_NO_WINDOW);
    let output = cmd
        .output()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("winget 不可用: {}", e)))?;
    if output.status.success() {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(AppError::Other(anyhow::anyhow!(
            "winget 安装失败(exit {:?}): {} {}",
            output.status.code(),
            stdout.chars().take(200).collect::<String>(),
            stderr.chars().take(200).collect::<String>()
        )))
    }
}

/// 流式下载 Ollama 官方安装包到临时目录(避免 ~1GB 全量缓冲在内存)
#[cfg(target_os = "windows")]
async fn download_installer() -> AppResult<PathBuf> {
    use futures_util::StreamExt;
    use tokio::io::AsyncWriteExt;

    let url = "https://ollama.com/download/OllamaSetup.exe";
    let resp = reqwest::get(url).await.map_err(|e| {
        AppError::Other(anyhow::anyhow!("下载 Ollama 安装包失败: {}", e))
    })?;
    if !resp.status().is_success() {
        return Err(AppError::Other(anyhow::anyhow!(
            "下载 Ollama 安装包失败: HTTP {}",
            resp.status()
        )));
    }

    let tmp = std::env::temp_dir().join("OllamaSetup.exe");
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            AppError::Other(anyhow::anyhow!("下载 Ollama 安装包中断: {}", e))
        })?;
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    tracing::info!("Ollama 安装包已下载: {}", tmp.display());
    Ok(tmp)
}

/// 以 Inno Setup 静默参数运行安装包(同步等待安装完成)
#[cfg(target_os = "windows")]
async fn run_silent_installer(installer: &std::path::Path) -> AppResult<()> {
    let status = tokio::process::Command::new(installer)
        .args(["/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART"])
        .status()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("启动安装程序失败: {}", e)))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Other(anyhow::anyhow!(
            "安装程序退出码异常: {:?}",
            status.code()
        )))
    }
}

/// 安装完成后轮询探测 ollama 可执行文件
///
/// 安装器进程退出即代表文件已落盘,此处轮询只是保险;
/// 依赖 which_ollama 的默认路径探测,不依赖当前进程已过期的 PATH 快照。
#[cfg(target_os = "windows")]
async fn wait_install_detected() -> AppResult<()> {
    for _ in 0..30 {
        if which_ollama().is_some() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
    Err(AppError::Other(anyhow::anyhow!(
        "安装后未检测到 ollama 可执行文件,请重启应用后重试"
    )))
}

/// 停止 Ollama 进程
async fn stop_ollama_process() -> AppResult<()> {
    #[cfg(windows)]
    {
        let mut cmd = tokio::process::Command::new("taskkill");
        cmd.args(["/IM", "ollama.exe", "/F"]);
        // taskkill 是控制台程序,退出清理时避免闪 cmd 窗口
        cmd.creation_flags(CREATE_NO_WINDOW);
        cmd.output()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("taskkill 失败: {}", e)))?;
        tokio::time::sleep(Duration::from_secs(1)).await;
        Ok(())
    }
    #[cfg(unix)]
    {
        let out = tokio::process::Command::new("pgrep")
            .args(["-f", "ollama serve"])
            .output()
            .await;
        if let Ok(out) = out {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for pid_str in stdout.lines() {
                if let Ok(pid) = pid_str.parse::<i32>() {
                    unsafe {
                        libc::kill(pid, libc::SIGTERM);
                    }
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
        let _ = tokio::process::Command::new("pkill")
            .args(["-9", "-f", "ollama serve"])
            .output()
            .await;
        Ok(())
    }
}
