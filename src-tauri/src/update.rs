// EasyWork - 应用更新辅助命令

use tauri::AppHandle;

use crate::cleanup_child_process;

/// 更新安装前退出应用：先清理 sidecar 子进程（easywork-agent / llama-server，
/// 否则它们锁住安装目录文件导致 MSI 安装失败 Error 1310），再正常退出
/// 让 msiexec 完成安装。不要使用 relaunch——新进程会再次锁住安装文件。
#[tauri::command]
pub async fn exit_for_update(app: AppHandle) {
    let app2 = app.clone();
    tauri::async_runtime::spawn_blocking(move || cleanup_child_process(&app2))
        .await
        .ok();
    app.exit(0);
}
