// EasyWork - 前端主界面
// 两层导航：Workbench（Agent 入口） ↔ MinutesApp（会议纪要内页）

import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import Workbench from "./workbench/Workbench";
import MinutesApp from "./minutes";
import AgentApp from "./agent/AgentApp";
import ReminderModal from "./ReminderModal";
import { ToastContainer, showToast } from "./components/Toast";
import type { MinutesTab } from "./types";

const MINUTES_TABS: MinutesTab[] = ["today", "history", "schedule", "reports"];
const isMinutesTab = (v: string): v is MinutesTab => MINUTES_TABS.includes(v as MinutesTab);

export default function App() {
  const [view, setView] = useState<"workbench" | "minutes" | "agent">("workbench");
  const [prefillTitle, setPrefillTitle] = useState("");
  const [initialTab, setInitialTab] = useState<MinutesTab>("today");

  // Load startup page setting
  useEffect(() => {
    (async () => {
      try {
        const settings = await invoke<Record<string, string>>("get_settings");
        const startupPage = settings["agent_startup_page"];
        if (startupPage === "minutes") setView("minutes");
        else if (startupPage === "agent") setView("agent");
      } catch {}
    })();
  }, []);

  // Check for updates on startup
  useEffect(() => {
    (async () => {
      try {
        const { check } = await import("@tauri-apps/plugin-updater");
        const { relaunch } = await import("@tauri-apps/plugin-process");
        const update = await check();
        if (update?.available) {
          await update.downloadAndInstall();
          await relaunch();
        }
      } catch {}
    })();
  }, []);

  // Schedule reminder — poll every 2s
  const [reminder, setReminder] = useState<{ id: string; title: string; startTime: string; zoomUrl: string } | null>(null);
  const [currentScheduleId, setCurrentScheduleId] = useState<string | null>(null);

  // Tray menu navigation
  useEffect(() => {
    const unlisten = listen<{ view: string; tab?: string }>("tray-navigate", (e) => {
      const { view, tab } = e.payload;
      if (view === "workbench") {
        setView("workbench");
      } else if (view === "minutes") {
        if (tab && isMinutesTab(tab)) setInitialTab(tab);
        setView("minutes");
      } else if (view === "agent") {
        setView("agent");
      }
    });
    return () => { unlisten.then((f) => f()); };
  }, []);

  // 生产环境禁用浏览器右键菜单
  useEffect(() => {
    if (!import.meta.env.PROD) return;
    const handler = (e: MouseEvent) => e.preventDefault();
    document.addEventListener("contextmenu", handler);
    return () => document.removeEventListener("contextmenu", handler);
  }, []);

  const navigateToRecording = useCallback((title: string, scheduleId?: string) => {
    setPrefillTitle(title);
    setCurrentScheduleId(scheduleId || null);
    setInitialTab("today");
    setView("minutes");
  }, []);

  useEffect(() => {
    const poll = async () => {
      try {
        const r = await invoke<{ id: string; title: string; startTime: string; zoomUrl: string } | null>("get_pending_reminder");
        if (r) setReminder(r);
      } catch (e) {
        console.error("轮询提醒失败", e);
      }
    };

    // Pause polling when the document is hidden (window minimized to tray)
    const onVisibilityChange = () => {
      if (document.hidden) {
        clearInterval(intervalId);
      } else {
        poll();
        intervalId = setInterval(poll, 5000);
      }
    };

    poll();
    let intervalId = setInterval(poll, 2000);
    document.addEventListener("visibilitychange", onVisibilityChange);
    return () => {
      clearInterval(intervalId);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  return (
    <div className="h-screen bg-gray-50">
      {view === "workbench" && (
        <Workbench
          onEnter={(title?: string, action?: string) => {
            if (action === "agent") {
              setView("agent");
            } else {
              setPrefillTitle(title || "");
              setCurrentScheduleId(null);
              setInitialTab(action && isMinutesTab(action) ? action : "today");
              setView("minutes");
            }
          }}
        />
      )}
      {view === "minutes" && (
        <MinutesApp
          prefillTitle={prefillTitle}
          scheduleId={currentScheduleId}
          initialTab={initialTab}
          onBack={() => setView("workbench")}
          onNavigateRecording={navigateToRecording}
        />
      )}
      {view === "agent" && (
        <AgentApp onBack={() => setView("workbench")} />
      )}

      {reminder && (
        <ReminderModal
          reminder={reminder}
          onGo={(r) => {
            setReminder(null);
            navigateToRecording(r.title, r.id);
          }}
          onClose={() => { setReminder(null); invoke("dismiss_reminder"); }}
        />
      )}
      <ToastContainer />
    </div>
  );
}
