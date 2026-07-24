// EasyWork - History Detail (view/edit meeting minutes + title)
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ArrowLeft, Loader, Pencil, Check, X } from "lucide-react";
import Markdown from "../../components/Markdown";
import ExportDropdown from "../../components/ExportDropdown";
import { ERRORS, toUserError } from "../../errors";

interface MeetingDetail {
  id: string;
  title: string;
  content: string;
}

export default function HistoryDetail({ meetingId, onBack }: { meetingId: string; onBack: () => void }) {
  const [detail, setDetail] = useState<MeetingDetail | null>(null);
  const [loading, setLoading] = useState(true);
  const [editing, setEditing] = useState(false);
  const [editContent, setEditContent] = useState("");
  const [editingTitle, setEditingTitle] = useState(false);
  const [editTitle, setEditTitle] = useState("");
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    setLoading(true);
    setEditing(false);
    setEditingTitle(false);
    invoke<MeetingDetail>("get_meeting", { meetingId })
      .then((d) => { setDetail(d); setEditContent(d.content); setEditTitle(d.title); })
      .catch(() => setDetail({ id: meetingId, title: ERRORS.LOAD_MINUTES, content: "" }))
      .finally(() => setLoading(false));
  }, [meetingId]);

  const handleSave = async () => {
    setSaving(true);
    setSaveError(null);
    try {
      await invoke("update_meeting_minutes", { meetingId, content: editContent });
      setDetail((prev) => prev ? { ...prev, content: editContent } : prev);
      setEditing(false);
    } catch (e) {
      setSaveError(toUserError(ERRORS.SAVE_MINUTES, e));
    }
    setSaving(false);
  };

  const handleSaveTitle = async () => {
    const trimmed = editTitle.trim();
    if (!trimmed || !detail) return;
    try {
      await invoke("update_meeting_title", { meetingId, title: trimmed });
      setDetail({ ...detail, title: trimmed });
      setEditingTitle(false);
    } catch (e) {
      setSaveError(toUserError(ERRORS.SAVE_MINUTES, e));
    }
  };

  return (
    <>
      <header className="px-8 py-6 border-b border-gray-100 bg-white flex items-center justify-between">
        <div className="flex items-center gap-4 min-w-0">
          <button onClick={onBack} className="p-1.5 rounded-lg hover:bg-gray-100 text-gray-400 hover:text-gray-600 transition-colors flex-shrink-0">
            <ArrowLeft size={18} />
          </button>
          <div className="min-w-0">
            {editingTitle ? (
              <div className="flex items-center gap-2">
                <input
                  type="text"
                  value={editTitle}
                  onChange={(e) => setEditTitle(e.target.value)}
                  className="text-xl font-semibold text-gray-900 bg-gray-50 border border-gray-300 rounded-lg px-3 py-1 outline-none focus:ring-2 focus:ring-accent-300 w-full max-w-md"
                  autoFocus
                  onKeyDown={(e) => { if (e.key === "Enter") handleSaveTitle(); if (e.key === "Escape") { setEditingTitle(false); setEditTitle(detail?.title || ""); } }}
                />
                <button onClick={handleSaveTitle} className="p-1.5 rounded-lg hover:bg-green-50 text-green-600 transition-colors">
                  <Check size={16} />
                </button>
                <button onClick={() => { setEditingTitle(false); setEditTitle(detail?.title || ""); }} className="p-1.5 rounded-lg hover:bg-gray-100 text-gray-400 transition-colors">
                  <X size={16} />
                </button>
              </div>
            ) : (
              <div className="flex items-center gap-2 group">
                <h2 className="text-xl font-semibold text-gray-900 truncate">{detail?.title || "会议纪要"}</h2>
                {!loading && detail && (
                  <button
                    onClick={() => { setEditingTitle(true); setEditTitle(detail.title); }}
                    className="p-1 rounded-lg opacity-0 group-hover:opacity-100 hover:bg-gray-100 text-gray-400 hover:text-gray-600 transition-all"
                    title="重命名"
                  >
                    <Pencil size={14} />
                  </button>
                )}
              </div>
            )}
            {!editingTitle && (
              <p className="text-sm text-gray-400 mt-0.5">AI 生成的会议摘要</p>
            )}
          </div>
        </div>
        {!loading && detail && !editing && (
          <div className="flex items-center gap-2">
            <ExportDropdown content={detail.content} />
            <button
              onClick={() => setEditing(true)}
              className="px-4 py-2 text-sm font-medium text-accent-600 bg-accent-50 rounded-lg hover:bg-accent-100 transition-colors"
            >
              编辑纪要
            </button>
          </div>
        )}
        {editing && (
          <div className="flex items-center gap-2">
            <button
              onClick={() => { setEditing(false); setEditContent(detail?.content || ""); }}
              className="px-4 py-2 text-sm font-medium text-gray-600 bg-gray-100 rounded-lg hover:bg-gray-200 transition-colors"
            >
              取消
            </button>
            <button
              onClick={handleSave}
              disabled={saving}
              className="px-4 py-2 text-sm font-medium text-white bg-accent-600 rounded-lg hover:bg-accent-700 disabled:opacity-50 transition-colors"
            >
              {saving ? "保存中..." : "保存"}
            </button>
          </div>
        )}
      </header>

      {saveError && (
        <div className="mx-8 mt-4 p-3 rounded-lg bg-red-50 border border-red-100 text-sm text-red-700 flex items-center justify-between">
          <span>{saveError}</span>
          <button onClick={() => setSaveError(null)} className="underline hover:no-underline text-red-500 ml-2">关闭</button>
        </div>
      )}

      <div className="flex-1 overflow-y-auto px-8 py-6">
        {loading && (
          <div className="flex items-center gap-3 text-sm text-gray-400">
            <Loader size={16} className="animate-spin" />
            加载中...
          </div>
        )}
        {!loading && !editing && detail && (
          detail.content ? <Markdown content={detail.content} />
            : <p className="text-sm text-gray-400 text-center pt-16">会议内容为空</p>
        )}
        {!loading && !editing && !detail && (
          <p className="text-sm text-gray-400 text-center pt-16">会议内容为空</p>
        )}
        {!loading && editing && (
          <textarea
            value={editContent}
            onChange={(e) => setEditContent(e.target.value)}
            className="w-full h-full min-h-[400px] text-sm text-gray-700 leading-relaxed resize-none border border-gray-200 rounded-lg p-4 outline-none focus:ring-2 focus:ring-accent-300 font-sans"
          />
        )}
      </div>
    </>
  );
}