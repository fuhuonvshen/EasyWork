# PRD：办公助手 Agent（代号：MomAgent）

> 版本 v2.0 | 2026-07-01 | 目标用户：供应链生产部门

---

## 一、项目背景

### 1.1 行业现状

2025-2026 年，AI Agent 已从被动问答演进为自主执行。核心趋势：

- **本地优先架构兴起**：Microsoft Fara-7B（73.5% 桌面任务成功率）、Belt Desktop 等产品验证了"本地模型处理敏感工作"的可行性
- **88% 企业已将 Agent 融入系统**（毕马威 2026），但其落地依赖具体岗位场景

### 1.2 供应链生产部门的典型痛点

| 场景 | 痛点 | Agent 价值 |
|------|------|-----------|
| 跨系统数据查询 | ERP/MES/Excel 多系统割裂，回答一个问题要查三四个地方 | 自然语言统一查询入口 |
| 生产日报/周报 | 手工从多个来源汇总数据，日均 25-40 分钟 | 一键生成，自动关联产量+质量+异常 |
| 物料齐套检查 | 需要交叉对比 BOM、库存、采购在途、生产计划 | 自动关联计算，标出短缺项 |
| 供应商交期变更影响分析 | 一个物料延迟 → 需要手动查影响哪些工单/哪些客户订单 | 自动追溯影响链 |
| 异常归因 | "A 线今天产量掉了 15%"→ 是设备问题？缺料？人员？需要查多个系统 | 关联多维度数据给初判 |
| 交接班记录 | 每班手写/打字，信息遗漏、格式不统一 | 从当班数据自动生成结构化交接记录 |

### 1.3 与 MoM 现有能力的关系

已有基础设施可复用：
- 本地 Whisper/SenseVoice 语音识别
- 本地 Ollama LLM（qwen3.5:4b, think:false）
- SQLite 数据库（会议、纪要、日程、报告）
- 日程提醒系统
- Markdown 报告渲染

---

## 二、产品定位

### 2.1 一句话描述

> MomAgent 是运行在本地的桌面 AI 助手，面向供应链生产岗位，能够**跨数据源回答生产问题、自动生成报告/交接记录、从对话中提取并跟踪待办事项**。

### 2.2 替换哪个占位模块

替换主页「**语音助手**」卡片（`key: "collab"`），重命名为「**办公助手**」。

### 2.3 差异化

| 维度 | 通用 AI 助手 | MomAgent |
|------|------------|----------|
| 运行环境 | 云端 | **本地 Ollama**，数据不出设备 |
| 领域知识 | 通用 | **生产制造场景预设** |
| 数据源 | 仅对话 | **可读取本地 Excel/CSV + MoM 数据库** |
| 报告 | 通用格式 | **生产日报/交接班记录等结构化模板** |
| 成本 | $20-30/月 | 免费（本地算力） |

---

## 三、核心功能

### 3.1 功能全景

```
办公助手 Agent
├── 1. 对话式查询（Chat）
│   ├── 自然语言问答（本地 LLM）
│   ├── 上下文记忆（跨会话）
│   └── 语音输入（复用 SenseVoice）
├── 2. 报告与记录（Write）
│   ├── 生产日报/周报自动生成
│   ├── 交接班记录生成
│   ├── 会议纪要 → 行动项提取
│   └── Markdown 渲染 + 一键复制
├── 3. 数据分析（Analyze）
│   ├── 本地 CSV/Excel 文件分析
│   ├── 自然语言 → SQL 查询（SQLite）
│   └── 跨表关联查询（BOM × 库存 × 采购）
├── 4. 待办与跟踪（Track）
│   ├── 从对话/纪要中提取待办事项
│   ├── 待办列表 + 状态管理
│   └── 物料短缺 / 交期延迟预警
├── 5. 知识库（Know）
│   ├── 本地文档索引（RAG）
│   └── 产品规格、工艺参数等查询
└── 6. 工具调用（Tools）
    ├── 读取本地文件
    ├── 发送系统通知
    └── 执行 Shell 命令（白名单）
```

### 3.2 MVP 功能范围（P0）

| # | 功能 | 说明 |
|----|------|------|
| C1 | 对话界面 | 类 ChatGPT 聊天窗口，Markdown 渲染 |
| C2 | 多轮上下文 | 同一会话内记住前文 |
| C3 | 对话历史 | 左侧栏展示、切换、删除历史会话 |
| C4 | 语音输入 | 复用 SenseVoice，说话 → 转文字 → 发给 LLM |
| Q1 | 生产日报 | 一键生成：汇总当日数据为结构化日报 |
| Q2 | 交接班记录 | 基于当班数据，生成结构化交接记录 |
| Q3 | 会议总结 | 复用纪要数据，提取关键结论和行动项 |

---

## 四、技术方案

### 4.1 架构

