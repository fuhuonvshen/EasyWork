# Agent Phase 1: Framework 搭建 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 搭建办公助手 Agent 的基础框架：对话界面 + 多轮上下文 + 对话历史持久化，能跑通"用户输入 → Ollama → 渲染回复"的完整链路。

**Architecture:** 前端新增 AgentApp（对话界面）作为 Workbench 的同级视图；后端新增 `agent/` 模块负责上下文管理和 Prompt 模板，`commands/agent.rs` 负责 Tauri 命令。对话数据持久化到 SQLite 两张新表。Ollama 调用复用已有 `summary/ollama.rs` 模式（`think: false`）。

**Tech Stack:** Rust (Tauri 2.x, sqlx, reqwest, serde_json) + TypeScript (React, Tailwind CSS, lucide-react)

---

## File Map

```
Create:
  src-tauri/src/agent/mod.rs              — 模块声明
  src-tauri/src/agent/context.rs          — 上下文窗口管理（构建 messages 数组）
  src-tauri/src/agent/prompt.rs           — System Prompt 模板
  src-tauri/src/commands/agent.rs         — Tauri 命令（chat / CRUD）
  src/components/AgentApp.tsx             — 主布局（sidebar + chat）
  src/components/AgentChat.tsx            — 消息列表 + 输入区
  src/components/AgentSidebar.tsx         — 历史对话列表

Modify:
  src-tauri/src/database/models.rs        — 新增 AgentConversation, AgentMessage
  src-tauri/src/database/repo.rs          — 新增建表 + agent 相关 CRUD
  src-tauri/src/commands/mod.rs           — 新增 pub mod agent;
  src-tauri/src/lib.rs                    — 注册 6 个 agent 命令
  src/types.ts                            — 新增 Agent 相关 TypeScript 类型
  src/components/Workbench.tsx            — 「语音助手」→「办公助手」，去掉 placeholder
  src/App.tsx                             — 新增 agent 视图路由
  src/index.css                           — 确保已有的 tailwind 样式生效
```

---

### Task 1: 数据库模型与建表

**Files:**
- Modify: `src-tauri/src/database/models.rs`
- Modify: `src-tauri/src/database/repo.rs`

- [ ] **Step 1: 新增 Rust 数据模型**

在 `src-tauri/src/database/models.rs` 末尾追加：

```rust
/// Agent conversation session.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentConversation {
    pub id: String,
    pub title: String,
    pub created_at: String,
}

/// Agent conversation message.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentMessage {
    pub id: String,
    pub conversation_id: String,
    pub role: String,   // "user" | "assistant" | "system" | "tool"
    pub content: String,
    pub tool_calls: Option<String>,  // JSON
    pub created_at: String,
}

/// Summary row for sidebar conversation list.
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AgentConversationSummary {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub last_message: Option<String>,
}
```

- [ ] **Step 2: 新增建表 SQL**

在 `src-tauri/src/database/repo.rs` 的 `init_db` 函数中，`Ok(())` 之前追加：

```rust
sqlx::query(
    "CREATE TABLE IF NOT EXISTS agent_conversations (
        id          TEXT PRIMARY KEY,
        title       TEXT NOT NULL DEFAULT '',
        created_at  TEXT NOT NULL
    )",
)
.execute(pool)
.await
.context("创建 agent_conversations 表失败")?;

sqlx::query(
    "CREATE TABLE IF NOT EXISTS agent_messages (
        id              TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL,
        role            TEXT NOT NULL,
        content         TEXT NOT NULL DEFAULT '',
        tool_calls      TEXT,
        created_at      TEXT NOT NULL,
        FOREIGN KEY (conversation_id) REFERENCES agent_conversations(id)
    )",
)
.execute(pool)
.await
.context("创建 agent_messages 表失败")?;
```

- [ ] **Step 3: 新增 Agent CRUD 函数**

在 `src-tauri/src/database/repo.rs` 末尾追加：

