// 切分策略相关 Tauri Command

use super::strategies::{
    ChunkResult, SplitContext, SplitStrategyId, StrategyInfo, get_strategy, list_strategies,
};
use crate::error::AppError;
use crate::ollama::commands::OllamaState;
use tauri::State;

/// 获取所有可用切分策略(供前端渲染选择项)
#[tauri::command]
pub async fn get_split_strategies() -> Result<Vec<StrategyInfo>, AppError> {
    Ok(list_strategies())
}

/// 预览切分效果
#[tauri::command]
pub async fn preview_split(
    ollama_state: State<'_, OllamaState>,
    text: String,
    strategy_id: String,
    params: serde_json::Value,
) -> Result<Vec<ChunkResult>, AppError> {
    let id = SplitStrategyId::from_str(&strategy_id)?;
    let strategy = get_strategy(id);
    // best-effort 注入：Ollama 在运行才给 client，否则给 none，
    // 由策略自己决定报错(预览)还是降级——非 LLM 策略完全不读 ctx
    let ctx = if id == SplitStrategyId::Agentic {
        let manager = ollama_state.get().await;
        if manager.is_running().await {
            SplitContext::new(
                manager.client_clone(),
                crate::ollama::effective_chat_model(&manager.settings_snapshot()),
                false, // 预览路径直接报错，让用户感知 Ollama 未运行
            )
        } else {
            SplitContext::none()
        }
    } else {
        SplitContext::none()
    };
    let chunks = strategy.split(&text, &params, &ctx).await?;
    Ok(chunks)
}
