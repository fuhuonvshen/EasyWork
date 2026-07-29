// EasyWork - 更新提示弹窗
import { useState } from "react";

interface UpdateInfo {
  version: string;
  body: string;
  onInstall: () => Promise<void>;
  onDismiss: () => void;
}

export default function UpdateDialog({ version, body, onInstall, onDismiss }: UpdateInfo) {
  const [installing, setInstalling] = useState(false);

  const handleInstall = async () => {
    setInstalling(true);
    try {
      await onInstall();
    } finally {
      setInstalling(false);
    }
  };

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-black/30">
      <div className="w-full max-w-md rounded-xl bg-white p-6 shadow-2xl">
        <div className="mb-4 flex items-start justify-between">
          <div>
            <h2 className="text-lg font-semibold text-gray-900">发现新版本</h2>
            <p className="mt-0.5 text-sm text-gray-500">EasyWork v{version} 可用</p>
          </div>
          <button
            onClick={onDismiss}
            className="rounded-lg p-1 text-gray-400 hover:bg-gray-100 hover:text-gray-600"
          >
            <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="mb-5 max-h-40 overflow-y-auto rounded-lg bg-gray-50 p-3 text-sm text-gray-600 whitespace-pre-wrap">
          {body || "暂无更新说明"}
        </div>

        <div className="flex justify-end gap-3">
          <button
            onClick={onDismiss}
            className="rounded-lg px-4 py-2 text-sm font-medium text-gray-600 hover:bg-gray-100"
          >
            稍后再说
          </button>
          <button
            onClick={handleInstall}
            disabled={installing}
            className="rounded-lg bg-blue-600 px-4 py-2 text-sm font-medium text-white hover:bg-blue-700 disabled:opacity-50"
          >
            {installing ? "更新中…" : "立即更新"}
          </button>
        </div>
      </div>
    </div>
  );
}
