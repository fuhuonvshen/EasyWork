// EasyWork - LLM Tauri 命令
// 提供前端调用的接口：模型列表/下载/删除/加载/状态 等

use tauri::State;
use crate::state::{DbState, LlmState};
use crate::database;

#[tauri::command]
pub async fn llm_list_models(
    state: State<'_, LlmState>,
) -> Result<serde_json::Value, String> {
    let engine = state.0.read().await;
    let models = engine.list_models();
    let server_healthy = engine.is_server_healthy().await;
    let current = engine.current_model.read().await.clone();
    let binary_ready = engine.is_binary_ready();
    Ok(serde_json::json!({
        "models": models,
        "serverHealthy": server_healthy,
        "currentModel": current,
        "binaryReady": binary_ready,
    }))
}

#[tauri::command]
pub async fn llm_download_model(
    name: String,
    state: State<'_, LlmState>,
) -> Result<(), String> {
    let engine = state.0.read().await;
    engine.download_model(&name).await.map_err(|e| format!("下载模型失败: {}", e))
}

#[tauri::command]
pub async fn llm_download_status(
    state: State<'_, LlmState>,
) -> Result<serde_json::Value, String> {
    let engine = state.0.read().await;
    Ok(engine.get_download_state())
}

#[tauri::command]
pub async fn llm_cancel_download(
    state: State<'_, LlmState>,
) -> Result<(), String> {
    let engine = state.0.read().await;
    engine.cancel_download();
    Ok(())
}

#[tauri::command]
pub async fn llm_delete_model(
    name: String,
    state: State<'_, LlmState>,
) -> Result<(), String> {
    let engine = state.0.read().await;
    // If loaded model is being deleted, stop server first
    let current = engine.current_model.read().await.clone();
    if current.as_deref() == Some(&name) {
        engine.stop_server().await;
    }
    engine.delete_model(&name).map_err(|e| format!("删除模型失败: {}", e))
}

#[tauri::command]
pub async fn llm_load_model(
    name: String,
    app: tauri::AppHandle,
    state: State<'_, LlmState>,
) -> Result<(), String> {
    let engine = state.0.read().await;

    // Ensure binary is ready
    if !engine.is_binary_ready() {
        return Err("请先等待 llama-server 下载完成".to_string());
    }

    // Ensure model file exists
    engine.start_server(&name).await.map_err(|e| format!("加载模型失败: {}", e))
}

#[tauri::command]
pub async fn llm_unload_model(
    state: State<'_, LlmState>,
) -> Result<(), String> {
    let engine = state.0.read().await;
    engine.stop_server().await;
    Ok(())
}

#[tauri::command]
pub async fn llm_server_status(
    state: State<'_, LlmState>,
) -> Result<serde_json::Value, String> {
    let engine = state.0.read().await;
    let healthy = engine.is_server_healthy().await;
    let current = engine.current_model.read().await.clone();
    let binary_ready = engine.is_binary_ready();
    Ok(serde_json::json!({
        "healthy": healthy,
        "currentModel": current,
        "binaryReady": binary_ready,
    }))
}

/// Called when entering the Agent module.
/// If backend is "local" and a model is downloaded but not loaded,
/// lazily starts llama-server. Online mode skips entirely.
#[tauri::command]
pub async fn agent_prepare_llm(
    llm_state: State<'_, LlmState>,
    db_state: State<'_, DbState>,
) -> Result<serde_json::Value, String> {
    // Read backend setting
    let backend = database::repo::get_setting(&db_state.0, "agent_llm_backend")
        .await
        .map_err(|e| format!("读取设置失败: {}", e))?
        .unwrap_or_default();

    // Online mode — no local server needed
    if backend != "local" {
        return Ok(serde_json::json!({"status": "skipped"}));
    }

    let engine = llm_state.0.read().await;

    // Already running
    if engine.is_server_healthy().await {
        let model = engine.current_model.read().await.clone();
        return Ok(serde_json::json!({"status": "ready", "model": model}));
    }

    // Not running — find a downloaded model and start
    let models = engine.list_models();
    if let Some(model) = models.iter().find(|m| m.downloaded) {
        let name = model.name.clone();
        drop(engine); // release read lock before async startup
        log::info!("Lazy-loading LLM model for Agent: {}", name);

        let engine = llm_state.0.read().await;
        engine.start_server(&name).await.map_err(|e| format!("启动本地模型失败: {}", e))?;

        // Persist as last used model for future sessions
        let _ = database::repo::update_setting(&db_state.0, "last_llm_model", &name).await;

        Ok(serde_json::json!({"status": "loading", "model": name}))
    } else {
        Ok(serde_json::json!({"status": "no_model"}))
    }
}
