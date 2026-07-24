// EasyWork - Agent main layout (sidebar + chat / todo)
import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import AgentSidebar from "./AgentSidebar";
import AgentChat from "./AgentChat";
import AgentTodo from "./AgentTodo";
import type { AgentConversationSummary, TodoItem } from "../types";
import { ERRORS, toUserError } from "../errors";
import { showToast } from "../components/Toast";

export default function AgentApp({ onBack }: { onBack: () => void }) {
  const [conversations, setConversations] = useState<AgentConversationSummary[]>([]);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [agentSubView, setAgentSubView] = useState<"chat" | "todo">("chat");
  const [todos, setTodos] = useState<TodoItem[]>([]);

  const loadConversations = useCallback(() => {
    invoke<AgentConversationSummary[]>("agent_list_conversations")
      .then((list) => {
        setConversations(list);
        if (list.length > 0 && !activeId) {
          setActiveId(list[0].id);
        }
      })
      .catch((e) => {
        console.error(e);
        showToast("加载对话列表失败", "error");
      })
      .finally(() => setLoading(false));
  }, [activeId]);

  const loadTodos = useCallback(() => {
    invoke<TodoItem[]>("todo_list")
      .then(setTodos)
      .catch((e) => {
        console.error(e);
        showToast("加载待办列表失败", "error");
      });
  }, []);

  useEffect(() => { loadConversations(); }, []);
  useEffect(() => { loadTodos(); }, []);

  const handleNew = async () => {
    try {
      const id = await invoke<string>("agent_create_conversation");
      setActiveId(id);
      setAgentSubView("chat");
      loadConversations();
    } catch (e) { console.error(e); showToast("创建对话失败", "error"); }
  };

  const handleSelect = (id: string) => {
    setActiveId(id);
    setAgentSubView("chat");
  };

  const handleDelete = async (id: string) => {
    try {
      await invoke("agent_delete_conversation", { id });
      if (activeId === id) setActiveId(null);
      loadConversations();
    } catch (e) { showToast(toUserError(ERRORS.DELETE_CONVERSATION, e), "error"); }
  };

  const handleRename = async (id: string, title: string) => {
    try {
      await invoke("agent_rename_conversation", { id, title });
      loadConversations();
    } catch (e) { console.error(e); showToast("重命名失败", "error"); }
  };

  const handleTodoToggle = async (id: string, done: boolean) => {
    try {
      await invoke("todo_update_status", { id, status: done ? "done" : "pending" });
      loadTodos();
    } catch (e) { console.error(e); showToast("更新待办状态失败", "error"); }
  };

  const handleTodoDelete = async (id: string) => {
    try {
      await invoke("todo_delete", { id });
      loadTodos();
    } catch (e) { console.error(e); showToast("删除待办失败", "error"); }
  };

  // After chat sends a message, refresh todos (todo may have been created by agent)
  const handleConversationUpdate = () => {
    loadConversations();
    loadTodos();
  };

  if (loading) {
    return (
      <div className="h-full flex items-center justify-center">
        <p className="text-sm text-gray-400">加载中...</p>
      </div>
    );
  }

  if (!activeId && conversations.length === 0 && agentSubView === "chat") {
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
        activeSubView={agentSubView}
        todos={todos}
        onSelect={handleSelect}
        onNew={handleNew}
        onDelete={handleDelete}
        onRename={handleRename}
        onBack={onBack}
        onSubViewChange={setAgentSubView}
        onTodoToggle={handleTodoToggle}
        onTodoDelete={handleTodoDelete}
      />
      <div className="flex-1 flex flex-col min-w-0">
        {agentSubView === "chat" && activeId ? (
          <AgentChat conversationId={activeId} onConversationUpdate={handleConversationUpdate} />
        ) : agentSubView === "chat" ? (
          <div className="flex-1 flex items-center justify-center">
            <p className="text-sm text-gray-400">选择一个对话或创建新对话</p>
          </div>
        ) : (
          <AgentTodo todos={todos} onRefresh={loadTodos} onToggle={handleTodoToggle} onDelete={handleTodoDelete} />
        )}
      </div>
    </div>
  );
}
