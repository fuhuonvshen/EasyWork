# EasyWork 项目地图

> 三层架构：React 前端 → Rust/Tauri 后端 + Python Agent 侧边服务 → SQLite 数据库

---

## 导航与入口

| 文件 | 说明 |
|------|------|
| [App.tsx](src/App.tsx) | 顶层路由：workbench / minutes / agent 三视图切换 + 会议提醒弹窗 |
| [main.tsx](src/main.tsx) | React 入口 |
| [main.rs](src-tauri/src/main.rs) | Rust 入口（禁止控制台窗口 + 启动 Tauri） |
| [lib.rs](src-tauri/src/lib.rs) | Tauri 应用主入口：模块声明、状态注册、初始化编排、命令注册（`generate_handler!`） |

---

## 一、会议系统

### 录制 → 转写 → AI 纪要 → 查看/编辑/导出

#### 前端

- [TodayView.tsx](src/minutes/TodayView.tsx) — 录制面板：选设备 → 开始/停止录制 → 实时转写展示 → 生成纪要弹窗
- [HistoryDetail.tsx](src/minutes/HistoryDetail.tsx) — 查看/编辑单条纪要 + 导出下拉菜单(md/docx/pdf/png)
- [MinutesApp.tsx](src/minutes/MinutesApp.tsx) — 侧边栏(历史列表+搜索+筛选+分页+置顶+管理模式批量删除) + 周报/月报列表
- [ScheduleView.tsx](src/minutes/ScheduleView.tsx) — 日历视图 + 日程CRUD + 生成周报/月报 + 导出 + 新建日程时自动创建待办
- [Markdown.tsx](src/components/Markdown.tsx) — Markdown 渲染组件

#### 后端 (Rust)

- [minutes/meeting.rs](src-tauri/src/minutes/meeting.rs) — `generate_minutes`(转写+LLM纪要), `list_meetings`(分页搜索), `delete_meeting/s`, `toggle_pin_meeting`, `get_meeting_minutes`, `update_meeting_minutes`
- [minutes/schedule.rs](src-tauri/src/minutes/schedule.rs) — `add_scheduled_meeting`, `list_scheduled_meetings`, `delete_scheduled_meeting`, `update_scheduled_meeting`, `find_meeting_by_schedule`
- [minutes/report.rs](src-tauri/src/minutes/report.rs) — `generate_report`(周报/月报), `list_reports`, `delete_report`, `export_report`(转发Python)
- [minutes/reminder.rs](src-tauri/src/minutes/reminder.rs) — `get_pending_reminder`, `dismiss_reminder`(轮询提醒)
- [minutes/zoom.rs](src-tauri/src/minutes/zoom.rs) — `launch_zoom`

#### 后端 (Python Agent — 导出)

- [main.py](src-tauri/py_backend/main.py) — `/export_report` 端点: markdown→docx/PDF/PNG 渲染

---

## 二、AI 办公助手 (Agent)

### 对话式 AI + 待办管理

#### 前端

- [AgentApp.tsx](src/agent/AgentApp.tsx) — 主布局：对话/待办 tab 切换(agentSubView)，列表加载，CRUD透传
- [AgentSidebar.tsx](src/agent/AgentSidebar.tsx) — 左侧边栏：对话列表(重命名/删除) + 待办列表(checkbox/优先级徽标/删除/待办数量角标)
- [AgentChat.tsx](src/agent/AgentChat.tsx) — 聊天区域：消息气泡 + 输入框 + 文件拖拽/选择上传(Excel/CSV/TXT)
- [AgentTodo.tsx](src/agent/AgentTodo.tsx) — 待办完整视图：待完成/已完成分组 + 新建表单(标题/截止日期/优先级) + 空状态提示

#### 后端 (Rust)

- [agent/commands.rs](src-tauri/src/agent/commands.rs) — 代理命令：`agent_chat`, `agent_attach_file`, `agent_attach_content`, `agent_list/create/delete/rename_conversation`, `agent_get_messages` + `todo_create/list/update_status/delete`
- [agent/sidecar.rs](src-tauri/src/agent/sidecar.rs) — Python FastAPI sidecar 进程管理(启动/停止/HTTP代理)
- [agent/mod.rs](src-tauri/src/agent/mod.rs) — 模块组织 + `init()` 初始化

#### 后端 (Python Agent — LLM 业务逻辑)

| 文件 | 说明 |
|------|------|
| [main.py](src-tauri/py_backend/main.py) | FastAPI 应用：`/chat`(ReAct+Plan-then-Execute+Skill系统), `/attach_file`, `/export_report`, 后处理提取todo/schedule JSON |
| [prompt.py](src-tauri/py_backend/prompt.py) | 系统提示词(供应链生产助手 + 当前时间注入) |
| [config.py](src-tauri/py_backend/config.py) | 配置(Ollama URL, 模型名, 路径) |
| [ollama_client.py](src-tauri/py_backend/ollama_client.py) | Ollama LLM API 通信 |
| [skills.py](src-tauri/py_backend/skills.py) | Skill 加载器(从 SKILL.md 读取) |
| [context.py](src-tauri/py_backend/context.py) | 对话上下文构建 |
| [database.py](src-tauri/py_backend/database.py) | Python 侧 SQLite 操作(aiosqlite, 共享 easywork.db) |
| [memory.py](src-tauri/py_backend/memory.py) | 短期对话摘要 |
| [memory_long.py](src-tauri/py_backend/memory_long.py) | 长期记忆 |
| [excel_executor.py](src-tauri/py_backend/excel_executor.py) | Excel 操作工具 |
| [email_utils.py](src-tauri/py_backend/email_utils.py) | 邮件发送工具 |
| [docker_sandbox.py](src-tauri/py_backend/docker_sandbox.py) | Docker 沙箱执行 |

