// 智识库 - 本地私有化 RAG 桌面应用
// 主库入口，Tauri 应用启动点

mod commands;
mod chunking;
mod config;
mod db;
mod error;
mod models;
mod ollama;
mod parsing;
mod vectorstore;
mod rag;

use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// 应用初始化
fn init_logging() -> tracing_appender::non_blocking::WorkerGuard {
    let log_dir = config::AppPaths::logs_dir();
    std::fs::create_dir_all(&log_dir).ok();

    let file_appender = tracing_appender::rolling::daily(&log_dir, "simplethinktank.log");
    let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,simplethinktank_lib=debug"));

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt::layer().with_target(true))
        .with(fmt::layer().with_writer(non_blocking).with_ansi(false))
        .init();

    guard
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _guard = init_logging();
    tracing::info!("智识库应用启动中...");

    // 初始化数据库
    let db_path = config::AppPaths::database_path();
    let db_url = format!("sqlite://{}?mode=rwc", db_path.display());
    tracing::info!("数据库路径: {}", db_url);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("无法创建 Tokio 运行时");

    let (pool, settings) = rt
        .block_on(async {
            let pool = db::init_database(&db_url).await.expect("数据库初始化失败");
            let dao = config::settings::SettingsDao::new(&pool);
            let settings = dao.load_all().await.unwrap_or_default();
            (pool, settings)
        });

    tracing::info!("数据库初始化完成,设置: {:?}", settings);

    // 提前构造 OllamaState 并注入 settings
    let ollama_state = ollama::commands::OllamaState::default();
    {
        let s = settings.clone();
        let ollama_state_clone = ollama_state.clone();
        rt.block_on(async move {
            ollama_state_clone.init_with_settings(s).await;
        });
    }

    let settings_state = config::SettingsState::new(pool.clone(), settings);

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(db::DbState(pool))
        .manage(ollama_state)
        .manage(settings_state)
        .setup(|app| {
            tracing::info!("Tauri 应用设置完成");
            // 启动时确保目录存在
            if let Err(e) = config::AppPaths::ensure_dirs() {
                tracing::warn!("创建应用目录失败: {}", e);
            }
            // 启动时检测 Ollama 状态并记录进程归属
            let app_handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                use tauri::Manager;
                if let Some(state) = app_handle.try_state::<ollama::commands::OllamaState>() {
                    let manager = state.get().await;
                    let _ = manager.detect_startup_state().await;
                }
            });
            Ok(())
        })
        .on_window_event(|_window, _event| {
            // 监听关闭事件以触发清理(实际清理在 RunEvent::ExitRequested)
        })
        .invoke_handler(tauri::generate_handler![
            // 知识库管理
            commands::knowledge_base::get_knowledge_bases,
            commands::knowledge_base::create_knowledge_base,
            commands::knowledge_base::rename_knowledge_base,
            commands::knowledge_base::delete_knowledge_base,
            // 文档管理
            commands::documents::import_documents,
            commands::documents::get_documents,
            commands::documents::get_import_task_progress,
            commands::documents::cancel_import,
            commands::documents::delete_document,
            // 切分策略
            commands::chunking::get_split_strategies,
            commands::chunking::preview_split,
            // RAG 对话
            commands::rag::query_knowledge_base,
            commands::rag::query_knowledge_base_stream,
            commands::rag::get_conversations,
            commands::rag::save_conversation,
            commands::rag::delete_conversation,
            commands::rag::get_messages,
            // Ollama 与系统
            ollama::commands::get_system_info,
            ollama::commands::install_ollama,
            ollama::commands::start_ollama,
            ollama::commands::download_default_models,
            ollama::commands::cancel_model_download,
            ollama::commands::get_app_settings,
            ollama::commands::update_app_settings,
            ollama::commands::test_download_source,
            ollama::commands::shutdown_cleanup,
            ollama::commands::check_models_on_disk,
            ollama::commands::list_local_models,
            ollama::commands::get_recommended_chat_models,
            // 存储统计
            commands::stats::get_storage_stats,
            commands::stats::clear_logs,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用时出错");

    // 运行应用,并在退出时执行清理
    app.run(|app_handle, event| {
        if let tauri::RunEvent::ExitRequested { .. } = event {
            tracing::info!("应用退出请求,执行清理...");
            use tauri::Manager;
            if let Some(state) = app_handle.try_state::<ollama::commands::OllamaState>() {
                let rt = tokio::runtime::Runtime::new().expect("无法创建清理运行时");
                let manager = rt.block_on(state.get());
                match rt.block_on(manager.shutdown(false)) {
                    Ok(steps) => {
                        for s in steps {
                            tracing::info!("清理步骤: {}", s);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("清理失败: {}", e);
                    }
                }
            }
            tracing::info!("应用清理完成,即将退出");
        }
    });
}