```rust
// ── Agent Conversations ──────────────────────────────────────────

use super::models::{AgentConversation, AgentConversationSummary, AgentMessage};

pub async fn agent_create_conversation(pool: &SqlitePool, conv: &AgentConversation) -> Result<()> {
    sqlx::query("INSERT INTO agent_conversations (id, title, created_at) VALUES (?, ?, ?)")
        .bind(&conv.id)
        .bind(&conv.title)
        .bind(&conv.created_at)
        .execute(pool)
        .await
        .context("创建对话失败")?;
    Ok(())
}

pub async fn agent_list_conversations(pool: &SqlitePool) -> Result<Vec<AgentConversationSummary>> {
    sqlx::query_as::<_, AgentConversationSummary>(
        "SELECT ac.id, ac.title, ac.created_at,
                (SELECT SUBSTR(am.content, 1, 100) FROM agent_messages am
                 WHERE am.conversation_id = ac.id AND am.role = 'user'
                 ORDER BY am.created_at DESC LIMIT 1) AS last_message
         FROM agent_conversations ac
         ORDER BY ac.created_at DESC"
    )
    .fetch_all(pool)
    .await
    .context("查询对话列表失败")
}

pub async fn agent_delete_conversation(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM agent_messages WHERE conversation_id = ?")
        .bind(id).execute(pool).await?;
    sqlx::query("DELETE FROM agent_conversations WHERE id = ?")
        .bind(id).execute(pool).await
        .context("删除对话失败")?;
    Ok(())
}

pub async fn agent_rename_conversation(pool: &SqlitePool, id: &str, title: &str) -> Result<()> {
    sqlx::query("UPDATE agent_conversations SET title = ? WHERE id = ?")
        .bind(title).bind(id).execute(pool).await
        .context("重命名对话失败")?;
    Ok(())
}

pub async fn agent_insert_message(pool: &SqlitePool, msg: &AgentMessage) -> Result<()> {
    sqlx::query(
        "INSERT INTO agent_messages (id, conversation_id, role, content, tool_calls, created_at)
         VALUES (?, ?, ?, ?, ?, ?)"
    )
    .bind(&msg.id).bind(&msg.conversation_id).bind(&msg.role)
    .bind(&msg.content).bind(&msg.tool_calls).bind(&msg.created_at)
    .execute(pool).await
    .context("插入消息失败")?;
    Ok(())
}

pub async fn agent_get_messages(pool: &SqlitePool, conversation_id: &str) -> Result<Vec<AgentMessage>> {
    sqlx::query_as::<_, AgentMessage>(
        "SELECT * FROM agent_messages WHERE conversation_id = ? ORDER BY created_at ASC"
    )
    .bind(conversation_id)
    .fetch_all(pool).await
    .context("查询消息失败")
}

pub async fn agent_auto_title(pool: &SqlitePool, conversation_id: &str) -> Result<()> {
    let first: Option<(String,)> = sqlx::query_as(
        "SELECT content FROM agent_messages WHERE conversation_id = ? AND role = 'user' ORDER BY created_at ASC LIMIT 1"
    )
    .bind(conversation_id)
    .fetch_optional(pool).await?;
    if let Some((content,)) = first {
        let title: String = content.chars().take(20).collect();
        sqlx::query("UPDATE agent_conversations SET title = ? WHERE id = ?")
            .bind(&title).bind(conversation_id).execute(pool).await?;
    }
    Ok(())
}
```

- [ ] **Step 4: Cargo check 验证编译**

```bash
cd "d:/PyProject/MoM/src-tauri" && export LIBCLANG_PATH="D:/LLVM/bin" && export CMAKE="D:/CMake/cmake-3.31.5-windows-x86_64/bin/cmake.exe" && cargo check 2>&1
```

---

### Task 2: Agent Rust 模块（context + prompt）

**Files:**
- Create: `src-tauri/src/agent/mod.rs`
- Create: `src-tauri/src/agent/context.rs`
- Create: `src-tauri/src/agent/prompt.rs`

- [ ] **Step 1: 创建 mod.rs**

创建 `src-tauri/src/agent/mod.rs`：

```rust
pub mod context;
pub mod prompt;
```

- [ ] **Step 2: 创建 prompt.rs — System Prompt 模板**

创建 `src-tauri/src/agent/prompt.rs`：

