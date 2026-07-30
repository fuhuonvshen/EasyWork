// EasyWork - Tauri 应用主入口（Rust 侧）
// 职责：声明模块、注册命令、编排初始化流程、管理全局状态。
// 每个模块的初始化逻辑在各自的 mod.rs 中，lib.rs 只负责按序调用和状态注册。

mod agent;
mod asr;
mod audio;
mod diarization;
mod minutes;
mod database;
mod sensevoice;
mod llm;
mod settings;
mod summary;
mod whisper;

mod state;

use state::{
    AgentProcessState, AgentSidecarState, CaptureState, DbState, DiarizationState, KillOnDrop,
    LlmState, ReminderState, SenseVoiceState, TranscriptBufState, TranscriptTaskState, WhisperState,
};
use std::collections::HashMap;
use std::sync::Mutex;
use std::path::PathBuf;
use tauri::{
    image::Image,
    menu::MenuItem,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager,
};

pub fn run() {
    // Suppress whisper.cpp/ggml debug output
    std::env::set_var("GGML_LOG_LEVEL", "0");
    std::env::set_var("GGML_METAL_LOG_LEVEL", "0");

    // Write logs to a writable user data directory
    let log_path = dirs::data_local_dir()
        .map(|d| d.join("easework").join("easework.log"))
        .unwrap_or_else(|| PathBuf::from("easework.log"));
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let log_target: Box<dyn std::io::Write + Send> = match std::fs::File::create(&log_path) {
        Ok(f) => Box::new(f),
        Err(_) => Box::new(std::io::stderr()),
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .target(env_logger::Target::Pipe(log_target))
        .init();
    log::info!("Log file: {:?}", log_path);

    // Capture panics to log file too
    let log_path2 = log_path.clone();
    std::panic::set_hook(Box::new(move |info| {
        let msg = format!("PANIC: {}", info);
        // Try log crate first
        let _ = log::error!("{}", msg);
        // Direct file write as fallback
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&log_path2) {
            use std::io::Write;
            let _ = writeln!(f, "[PANIC] {}", msg);
        }
    }));

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::Builder::new().app_name("EasyWork").arg("--from-autostart").build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // ── Register empty states upfront ──
        .manage(CaptureState(Mutex::new(None)))
        .manage(TranscriptBufState(std::sync::Arc::new(
            std::sync::Mutex::new(Vec::new()),
        )))
        .manage(TranscriptTaskState(Mutex::new(Vec::new())))
        .manage(WhisperState(Mutex::new(None)))
        .manage(SenseVoiceState(Mutex::new(None)))
        .manage(ReminderState(std::sync::Arc::new(
            std::sync::Mutex::new(None),
        )))
        .manage(DiarizationState(Mutex::new(None)))
        .manage(AgentSidecarState(agent::sidecar::AgentSidecar::new(9876)))
        .manage(AgentProcessState(std::sync::Arc::new(
            std::sync::Mutex::new(KillOnDrop(None)),
        )))
        .setup(|app| {
            let app_dir = match app.path().app_data_dir() {
                Ok(d) => d,
                Err(e) => {
                    log::error!("无法获取应用数据目录: {}", e);
                    return Err(Box::new(e));
                }
            };

            // ── 1. Database (fast path, stays synchronous) ──
            let pool = tauri::async_runtime::block_on(database::init(&app_dir));
            let pool = match pool {
                Ok(p) => {
                    app.manage(DbState(p.clone()));
                    log::info!("Database initialized");
                    p
                }
                Err(e) => {
                    log::error!("数据库初始化失败: {}", e);
                    return Err(e.into());
                }
            };

            // ── 2. Read settings (fast) ──
            let settings: HashMap<String, String> = tauri::async_runtime::block_on(
                database::repo::get_all_settings(&pool)
            ).unwrap_or_default();

            // ── 3. Reminder polling (needs pool, starts now) ──
            let reminder_arc = app.state::<ReminderState>().0.clone();
            minutes::spawn_reminder(app.handle().clone(), pool, reminder_arc);

            // ── 4. System tray (sync, instant) ──
            build_tray(app)?;

            // Autostart: hide window so app starts in tray only
            if std::env::args().any(|a| a == "--from-autostart") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }

            // ── 5. Heavy init in background (model loading, sidecar) ──
            let handle = app.handle().clone();
            let bg_app_dir = app_dir.clone();
            tauri::async_runtime::spawn(async move {
                init_background(&handle, &bg_app_dir, &settings).await;
            });

            log::info!("EasyWork 窗口已显示，后台初始化中…");
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            // ── ASR ──
            asr::asr_check_model,
            asr::asr_list_models,
            // ── Audio ──
            audio::commands::list_devices,
            audio::commands::start_capture,
            audio::commands::stop_capture,
            audio::commands::get_transcript_chunks,
            // ── Whisper ──
            whisper::commands::whisper_check_model,
            whisper::commands::whisper_list_models,
            whisper::commands::whisper_delete_model,
            whisper::commands::whisper_get_models_dir,
            whisper::commands::whisper_load_model,
            whisper::commands::whisper_download_model,
            whisper::commands::whisper_download_status,
            whisper::commands::whisper_cancel_download,
            whisper::commands::whisper_unload_model,
            whisper::commands::whisper_transcribe,
            // ── SenseVoice ──
            sensevoice::commands::sv_check_model,
            sensevoice::commands::sv_list_models,
            sensevoice::commands::sv_delete_model,
            sensevoice::commands::sv_load_model,
            sensevoice::commands::sv_download_model,
            sensevoice::commands::sv_cancel_download,
            sensevoice::commands::sv_unload_model,
            sensevoice::commands::sv_transcribe,
            // ── Minutes ──
            minutes::reminder::get_pending_reminder,
            minutes::reminder::dismiss_reminder,
            minutes::meeting::generate_minutes,
            minutes::meeting::update_meeting_minutes,
            minutes::meeting::list_meetings,
            minutes::meeting::delete_meeting,
            minutes::meeting::delete_meetings,
            minutes::meeting::toggle_pin_meeting,
            minutes::meeting::get_meeting_minutes,
            minutes::meeting::get_meeting,
            minutes::meeting::update_meeting_title,
            minutes::schedule::add_scheduled_meeting,
            minutes::schedule::delete_scheduled_meeting,
            minutes::schedule::update_scheduled_meeting,
            minutes::schedule::list_scheduled_meetings,
            minutes::schedule::find_meeting_by_schedule,
            minutes::zoom::launch_zoom,
            minutes::report::generate_report,
            minutes::report::list_reports,
            minutes::report::delete_report,
            minutes::report::export_report,
            // ── LLM ──
            llm::commands::llm_list_models,
            llm::commands::llm_download_model,
            llm::commands::llm_download_status,
            llm::commands::llm_cancel_download,
            llm::commands::llm_delete_model,
            llm::commands::llm_load_model,
            llm::commands::llm_unload_model,
            llm::commands::llm_server_status,
            llm::commands::agent_prepare_llm,
            // ── Agent ──
            agent::commands::agent_attach_file,
            agent::commands::agent_attach_content,
            agent::commands::agent_chat,
            agent::commands::agent_list_conversations,
            agent::commands::agent_create_conversation,
            agent::commands::agent_delete_conversation,
            agent::commands::agent_rename_conversation,
            agent::commands::agent_get_messages,
            agent::commands::todo_create,
            agent::commands::todo_list,
            agent::commands::todo_update_status,
            agent::commands::todo_delete,
            // ── Settings ──
            settings::commands::get_settings,
            settings::commands::update_setting,
            settings::commands::select_folder,
            settings::commands::get_default_paths,
            settings::commands::pick_audio_file,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|app_handle, event| {
        if let tauri::RunEvent::Exit = event {
            cleanup_child_process(app_handle);
            log::info!("EasyWork 已退出");
        }
    });
}