```
前端 (React/TS)
├── components/AgentChat.tsx    — 对话界面（消息列表、输入框、快捷指令）
├── components/AgentSidebar.tsx — 历史会话列表
└── components/AgentTools.tsx   — 工具结果展示（表格、文件列表）

Tauri 命令层 (Rust)
├── commands/agent.rs           — agent_chat、agent_generate_report、agent_analyze_file
└── agent/ 模块 (新增)
    ├── context.rs  — 上下文窗口管理
    ├── tools.rs    — 工具注册与调用
    ├── memory.rs   — 长期记忆 (agent_memory 表)
    ├── rag.rs      — 本地文档索引
    └── prompt.rs   — 生产场景 Prompt 模板

已有基础设施
Ollama (qwen3.5:4b) · SQLite · Whisper · SenseVoice
```

### 4.2 Tool-Use 机制

Agent 通过 System Prompt 声明可用工具。LLM 返回文本直接展示；返回 `tool_call` 则由 Rust 侧执行后回传结果。

执行流程：
1. 用户输入 → 拼接 System Prompt（工具列表 + 上下文 + 行业角色）
2. 发给 Ollama → 解析响应
3. 有 tool_call → 执行（读文件/查数据库）→ 结果追加到对话历史 → 再次发给 LLM
4. 纯文本 → 直接渲染展示

### 4.3 数据库扩展

```sql
CREATE TABLE agent_conversations (
    id TEXT PRIMARY KEY,
    title TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);

CREATE TABLE agent_messages (
    id TEXT PRIMARY KEY,
    conversation_id TEXT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL DEFAULT '',
    tool_calls TEXT,
    created_at TEXT NOT NULL,
    FOREIGN KEY (conversation_id) REFERENCES agent_conversations(id)
);

CREATE TABLE agent_memory (
    id TEXT PRIMARY KEY,
    key TEXT NOT NULL UNIQUE,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE agent_todos (
    id TEXT PRIMARY KEY,
    conversation_id TEXT,
    content TEXT NOT NULL,
    source_type TEXT,     -- 'chat', 'minutes', 'manual'
    source_id TEXT,       -- meeting_id 或 message_id
    status TEXT NOT NULL DEFAULT 'pending',  -- pending, done, cancelled
    priority INTEGER NOT NULL DEFAULT 0,
    due_date TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE agent_documents (
    id TEXT PRIMARY KEY,
    file_path TEXT NOT NULL,
    file_name TEXT NOT NULL,
    content_hash TEXT NOT NULL,
    chunk_count INTEGER NOT NULL DEFAULT 0,
    indexed_at TEXT NOT NULL
);
```

### 4.4 Prompt 设计

- System Prompt 分层：基础角色（供应链生产岗位）+ 工具声明（动态生成）
- 强制 `think: false`
- 输出约束：关键结论加粗、数据用表格、待办用 checkbox
- 安全边界：不执行删除文件、修改系统设置等操作

---

## 五、UI 草案

```
┌──────────────────────────────────────────────────┐
│  ← 返回工作台    办公助手    [新对话] [历史]      │
├────────────┬─────────────────────────────────────┤
│  历史会话   │                                     │
│            │  AI: 早上好！今天 A 线排了 3 个工单  │
│  ┌──────┐  │  物料齐套状态：正常                   │
│  │ 日报  │  │                                     │
│  └──────┘  │  用户: A 线下午的产量出来了吗？       │
│            │                                     │
│  [新对话]  │  AI: 已读取今日产量数据：             │
│            │  | 产线 | 计划 | 实际 | 达成率 |      │
│            │  | A线  | 500  | 487  | 97.4%  |     │
│            │  [复制] [重新生成]                    │
├────────────┴─────────────────────────────────────┤
│  🎤 │  输入问题...                      │  [发送] │
│  [日报] [交接班] [查数据] [会议总结] [分析文件]   │
└──────────────────────────────────────────────────┘
```

---

## 六、开发计划

| 阶段 | 内容 | 预估 |
|------|------|------|
| **Phase 1** | 对话界面 + 多轮上下文 + 对话历史 + 语音输入 | 1 周 |
| **Phase 2** | 日报/交接班记录生成 + 会议总结 + 待办提取 | 1 周 |
| **Phase 3** | 文件分析（CSV/Excel）+ Tool-Use 框架 + 上下文记忆 | 1-2 周 |
| **Phase 4** | RAG 知识库 + 跨表关联查询 + 预警 | 2-3 周 |

---

## 七、风险

| 风险 | 应对 |
|------|------|
| 本地 4B 模型推理质量不足 | 保留切换大模型能力 |
| Agent 幻觉生成错误数据 | 数据类输出标注来源；人工审核 |
| Token 消耗过大 | 上下文窗口管理（摘要压缩） |
| 缺少 ERP 等系统集成 | Phase 1-2 先聚焦本地文件 + MoM 数据库 |