```rust
/// Build the system prompt for the supply chain production role.
pub fn system_prompt() -> String {
    r#"你是一个专业的供应链生产助手，运行在用户的本地桌面应用中。

你的职责：
1. 回答生产制造相关的问题（排程、物料、质量、工艺等）
2. 根据用户提供的数据生成生产日报、交接班记录等结构化报告
3. 从会议纪要中提取待办事项和关键结论
4. 帮助用户分析生产数据（产量、不良率、OEE 等）

回答原则：
- 使用中文回答
- 数据相关的输出优先使用表格展示
- 关键结论使用 **加粗** 强调
- 待办事项使用 - [ ] 格式
- 如果信息不足以给出准确回答，先列出需要补充的数据
- 不要编造数据，不确定的地方明确标注 [待确认]
- 简洁专业，避免冗长

你可以访问以下数据源：
- 用户的会议纪要和日程（通过工具调用）
- 用户粘贴或拖入的 Excel/CSV 数据
- 用户手动输入的信息
"#.to_string()
}
```

- [ ] **Step 3: 创建 context.rs — 上下文窗口管理**

创建 `src-tauri/src/agent/context.rs`：

```rust
use anyhow::Result;
use sqlx::sqlite::SqlitePool;
use crate::database::models::AgentMessage;
use crate::database::repo;
use crate::agent::prompt;

const MAX_CONTEXT_MESSAGES: usize = 20;

/// Build the messages array for Ollama API call.
/// Returns Vec<serde_json::Value> in Ollama chat format.
pub async fn build_messages(
    pool: &SqlitePool,
    conversation_id: &str,
    new_user_message: &str,
) -> Result<Vec<serde_json::Value>> {
    let history = repo::agent_get_messages(pool, conversation_id).await?;

    let mut messages: Vec<serde_json::Value> = Vec::new();

    // System prompt
    messages.push(serde_json::json!({
        "role": "system",
        "content": prompt::system_prompt()
    }));

    let total = history.len();
    let start = if total > MAX_CONTEXT_MESSAGES {
        total - MAX_CONTEXT_MESSAGES
    } else {
        0
    };

    for msg in &history[start..] {
        messages.push(serde_json::json!({
            "role": msg.role,
            "content": msg.content
        }));
    }

    // Append the new user message (not yet persisted)
    messages.push(serde_json::json!({
        "role": "user",
        "content": new_user_message
    }));

    Ok(messages)
}
```

- [ ] **Step 4: 在 lib.rs 中声明 agent 模块**

在 `src-tauri/src/lib.rs` 第 4 行附近，`mod audio;` 之后添加：

```rust
mod agent;
```

- [ ] **Step 5: Cargo check 验证**

```bash
cd "d:/PyProject/MoM/src-tauri" && export LIBCLANG_PATH="D:/LLVM/bin" && export CMAKE="D:/CMake/cmake-3.31.5-windows-x86_64/bin/cmake.exe" && cargo check 2>&1
```

---

### Task 3: Agent Tauri 命令

**Files:**
- Create: `src-tauri/src/commands/agent.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 在 commands/mod.rs 中添加 agent 模块**

在 `src-tauri/src/commands/mod.rs` 第 1 行后添加：

```rust
pub mod agent;
```

- [ ] **Step 2: 创建 commands/agent.rs 完整文件**

创建 `src-tauri/src/commands/agent.rs`：

```rust
// MoM - Agent chat commands

use tauri::State;
use crate::state::DbState;
use crate::database::models::{AgentConversation, AgentMessage};
use crate::database::repo;
use crate::agent::context;