// ── Background initialization ─────────────────────────────────
// Runs after the window is displayed so model loading doesn't block the UI.
async fn init_background(app_handle: &tauri::AppHandle, app_dir: &std::path::Path, settings: &HashMap<String, String>) {
    let settings = settings.clone();
    let app_dir = app_dir.to_path_buf();

    // ── Agent sidecar (独立启动，不依赖 AI 引擎) ──
    let handle_for_agent = app_handle.clone();
    let settings_for_agent = settings.clone();
    let app_dir_for_agent = app_dir.clone();
    tauri::async_runtime::spawn(async move {
        start_agent_sidecar(&handle_for_agent, &app_dir_for_agent, &settings_for_agent).await;
    });

    // 1. Whisper engine
    let whisper_dir = settings::resolve_path(&app_dir, &settings, "whisper_models_dir", "models");
    match whisper::init(&whisper_dir).await {
        Ok(engine) => {
            if let Ok(mut guard) = app_handle.state::<WhisperState>().0.lock() {
                *guard = Some(engine);
            }
            emit_init_status(app_handle, "whisper", "ok", "Whisper 引擎就绪");
            log::info!("Whisper engine initialized");
        }
        Err(e) => {
            emit_init_status(app_handle, "whisper", "error", &format!("Whisper 引擎初始化失败: {}", e));
            log::error!("Whisper engine init failed: {}", e);
        }
    }

    // 2. SenseVoice engine
    let sv_dir = settings::resolve_path(&app_dir, &settings, "sensevoice_models_dir", "sensevoice_models");
    match sensevoice::init(&sv_dir).await {
        Ok(engine) => {
            if let Ok(mut guard) = app_handle.state::<SenseVoiceState>().0.lock() {
                *guard = Some(engine);
            }
            emit_init_status(app_handle, "sensevoice", "ok", "SenseVoice 引擎就绪");
            log::info!("SenseVoice engine initialized");
        }
        Err(e) => {
            emit_init_status(app_handle, "sensevoice", "error", &format!("SenseVoice 引擎初始化失败: {}", e));
            log::error!("SenseVoice engine init failed: {}", e);
        }
    }

    // 3. Silero VAD model (bundled in release, copied from resources or dev dir)
    ensure_vad_model(&app_dir, app_handle).await;

    // 4. Speaker diarization engine (auto-downloads model if missing)
    let diarize_dir = app_dir.join("speaker_embedding");
    match diarization::ensure_model_downloaded(&diarize_dir).await {
        Ok(model_path) => {
            match diarization::DiarizationEngine::new(&model_path) {
                Ok(engine) => {
                    let engine = std::sync::Arc::new(engine);
                    if let Ok(mut guard) = app_handle.state::<DiarizationState>().0.lock() {
                        *guard = Some(engine);
                    }
                    emit_init_status(app_handle, "diarization", "ok", "说话人区分引擎就绪");
                    log::info!("Diarization engine initialized");
                }
                Err(e) => {
                    emit_init_status(app_handle, "diarization", "error",
                        &format!("说话人区分引擎初始化失败: {}", e));
                    log::error!("Diarization engine init failed: {}", e);
                }
            }
        }
        Err(e) => {
            emit_init_status(app_handle, "diarization", "warning",
                &format!("声纹模型不可用，将使用固定说话人标签: {}", e));
            log::warn!("Diarization model unavailable (fixed speaker labels will be used): {}", e);
        }
    }

    // 5. LLM engine
    let llm_dir = settings::resolve_path(&app_dir, &settings, "llm_models_dir", "llm_models");
    let bin_dir = settings::resolve_path(&app_dir, &settings, "", "bin");
    let resource_dir = app_handle.path().resource_dir().ok();
    let dev_bin_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    match llm::init(&llm_dir, &bin_dir, resource_dir.as_deref(), Some(&dev_bin_dir)).await {
        Ok(engine) => {
            app_handle.manage(LlmState(engine));
            emit_init_status(app_handle, "llm", "ok", "LLM 引擎就绪");
            log::info!("LLM engine initialized");
        }
        Err(e) => {
            emit_init_status(app_handle, "llm", "error", &format!("LLM 引擎初始化失败: {}", e));
            log::warn!("LLM engine 初始化失败，纪要生成功能不可用: {}", e);
        }
    }

    emit_init_status(app_handle, "all", "done", "后台初始化完成");
    log::info!("EasyWork 后台初始化完成");
}