#### Skills

- [skills/todo/SKILL.md](src/agent/skills/todo/SKILL.md) — 待办/日程提取技能定义

---

## 三、语音识别引擎

| 前端 | Rust 后端 | 说明 |
|------|-----------|------|
| [TodayView.tsx](src/minutes/TodayView.tsx) | [audio/capture.rs](src-tauri/src/audio/capture.rs) | WASAPI 系统音频环回捕获 |
| — | [audio/device.rs](src-tauri/src/audio/device.rs) | 音频设备枚举 |
| — | [audio/commands.rs](src-tauri/src/audio/commands.rs) | `list_devices`, `start_capture`, `stop_capture`, `get_transcript_chunks` |
| — | [whisper/](src-tauri/src/whisper/) | Whisper.cpp 封装(引擎+模型管理) |
| — | [sensevoice/](src-tauri/src/sensevoice/) | SenseVoice 封装(中文更优) |
| — | [asr/mod.rs](src-tauri/src/asr/mod.rs) | `asr_check_model`, `asr_list_models` |

---

## 四、本地 LLM 引擎

| 文件 | 说明 |
|------|------|
| [llm/engine.rs](src-tauri/src/llm/engine.rs) | llama.cpp 绑定，用于纪要/报告生成(Rust 侧本地推理) |
| [llm/commands.rs](src-tauri/src/llm/commands.rs) | 模型管理：下载/加载/卸载/删除 |
| [llm/models.rs](src-tauri/src/llm/models.rs) | 模型信息定义 |
| [summary/gen.rs](src-tauri/src/summary/gen.rs) | 纪要生成 prompt |
| [summary/template.rs](src-tauri/src/summary/template.rs) | 纪要格式化模板 |

> **两个 LLM 入口**：Rust 侧(llama.cpp)用于纪要/报告生成，Python 侧(Ollama)用于对话 Agent

---

## 五、数据库层

| 文件 | 说明 |
|------|------|
| [database/mod.rs](src-tauri/src/database/mod.rs) | 初始化连接池 |
| [database/models.rs](src-tauri/src/database/models.rs) | 所有数据模型(meetings, transcripts, minutes, scheduled_meetings, reports, agent_conversations, agent_messages, agent_todos) |
| [database/repo.rs](src-tauri/src/database/repo.rs) | 所有 CRUD(791行，含自动建表+迁移) |

### 表结构

```
meetings              — 会议记录
transcripts           — 语音转写 (FK→meetings)
minutes               — AI 纪要 (FK→meetings)
scheduled_meetings    — 日程安排
reports               — 周报/月报
agent_conversations   — Agent 对话
agent_messages        — 对话消息 (FK→conversations)
agent_todos           — 待办事项
settings              — 设置键值对
```

Rust 和 Python **共享同一个 SQLite 文件**(WAL 模式保障并发安全)。

---

## 六、设置

| 文件 | 说明 |
|------|------|
| [settings/commands.rs](src-tauri/src/settings/commands.rs) | `get_settings`, `update_setting`, `select_folder`, `get_default_paths` |

---

## 七、类型定义(前后端契约)

- [types.ts](src/types.ts) — `AudioDevice`, `MeetingRow`, `ScheduledMeeting`, `ModelInfo`, `MinutesTab`, `AgentConversationSummary`, `AgentMessage`, `TodoItem`

---

## 八、全局状态

- [state.rs](src-tauri/src/state.rs) — 所有 Tauri 托管状态(`DbState`, `WhisperState`, `SenseVoiceState`, `LlmState`, `CaptureState`, `AgentSidecarState`, `AgentProcessState`, `ReminderState` 等)

---

## 关键数据流图解

### 会议录制流程

```
[TodayView] start_capture → Rust WASAPI录系统音频
    → 实时 Whisper/SenseVoice 转写 → get_transcript_chunks 轮询
    → 用户点停止 → stop_capture + generate_minutes
    → Rust 调本地 LLM(local) 生成纪要 → 存库
    → TodayView 显示纪要弹窗
    → 历史记录: MinutesApp → list_meetings → HistoryDetail → get_meeting_minutes
    → 导出: export_report → Rust 转发 Python → base64 → 保存文件
```

### Agent 对话流程

```
[AgentChat] invoke("agent_chat") → Rust 转发 Python /chat
    → Python 构建上下文(历史消息+system prompt+skills)
    → ReAct 循环: 调 Ollama → 解析工具调用 → 执行工具 → 继续
    → 最终回复 → 后处理提取 ```todo```/```schedule``` JSON → 写库
    → 返回前端 → AgentApp 刷新对话+待办列表
```

### 待办联动

```
对话提取: "帮我记一下…" → LLM 输出 ```todo{...}``` → Python insert_todo()
日程同步: ScheduleView 新建日程 → invoke("todo_create") 自动生成待办
手动创建: AgentTodo 新建表单 → invoke("todo_create")
```

---

## 九、模型管理

- [ModelDownloadDialog.tsx](src/minutes/ModelDownloadDialog.tsx) — 模型下载对话框(Whisper/SenseVoice/LLM)