/// Send a chat message and get AI response.
/// Returns the assistant's reply text.
#[tauri::command]
pub async fn agent_chat(
    conversation_id: String,
    message: String,
    db: State<'_, DbState>,
) -> Result<String, String> {
    // 1. Persist user message
    let user_msg = AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        role: "user".to_string(),
        content: message.clone(),
        tool_calls: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    repo::agent_insert_message(&db.0, &user_msg)
        .await
        .map_err(|e| format!("保存消息失败: {}", e))?;

    // Auto-title on first message
    let _ = repo::agent_auto_title(&db.0, &conversation_id).await;

    // 2. Build messages with context
    let messages = context::build_messages(&db.0, &conversation_id, &message)
        .await
        .map_err(|e| format!("构建上下文失败: {}", e))?;

    // 3. Call Ollama
    let body = serde_json::json!({
        "model": "qwen3.5:4b",
        "messages": messages,
        "stream": false,
        "think": false
    });

    let client = reqwest::Client::new();
    let response = client
        .post("http://localhost:11434/api/chat")
        .json(&body)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| format!("无法连接 Ollama: {}", e))?;

    let chat: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    let content = chat["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "[Ollama 返回空内容]".to_string());

    // 4. Persist assistant message
    let assistant_msg = AgentMessage {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.clone(),
        role: "assistant".to_string(),
        content: content.clone(),
        tool_calls: None,
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    repo::agent_insert_message(&db.0, &assistant_msg)
        .await
        .map_err(|e| log::warn!("保存助手回复失败: {}", e))
        .ok();

    Ok(content)
}

/// List all conversations (for sidebar).
#[tauri::command]
pub async fn agent_list_conversations(
    db: State<'_, DbState>,
) -> Result<Vec<crate::database::models::AgentConversationSummary>, String> {
    repo::agent_list_conversations(&db.0)
        .await
        .map_err(|e| format!("查询对话列表失败: {}", e))
}

/// Create a new conversation, returns its id.
#[tauri::command]
pub async fn agent_create_conversation(
    db: State<'_, DbState>,
) -> Result<String, String> {
    let conv = AgentConversation {
        id: uuid::Uuid::new_v4().to_string(),
        title: String::new(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    let id = conv.id.clone();
    repo::agent_create_conversation(&db.0, &conv)
        .await
        .map_err(|e| format!("创建对话失败: {}", e))?;
    Ok(id)
}

/// Delete a conversation and its messages.
#[tauri::command]
pub async fn agent_delete_conversation(
    id: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    repo::agent_delete_conversation(&db.0, &id)
        .await
        .map_err(|e| format!("删除对话失败: {}", e))
}

/// Rename a conversation.
#[tauri::command]
pub async fn agent_rename_conversation(
    id: String,
    title: String,
    db: State<'_, DbState>,
) -> Result<(), String> {
    repo::agent_rename_conversation(&db.0, &id, &title)
        .await
        .map_err(|e| format!("重命名对话失败: {}", e))
}

/// Get all messages for a conversation.
#[tauri::command]
pub async fn agent_get_messages(
    conversation_id: String,
    db: State<'_, DbState>,
) -> Result<Vec<AgentMessage>, String> {
    repo::agent_get_messages(&db.0, &conversation_id)
        .await
        .map_err(|e| format!("查询消息失败: {}", e))
}
```

- [ ] **Step 3: 在 lib.rs 中注册 agent 命令**

`generate_handler![]` 末尾的 `]` 之前追加：

```rust
            commands::agent::agent_chat,
            commands::agent::agent_list_conversations,
            commands::agent::agent_create_conversation,
            commands::agent::agent_delete_conversation,
            commands::agent::agent_rename_conversation,
            commands::agent::agent_get_messages,
```

- [ ] **Step 4: Cargo check 验证**

```bash
cd "d:/PyProject/MoM/src-tauri" && export LIBCLANG_PATH="D:/LLVM/bin" && export CMAKE="D:/CMake/cmake-3.31.5-windows-x86_64/bin/cmake.exe" && cargo check 2>&1
```

---

### Task 4: 前端 TypeScript 类型 + Workbench 卡片更新

**Files:**
- Modify: `src/types.ts`
- Modify: `src/components/Workbench.tsx`

- [ ] **Step 1: 新增 Agent TypeScript 类型**

在 `src/types.ts` 末尾追加：

```typescript
// Agent types
export interface AgentConversationSummary {
  id: string;
  title: string;
  created_at: string;
  last_message: string | null;
}

export interface AgentMessage {
  id: string;
  conversation_id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string;
  tool_calls: string | null;
  created_at: string;
}

export type AppView = "workbench" | "minutes" | "agent";
```

- [ ] **Step 2: 更新 Workbench.tsx — 替换语音助手为办公助手**

修改 `src/components/Workbench.tsx`：

将第 24-32 行的语音助手卡片替换为：

```typescript
  {
    key: "agent",
    icon: Bot,
    title: "办公助手",
    desc: "智能问答 · 生产报告 · 待办管理",
    color: "bg-emerald-50 text-emerald-600",
    hoverColor: "hover:bg-emerald-100 hover:border-emerald-200",
    action: "agent" as const,
  },
```

同时将第 2 行的 import 中 `Mic` 替换为 `Bot`：

```typescript
import { FileText, Video, Bot, LayoutGrid } from "lucide-react";
```

删除第 3 行的 `MinutesTab` 导入（不再需要）：

```typescript
// 删除: import type { MinutesTab } from "../types";
```

更新第 44 行的 Props 类型：

```typescript
export default function Workbench({ onEnter }: { onEnter: (title?: string, action?: string) => void }) {
```

更新第 55 行的 onClick 逻辑，让 agent 卡片也响应点击：

```typescript
              onClick={() => {
                if (!isPlaceholder) {
                  if (card.key === "agent") {
                    onEnter(undefined, "agent");
                  } else {
                    onEnter(undefined, card.action as string);
                  }
                }
              }}
```

- [ ] **Step 3: 验证 TypeScript 编译**

```bash
cd "d:/PyProject/MoM" && npx tsc --noEmit 2>&1
```

Expected: clean output (no errors).

---

### Task 5: 前端 Agent 组件（AgentApp + AgentSidebar + AgentChat）

**Files:**
- Create: `src/components/AgentApp.tsx`
- Create: `src/components/AgentSidebar.tsx`
- Create: `src/components/AgentChat.tsx`

- [ ] **Step 1: 创建 AgentApp.tsx — 主布局**

创建 `src/components/AgentApp.tsx`：

```typescript
// MoM - Agent main layout (sidebar + chat)
import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import AgentSidebar from "./AgentSidebar";
import AgentChat from "./AgentChat";
import type { AgentConversationSummary } from "../types";

export default function AgentApp({ onBack }: { onBack: () => void }) {
  const [conversations, setConversations] = useState<AgentConversationSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);

  const loadConversations = useCallback(() => {
    invoke<AgentConversationSummary[]>("agent_list_conversations")
      .then((list) => {
        setConversations(list);
        if (list.length > 0 && !activeId) {
          setActiveId(list[0].id);
        }
      })
      .catch(console.error)
      .finally(() => setLoading(false));
  }, [activeId]);

  useEffect(() => { loadConversations(); }, []);

  const handleNew = async () => {
    try {
      const id = await invoke<string>("agent_create_conversation");
      setActiveId(id);
      loadConversations();
    } catch (e) { console.error(e); }
  };

  const handleSelect = (id: string) => setActiveId(id);

  const handleDelete = async (id: string) => {
    try {
      await invoke("agent_delete_conversation", { id });
      if (activeId === id) setActiveId(null);
      loadConversations();
    } catch (e) { console.error(e); }
  };

  const handleRename = async (id: string, title: string) => {
    try {
      await invoke("agent_rename_conversation", { id, title });
      loadConversations();
    } catch (e) { console.error(e); }
  };

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-sm text-gray-400">加载中...</p>
      </div>
    );
  }

  if (!activeId && conversations.length === 0) {
    return (
      <div className="h-full flex flex-col items-center justify-center gap-4">
        <p className="text-sm text-gray-400">还没有对话</p>
        <button
          onClick={handleNew}
          className="px-4 py-2 bg-emerald-600 text-white text-sm font-medium rounded-xl hover:bg-emerald-700 transition-colors"
        >
          开始新对话
        </button>
      </div>
    );
  }

  return (
    <div className="flex h-full">
      <AgentSidebar
        conversations={conversations}
        activeId={activeId}
        onSelect={handleSelect}
        onNew={handleNew}
        onDelete={handleDelete}
        onRename={handleRename}
        onBack={onBack}
      />
      <div className="flex-1 flex flex-col min-w-0">
        {activeId ? (
          <AgentChat conversationId={activeId} onConversationUpdate={loadConversations} />
        ) : (
          <div className="flex-1 flex items-center justify-center">
            <p className="text-sm text-gray-400">选择一个对话或创建新对话</p>
          </div>
        )}
      </div>
    </div>
  );
}
```

- [ ] **Step 2: 创建 AgentSidebar.tsx — 历史对话列表**

创建 `src/components/AgentSidebar.tsx`：

```typescript
// MoM - Agent sidebar (conversation list)
import { useState } from "react";
import { ArrowLeft, MessageSquare, Plus, X, Pencil, Check } from "lucide-react";
import type { AgentConversationSummary } from "../types";