/// Start the Python agent sidecar independently from AI engine loading.
/// This ensures the agent is available as soon as possible, even if
/// Whisper / SenseVoice / LLM engine loading is slow or fails.
async fn start_agent_sidecar(app_handle: &tauri::AppHandle, app_dir: &std::path::Path, settings: &HashMap<String, String>) {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project_dir = manifest_dir.parent().map_or_else(
        || manifest_dir.clone(),
        |p| p.to_path_buf(),
    );
    let db_path = app_dir.join("easework.db");

    // Dynamically allocate port if default is occupied
    let default_port = 9876u16;
    let port = agent::sidecar::find_available_port(default_port, 10)
        .unwrap_or(default_port);
    if port != default_port {
        app_handle.state::<AgentSidecarState>().0.set_port(port).await;
        log::warn!("Agent port {} occupied, using {} instead", default_port, port);
    }

    // Find bundled agent executable (PyInstaller-built) if available
    let agent_exe_name = if cfg!(target_os = "windows") { "easywork-agent.exe" } else { "easywork-agent" };
    let bundled_agent = {
        // Development: check project binaries/ directory
        let dev_path = manifest_dir.join("binaries").join(agent_exe_name);
        if dev_path.exists() {
            Some(dev_path)
        } else {
            // Release: check resource directory (Tauri preserves relative path)
            app_handle.path().resource_dir()
                .ok()
                .map(|d| d.join("binaries").join(agent_exe_name))
                .filter(|p| p.exists())
        }
    };

    match agent::init(&project_dir, &manifest_dir, &db_path, port, settings, bundled_agent.as_deref()).await {
        Ok((_, Some(child))) => {
            match app_handle.state::<AgentProcessState>().0.try_lock() {
                Ok(mut guard) => guard.0 = Some(child),
                Err(e) => log::error!("无法获取 AgentProcessState 锁: {:?}", e),
            }
            emit_init_status(app_handle, "agent", "ok", &format!("Agent sidecar 就绪 (端口 {})", port));
            log::info!("Agent sidecar initialized on port {}", port);
        }
        Ok((_, None)) => {
            emit_init_status(app_handle, "agent", "error", "Agent 进程已退出（健康检查未通过）");
            log::warn!("Agent sidecar 创建成功但未通过健康检查");
        }
        Err(e) => {
            emit_init_status(app_handle, "agent", "error", &format!("Agent 启动失败: {}", e));
            log::error!("Agent sidecar init failed: {}", e);
        }
    }

}

