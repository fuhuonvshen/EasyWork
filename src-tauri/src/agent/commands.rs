// EasyWork - Agent Tauri commands (thin HTTP proxies to Python agent server).
//
// All LLM logic (Ollama calls, context building, memory management,
// ReAct loop, skill system) runs in the Python sidecar.
// These commands only forward requests and return responses.

use tauri::State;
use crate::state::AgentSidecarState;
use crate::state::DbState;
use crate::database::repo;
use crate::database::models::TodoItem;

/// Forward a chat message to the Python agent server and return the AI response.
#[tauri::command]
pub async fn agent_chat(
    conversation_id: String,
    message: String,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<String, String> {
    let body = serde_json::json!({
        "conversation_id": conversation_id,
        "message": message,
    });
    let resp: serde_json::Value = sidecar.0.post("/chat", &body).await?;
    resp["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 content 字段".to_string())
}

/// Forward a file attachment request to the Python agent server.
#[tauri::command]
pub async fn agent_attach_file(
    conversation_id: String,
    file_path: String,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<String, String> {
    let body = serde_json::json!({
        "conversation_id": conversation_id,
        "file_path": file_path,
    });
    let resp: serde_json::Value = sidecar.0.post("/attach_file", &body).await?;
    resp["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 content 字段".to_string())
}

/// List all conversations.
#[tauri::command]
pub async fn agent_list_conversations(
    sidecar: State<'_, AgentSidecarState>,
) -> Result<Vec<crate::database::models::AgentConversationSummary>, String> {
    sidecar.0.get("/list_conversations").await
}

/// Create a new conversation, returns its id.
#[tauri::command]
pub async fn agent_create_conversation(
    sidecar: State<'_, AgentSidecarState>,
) -> Result<String, String> {
    let resp: serde_json::Value = sidecar.0.post("/create_conversation", &serde_json::json!({})).await?;
    resp["id"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 id 字段".to_string())
}

/// Delete a conversation and its messages.
#[tauri::command]
pub async fn agent_delete_conversation(
    id: String,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<(), String> {
    let body = serde_json::json!({ "id": id });
    let _: serde_json::Value = sidecar.0.post("/delete_conversation", &body).await?;
    Ok(())
}

/// Rename a conversation.
#[tauri::command]
pub async fn agent_rename_conversation(
    id: String,
    title: String,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<(), String> {
    let body = serde_json::json!({ "id": id, "title": title });
    let _: serde_json::Value = sidecar.0.post("/rename_conversation", &body).await?;
    Ok(())
}

/// Forward file content (from file picker) to the Python server.
#[tauri::command]
pub async fn agent_attach_content(
    conversation_id: String,
    file_name: String,
    content: String,
    is_binary: bool,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<String, String> {
    let body = serde_json::json!({
        "conversation_id": conversation_id,
        "file_name": file_name,
        "content": content,
        "is_binary": is_binary,
    });
    let resp: serde_json::Value = sidecar.0.post("/attach_content", &body).await?;
    resp["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "响应缺少 content 字段".to_string())
}

/// Get all messages for a conversation.
#[tauri::command]
pub async fn agent_get_messages(
    conversation_id: String,
    sidecar: State<'_, AgentSidecarState>,
) -> Result<Vec<crate::database::models::AgentMessage>, String> {
    sidecar.0.get(&format!("/get_messages?conversation_id={}", &conversation_id)).await
}

// ── Todo CRUD ──────────────────────────────────────────────────────

#[tauri::command]
pub async fn todo_create(
    db: State<'_, DbState>,
    title: String,
    deadline: Option<String>,
    priority: Option<String>,
    source: Option<String>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    let todo = TodoItem {
        id: id.clone(),
        title,
        status: "pending".to_string(),
        priority: priority.unwrap_or_else(|| "medium".to_string()),
        deadline,
        source: source.unwrap_or_else(|| "manual".to_string()),
        created_at: now,
        schedule_id: None,
    };
    repo::todo_create(&db.0, &todo)
        .await
        .map_err(|e| format!("创建待办失败: {}", e))?;
    Ok(id)
}

#[tauri::command]
pub async fn todo_list(
    db: State<'_, DbState>,
) -> Result<Vec<TodoItem>, String> {
    repo::todo_list(&db.0)
        .await
        .map_err(|e| format!("查询待办列表失败: {}", e))
}

#[tauri::command]
pub async fn todo_update_status(
    db: State<'_, DbState>,
    id: String,
    status: String,
) -> Result<(), String> {
    repo::todo_update_status(&db.0, &id, &status)
        .await
        .map_err(|e| format!("更新待办状态失败: {}", e))
}

#[tauri::command]
pub async fn todo_delete(
    db: State<'_, DbState>,
    id: String,
) -> Result<(), String> {
    repo::todo_delete(&db.0, &id)
        .await
        .map_err(|e| format!("删除待办失败: {}", e))
}