interface Props {
  conversations: AgentConversationSummary[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onNew: () => void;
  onDelete: (id: string) => void;
  onRename: (id: string, title: string) => void;
  onBack: () => void;
}

export default function AgentSidebar({ conversations, activeId, onSelect, onNew, onDelete, onRename, onBack }: Props) {
  const [editingId, setEditingId] = useState<string | null>(null);
  const [editTitle, setEditTitle] = useState("");

  return (
    <aside className="w-56 bg-white border-r border-gray-100 flex flex-col flex-shrink-0">
      <div className="px-4 py-5 border-b border-gray-50">
        <button
          onClick={onBack}
          className="flex items-center gap-1.5 text-xs text-gray-400 hover:text-gray-700 transition-colors mb-2"
        >
          <ArrowLeft size={14} />
          返回工作台
        </button>
        <h1 className="text-lg font-bold tracking-tight text-gray-900">办公助手</h1>
        <p className="text-xs text-gray-400 mt-0.5">智能问答 · 生产报告</p>
      </div>

      <div className="flex-1 px-2 py-3 space-y-1 overflow-y-auto">
        <button
          onClick={onNew}
          className="w-full flex items-center gap-2 px-3 py-2 rounded-lg text-sm font-medium text-emerald-600 hover:bg-emerald-50 transition-colors"
        >
          <Plus size={16} />
          新对话
        </button>

        {conversations.map((c) => (
          <div key={c.id} className="group relative">
            {editingId === c.id ? (
              <div className="flex items-center gap-1 px-2 py-1">
                <input
                  value={editTitle}
                  onChange={(e) => setEditTitle(e.target.value)}
                  className="flex-1 px-2 py-1 text-xs border border-gray-200 rounded focus:outline-none focus:ring-1 focus:ring-emerald-300"
                  autoFocus
                  onKeyDown={(e) => {
                    if (e.key === "Enter") { onRename(c.id, editTitle); setEditingId(null); }
                    if (e.key === "Escape") setEditingId(null);
                  }}
                />
                <button onClick={() => { onRename(c.id, editTitle); setEditingId(null); }} className="p-0.5 text-emerald-500 hover:bg-emerald-50 rounded">
                  <Check size={14} />
                </button>
              </div>
            ) : (
              <button
                onClick={() => onSelect(c.id)}
                className={`w-full flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm text-left transition-colors ${
                  c.id === activeId
                    ? "bg-emerald-50 text-emerald-700"
                    : "text-gray-600 hover:bg-gray-50"
                }`}
              >
                <MessageSquare size={16} className="flex-shrink-0" />
                <span className="truncate flex-1">{c.title || "新对话"}</span>
              </button>
            )}
            {c.id === activeId && editingId !== c.id && (
              <div className="absolute right-1 top-1/2 -translate-y-1/2 hidden group-hover:flex items-center gap-0.5">
                <button
                  onClick={(e) => { e.stopPropagation(); setEditingId(c.id); setEditTitle(c.title); }}
                  className="p-1 rounded text-gray-400 hover:text-gray-600 hover:bg-gray-100"
                >
                  <Pencil size={12} />
                </button>
                <button
                  onClick={(e) => { e.stopPropagation(); onDelete(c.id); }}
                  className="p-1 rounded text-gray-400 hover:text-red-500 hover:bg-red-50"
                >
                  <X size={12} />
                </button>
              </div>
            )}
          </div>
        ))}
      </div>
    </aside>
  );
}
```

- [ ] **Step 3: 创建 AgentChat.tsx — 消息列表 + 输入区**

创建 `src/components/AgentChat.tsx`：

```typescript
// MoM - Agent chat area (message list + input)
import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Send, Loader } from "lucide-react";
import { renderMarkdown } from "../utils/markdown";
import type { AgentMessage } from "../types";