fn emit_init_status(app_handle: &tauri::AppHandle, module: &str, status: &str, message: &str) {
    let _ = app_handle.emit("init-status", serde_json::json!({
        "module": module,
        "status": status,
        "message": message,
    }));
}

// ── Silero VAD model ─────────────────────────────────────────
// The model is small (~2 MB) and bundled in the release.

async fn ensure_vad_model(app_dir: &std::path::Path, app_handle: &tauri::AppHandle) {
    let dest = app_dir.join("silero_vad.onnx");
    if dest.exists() {
        log::info!("Silero VAD model ready");
        return;
    }
    // Try local models/ directory (dev tree)
    let manifest_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let local = manifest_dir.join("models").join("silero_vad.onnx");
    if local.exists() {
        if let Err(e) = std::fs::copy(&local, &dest) {
            log::error!("复制 Silero VAD 模型失败: {}", e);
        } else {
            log::info!("Silero VAD model copied from src-tauri/models/");
            return;
        }
    }
    // Download from GitHub (release build)
    log::info!("Downloading Silero VAD model (~2 MB)...");
    let url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx";
    match reqwest::get(url).await {
        Ok(resp) => match resp.bytes().await {
            Ok(bytes) => {
                if let Err(e) = std::fs::write(&dest, &bytes) {
                    log::error!("写入 Silero VAD 模型失败: {}", e);
                } else {
                    log::info!("Silero VAD 模型已下载 ({:.1} KB)", bytes.len() as f64 / 1024.0);
                }
            }
            Err(e) => log::error!("读取 Silero VAD 响应失败: {}", e),
        },
        Err(e) => log::warn!("下载 Silero VAD 模型失败 (转录将不可用): {}", e),
    }
}

// ── Cleanup ──────────────────────────────────────────────────

fn cleanup_child_process(app_handle: &tauri::AppHandle) {
    // Kill agent sidecar process
    match app_handle.state::<AgentProcessState>().0.try_lock() {
        Ok(mut guard) => {
            if let Some(mut child) = guard.0.take() {
                let _ = child.start_kill();
                log::info!("Agent 子进程已终止");
            }
        }
        Err(e) => log::error!("cleanup: 无法获取 AgentProcessState 锁: {:?}", e),
    }

    // Windows fallback: kill all easywork-agent.exe processes (PyInstaller
    // bootstrap exits early, so the tracked Child handle may not cover the
    // actual Python process tree).
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("taskkill")
            .args(&["/f", "/im", "easywork-agent.exe"])
            .output();
    }

    // Close database pool (fire-and-forget; SQLite 已提交事务是 crash-safe 的)
    let pool = &app_handle.state::<DbState>().0;
    tauri::async_runtime::spawn({
        let pool = pool.clone();
        async move {
            pool.close().await;
            log::info!("数据库连接池已关闭");
        }
    });
}

// ── System tray ─────────────────────────────────────────────

fn build_tray(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let home = MenuItem::with_id(app, "home", "打开主页面", true, None::<&str>)?;
    let meeting = MenuItem::with_id(app, "meeting", "加入会议", true, None::<&str>)?;
    let chat = MenuItem::with_id(app, "chat", "开启对话", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = tauri::menu::Menu::with_items(app, &[&home, &meeting, &chat, &quit])?;

    // Load tray icon from PNG (embedded at compile time)
    let icon_bytes = include_bytes!("../icons/tray.png");
    let img = image::load_from_memory(icon_bytes)
        .expect("failed to decode tray icon")
        .into_rgba8();
    let (w, h) = img.dimensions();
    let icon = Image::new_owned(img.into_raw(), w, h);

    TrayIconBuilder::new()
        .icon(icon)
        .tooltip("EasyWork 办公助手")
        .menu(&menu)
        .on_menu_event(move |app, event| match event.id.as_ref() {
            "home" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                let _ = app.emit("tray-navigate", serde_json::json!({"view": "workbench"}));
            }
            "meeting" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                let _ = app.emit("tray-navigate", serde_json::json!({"view": "minutes", "tab": "today"}));
            }
            "chat" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
                let _ = app.emit("tray-navigate", serde_json::json!({"view": "agent"}));
            }
            "quit" => {
                cleanup_child_process(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
