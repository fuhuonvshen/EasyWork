// EasyWork - Audio capture commands

use tauri::{AppHandle, Emitter, Manager, State};
use crate::audio::capture::{AudioCapture, CaptureConfig};
use crate::audio::device::{list_output_devices, AudioDevice};
use crate::diarization::DiarizationEngine;
use crate::state::{
    CaptureState, DbState, DiarizationState, SenseVoiceState, TranscriptBufState,
    TranscriptTaskState, WhisperState,
};
use std::sync::Arc;

#[tauri::command]
pub fn list_devices() -> Result<Vec<AudioDevice>, String> {
    list_output_devices()
}

#[tauri::command]
pub async fn start_capture(
    device_name: String,
    label: String,
    capture_state: State<'_, CaptureState>,
    whisper_state: State<'_, WhisperState>,
    sensevoice_state: State<'_, SenseVoiceState>,
    diarization_state: State<'_, DiarizationState>,
    task_state: State<'_, TranscriptTaskState>,
    transcript_buf: State<'_, TranscriptBufState>,
    db: State<'_, DbState>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    // Lock scope: check no capture is running, then release before any .await
    {
        let guard = capture_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        if guard.is_some() {
            return Err("已有录制正在进行".into());
        }
    }

    let app_data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录: {}", e))?;

    // 读取所有设置，用于 resolve_path（支持 data_root_dir + 旧版单独设置）
    let settings = crate::database::repo::get_all_settings(&db.0)
        .await
        .unwrap_or_default();
    let output_dir = crate::settings::resolve_path(
        &app_data_dir, &settings, "recordings_dir", "recordings",
    );
    std::fs::create_dir_all(&output_dir).map_err(|e| format!("无法创建录制目录: {}", e))?;

    let mut capture = AudioCapture::start(CaptureConfig {
        device_name,
        output_dir,
        label,
    })
    .map_err(|e| format!("启动录制失败: {}", e))?;

    let audio_rx =
        std::mem::replace(&mut capture.audio_rx, tokio::sync::mpsc::unbounded_channel().1);
    let mic_audio_rx =
        std::mem::replace(&mut capture.mic_audio_rx, tokio::sync::mpsc::unbounded_channel().1);
    let sample_rate = capture.sample_rate;
    let channels = capture.channels;
    let app_for_task = app.clone();

    let whisper_engine = {
        let wg = whisper_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        wg.as_ref().ok_or("Whisper 引擎未初始化")?.clone()
    };

    // Auto-load ASR model: prefer SenseVoice (better Chinese accuracy), fallback to Whisper
    let sv_engine = {
        let sv = sensevoice_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        sv.clone()
    };

    let use_sensevoice = if let Some(ref sv) = sv_engine {
        if !sv.is_loaded().await {
            let _ = sv.ensure_model_loaded().await;
        }
        sv.is_loaded().await
    } else {
        false
    };

    if !use_sensevoice {
        if !whisper_engine.is_loaded().await {
            let _ = whisper_engine.ensure_model_loaded().await;
        }
    }
    let sv_engine = if use_sensevoice { sv_engine } else { None };
    log::info!("Transcription engine: {}", if use_sensevoice { "SenseVoice" } else { "Whisper" });

    // Read diarization engine (optional — fall back to fixed labels if unavailable)
    let diarize_engine: Option<Arc<DiarizationEngine>> = {
        let guard = diarization_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        guard.clone()
    };
    if diarize_engine.is_some() {
        log::info!("说话人区分引擎已接入");
    } else {
        log::info!("说话人区分引擎未启用，将使用固定标签");
    }

    // Silero VAD model path
    let app_data = app.path().app_data_dir().expect("app data dir");
    let vad_model = app_data.join("silero_vad.onnx");

    let buf = transcript_buf.0.clone();
    let spawn_transcriber = |mut rx: tokio::sync::mpsc::UnboundedReceiver<Vec<f32>>,
                              default_speaker: &'static str,
                              app_handle: tauri::AppHandle,
                              diarize: Option<Arc<DiarizationEngine>>| {
        let buf_clone = buf.clone();
        let w_engine = whisper_engine.clone();
        let sv_engine_opt = sv_engine.clone();
        let prefer_sv = use_sensevoice;
        let model_path = vad_model.clone();

        tauri::async_runtime::spawn(async move {
            let vad_config = sherpa_onnx::VadModelConfig {
                silero_vad: sherpa_onnx::SileroVadModelConfig {
                    model: Some(model_path.to_string_lossy().to_string()),
                    threshold: 0.5,
                    min_silence_duration: 0.4,
                    min_speech_duration: 0.5,
                    max_speech_duration: 15.0,
                    ..Default::default()
                },
                sample_rate: 16000,
                num_threads: 2,
                ..Default::default()
            };

            let vad = match sherpa_onnx::VoiceActivityDetector::create(&vad_config, 30.0) {
                Some(v) => v,
                None => {
                    log::error!("[{}] Failed to create Silero VAD", default_speaker);
                    return;
                }
            };
            log::info!("[{}] Silero VAD ready", default_speaker);

            let mut was_speaking = false;

            loop {
                match tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv()).await {
                    Ok(Some(chunk)) => {
                        let audio = crate::whisper::engine::convert_audio_for_whisper(
                            &chunk, channels, sample_rate,
                        );
                        vad.accept_waveform(&audio);
                        was_speaking = true;
                    }
                    Ok(None) => {
                        vad.flush();
                        break;
                    }
                    Err(_) => {
                        if was_speaking {
                            let silence = vec![0.0f32; 1600]; // 100ms of silence
                            vad.accept_waveform(&silence);
                            was_speaking = vad.detected();
                        }
                    }
                }

                let mut transcribed = false;
                while let Some(segment) = vad.front() {
                    let samples = segment.samples().to_vec();
                    let dur = samples.len() as f32 / 16000.0;
                    vad.pop();
                    if dur < 0.5 { continue; }

                    // ── Speaker diarization ─────────────────────────────
                    let speaker_label = if let Some(ref engine) = diarize {
                        engine.diarize(&samples).unwrap_or_else(|| {
                            log::debug!("声纹提取未返回结果，使用默认标签");
                            default_speaker.to_string()
                        })
                    } else {
                        default_speaker.to_string()
                    };

                    transcribed = true;
                    let w = w_engine.clone();
                    let sv = sv_engine_opt.clone();
                    let app = app_handle.clone();
                    let buf = buf_clone.clone();
                    let spk = speaker_label.clone();
                    let use_sv = prefer_sv;
                    tauri::async_runtime::spawn(async move {
                        let mut r = if use_sv && sv.is_some() {
                            sv.as_ref().unwrap().transcribe(&samples).await
                        } else {
                            w.transcribe(&samples).await
                        };
                        if let Err(ref _e) = r {
                            if use_sv && sv.is_some() { r = w.transcribe(&samples).await; }
                            else if sv.is_some() { r = sv.as_ref().unwrap().transcribe(&samples).await; }
                        }
                        if let Ok(t) = r {
                            if !t.trim().is_empty() {
                                log::info!("[{}] {:.1}s '{}'", spk, dur, t.trim());
                                let c = serde_json::json!({"speaker": spk, "text": t.trim()});
                                let _ = app.emit("transcript-chunk", &c);
                                if let Ok(mut g) = buf.lock() { g.push(c); }
                            }
                        }
                    });
                }
                if transcribed {
                    tokio::task::yield_now().await;
                }
            }

            // Final flush
            vad.flush();
            while let Some(segment) = vad.front() {
                let samples = segment.samples().to_vec();
                let dur = samples.len() as f32 / 16000.0;
                vad.pop();
                if dur < 0.5 { continue; }

                let speaker_label = if let Some(ref engine) = diarize {
                    engine.diarize(&samples).unwrap_or_else(|| default_speaker.to_string())
                } else {
                    default_speaker.to_string()
                };

                let w = w_engine.clone();
                let sv = sv_engine_opt.clone();
                let app = app_handle.clone();
                let buf = buf_clone.clone();
                let spk = speaker_label.clone();
                let use_sv = prefer_sv;
                tauri::async_runtime::spawn(async move {
                    let mut r = if use_sv && sv.is_some() {
                        sv.as_ref().unwrap().transcribe(&samples).await
                    } else {
                        w.transcribe(&samples).await
                    };
                    if let Err(ref _e) = r {
                        if use_sv && sv.is_some() { r = w.transcribe(&samples).await; }
                        else if sv.is_some() { r = sv.as_ref().unwrap().transcribe(&samples).await; }
                    }
                    if let Ok(t) = r {
                        if !t.trim().is_empty() {
                            log::info!("[{}] {:.1}s '{}'", spk, dur, t.trim());
                            let c = serde_json::json!({"speaker": spk, "text": t.trim()});
                            let _ = app.emit("transcript-chunk", &c);
                            if let Ok(mut g) = buf.lock() { g.push(c); }
                        }
                    }
                });
            }

            log::info!("[{}] Silero VAD transcriber stopped", default_speaker);
        })
    };

    let handle1 = spawn_transcriber(audio_rx, "发言人", app_for_task.clone(), diarize_engine);
    let handle2 = spawn_transcriber(mic_audio_rx, "我", app_for_task, None);

    {
        let mut tg = task_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        *tg = vec![handle1, handle2];
    }

    {
        let mut guard = capture_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        *guard = Some(capture);
    }
    Ok(())
}