interface Props {
  conversationId: string;
  onConversationUpdate: () => void;
}

export default function AgentChat({ conversationId, onConversationUpdate }: Props) {
  const [messages, setMessages] = useState<AgentMessage[]>([]);
  const [input, setInput] = useState("");
  const [sending, setSending] = useState(false);
  const bottomRef = useRef<HTMLDivElement>(null);

  const loadMessages = useCallback(() => {
    invoke<AgentMessage[]>("agent_get_messages", { conversationId })
      .then(setMessages)
      .catch(console.error);
  }, [conversationId]);

  useEffect(() => { loadMessages(); }, [loadMessages]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [messages]);

  const handleSend = async () => {
    const text = input.trim();
    if (!text || sending) return;
    setInput("");

    const userMsg: AgentMessage = {
      id: "temp-" + Date.now(),
      conversation_id: conversationId,
      role: "user",
      content: text,
      tool_calls: null,
      created_at: new Date().toISOString(),
    };
    setMessages((prev) => [...prev, userMsg]);
    setSending(true);

    try {
      const reply = await invoke<string>("agent_chat", {
        conversationId,
        message: text,
      });

      const assistantMsg: AgentMessage = {
        id: "temp-" + (Date.now() + 1),
        conversation_id: conversationId,
        role: "assistant",
        content: reply,
        tool_calls: null,
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, assistantMsg]);
      onConversationUpdate();
    } catch (e) {
      const errorMsg: AgentMessage = {
        id: "temp-" + (Date.now() + 1),
        conversation_id: conversationId,
        role: "assistant",
        content: "**错误:** " + String(e),
        tool_calls: null,
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, errorMsg]);
    }
    setSending(false);
  };

  return (
    <>
      {/* Message list */}
      <div className="flex-1 overflow-y-auto px-8 py-6">
        {messages.length === 0 && (
          <div className="flex items-center justify-center h-full">
            <p className="text-sm text-gray-400">开始对话吧</p>
          </div>
        )}
        <div className="max-w-3xl mx-auto space-y-4">
          {messages.map((msg) => (
            <div
              key={msg.id}
              className={`flex gap-3 text-sm ${
                msg.role === "user" ? "justify-end" : "justify-start"
              }`}
            >
              {msg.role === "assistant" && (
                <div className="w-7 h-7 rounded-full bg-emerald-100 flex items-center justify-center flex-shrink-0 mt-1">
                  <span className="text-xs font-bold text-emerald-600">AI</span>
                </div>
              )}
              <div
                className={`max-w-[75%] rounded-2xl px-4 py-3 ${
                  msg.role === "user"
                    ? "bg-emerald-600 text-white"
                    : "bg-gray-100 text-gray-700"
                }`}
              >
                {msg.role === "assistant" ? (
                  <div
                    className="prose prose-sm max-w-none [&_h2]:text-base [&_h2]:font-semibold [&_h3]:text-sm [&_h3]:font-semibold [&_table]:text-xs [&_th]:px-2 [&_th]:py-1 [&_td]:px-2 [&_td]:py-1 [&_ul]:my-1 [&_ol]:my-1 [&_li]:text-sm [&_p]:my-1 [&_strong]:font-semibold"
                    dangerouslySetInnerHTML={{ __html: renderMarkdown(msg.content) }}
                  />
                ) : (
                  <p className="whitespace-pre-wrap">{msg.content}</p>
                )}
              </div>
              {msg.role === "user" && (
                <div className="w-7 h-7 rounded-full bg-gray-300 flex items-center justify-center flex-shrink-0 mt-1">
                  <span className="text-xs font-bold text-white">U</span>
                </div>
              )}
            </div>
          ))}
          {sending && (
            <div className="flex items-center gap-2 text-sm text-gray-400">
              <Loader size={14} className="animate-spin" />
              AI 思考中...
            </div>
          )}
          <div ref={bottomRef} />
        </div>
      </div>

      {/* Input area */}
      <div className="border-t border-gray-100 bg-white px-6 py-4">
        <div className="max-w-3xl mx-auto flex items-center gap-3">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) handleSend(); }}
            placeholder="输入你的问题... (Enter 发送)"
            disabled={sending}
            className="flex-1 px-4 py-3 rounded-xl border border-gray-200 bg-gray-50 text-sm placeholder-gray-400 focus:outline-none focus:ring-2 focus:ring-emerald-300 focus:border-emerald-400 focus:bg-white disabled:opacity-50 transition-colors"
          />
          <button
            onClick={handleSend}
            disabled={!input.trim() || sending}
            className="p-3 rounded-xl bg-emerald-600 text-white hover:bg-emerald-700 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
          >
            <Send size={18} />
          </button>
        </div>
      </div>
    </>
  );
}
```

- [ ] **Step 4: 验证 TypeScript 编译**

```bash
cd "d:/PyProject/MoM" && npx tsc --noEmit 2>&1
```

Expected: clean output (no errors).

---

### Task 6: 前端路由集成 — App.tsx 添加 agent 视图

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 更新 App.tsx 支持 agent 视图**

修改 `src/App.tsx`：

将第 3 行的 import 改为：

```typescript
import Workbench from "./components/Workbench";
import MinutesApp from "./components/MinutesApp";
import AgentApp from "./components/AgentApp";
import type { MinutesTab } from "./types";
```

将第 5 行的 view 类型改为：

```typescript
const [view, setView] = useState<"workbench" | "minutes" | "agent">("workbench");
```

将第 22-33 行的 Workbench onEnter 回调更新为：

```typescript
      {view === "workbench" && (
        <Workbench
          onEnter={(title?: string, action?: string) => {
            if (action === "agent") {
              setView("agent");
            } else {
              setPrefillTitle(title || "");
              setInitialTab((action as MinutesTab) || "today");
              setView("minutes");
            }
          }}
        />
      )}
```

在第 37 行后（`{view === "minutes" && (...)}` 闭合后）新增：

```typescript
      {view === "agent" && (
        <AgentApp onBack={() => setView("workbench")} />
      )}
```

- [ ] **Step 2: 验证 TypeScript 编译**

```bash
cd "d:/PyProject/MoM" && npx tsc --noEmit 2>&1
```

Expected: clean output (no errors).

---

### Task 7: 全链路联调验证

- [ ] **Step 1: Rust 编译**

```bash
cd "d:/PyProject/MoM/src-tauri" && export LIBCLANG_PATH="D:/LLVM/bin" && export CMAKE="D:/CMake/cmake-3.31.5-windows-x86_64/bin/cmake.exe" && cargo check 2>&1
```

Expected: `Finished` with only pre-existing warnings.

- [ ] **Step 2: TypeScript 编译**

```bash
cd "d:/PyProject/MoM" && npx tsc --noEmit 2>&1
```

Expected: clean output.

- [ ] **Step 3: 完整构建**

```bash
cd "d:/PyProject/MoM/src-tauri" && export LIBCLANG_PATH="D:/LLVM/bin" && export CMAKE="D:/CMake/cmake-3.31.5-windows-x86_64/bin/cmake.exe" && cargo build 2>&1
```

Expected: `Finished` with no errors.

---

### Task 8: 手动测试 Checklist

- [ ] 启动应用 `npm run tauri:dev`
- [ ] 主页显示四张卡片，「办公助手」替换了「语音助手」，颜色为 emerald（绿色）
- [ ] 点击「办公助手」→ 进入 Agent 界面（空状态："还没有对话" + "开始新对话"按钮）
- [ ] 点击「开始新对话」→ 创建对话，进入聊天界面
- [ ] 输入消息发送 → 看到用户消息（右对齐，白色文字绿色背景）
- [ ] 等待 Ollama 响应 → 看到 AI 回复（左对齐，Markdown 渲染，AI 头像）
- [ ] 左侧栏显示对话列表，标题自动取首条消息前 20 字
- [ ] 可以创建多个对话，切换对话保留各自的消息
- [ ] 重命名/删除对话功能正常
- [ ] 返回工作台 → 再次进入，对话历史保持