#[tauri::command]
pub fn stop_capture(
    capture_state: State<'_, CaptureState>,
    task_state: State<'_, TranscriptTaskState>,
) -> Result<String, String> {
    {
        let mut tg = task_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
        for handle in tg.drain(..) {
            handle.abort();
        }
    }

    let mut guard = capture_state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
    let capture = guard.take().ok_or("没有正在进行的录制")?;
    let path = capture.stop().map_err(|e| format!("停止录制失败: {}", e))?;
    Ok(path.to_string_lossy().to_string())
}

#[tauri::command]
pub fn get_transcript_chunks(
    state: State<'_, TranscriptBufState>,
) -> Result<Vec<serde_json::Value>, String> {
    let mut guard = state.0.lock().map_err(|e| format!("状态锁失败: {}", e))?;
    let chunks: Vec<serde_json::Value> = guard.drain(..).collect();
    Ok(chunks)
}

/// Check whether the VAD and diarization models are ready for recording.
/// Frontend calls this when entering the meeting module to decide whether to show a download dialog.
#[tauri::command]
pub fn check_meeting_models(app: AppHandle) -> Result<serde_json::Value, String> {
    let app_dir = app.path().app_data_dir().map_err(|e| format!("{:?}", e))?;

    let vad_path = app_dir.join("silero_vad.onnx");
    let vad_ready = std::fs::metadata(&vad_path)
        .map(|m| m.len() > 10_000)
        .unwrap_or(false);

    let diar_path = app_dir.join("speaker_embedding")
        .join("3dspeaker_speech_eres2net_base_sv_zh-cn_3dspeaker_16k")
        .join("model.onnx");
    let diarization_ready = diar_path.exists();

    Ok(serde_json::json!({
        "vadReady": vad_ready,
        "diarizationReady": diarization_ready,
        "bothReady": vad_ready && diarization_ready,
    }))
}
