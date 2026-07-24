// EasyWork - LLM 推理引擎
// 管理 llama-server 进程生命周期 + GGUF 模型下载 + 推理调用
//
// 架构：
//   LlmEngine (全局单例)
//     ├─ 复制捆绑的 llama-server 二进制到数据目录
//     ├─ 下载 GGUF 模型文件（HuggingFace，参考 whisper/engine.rs 模式）
//     ├─ 启动/停止 llama-server 子进程
//     └─ 通过 HTTP 调 /v1/chat/completions 做推理

use anyhow::{Context, Result};
use futures_util::StreamExt;
use reqwest::Client;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

use crate::llm::models;

// ── Constants ──

const LLAMA_SERVER_PORT: u16 = 11435;
const REQUEST_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(600);
const SERVER_START_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(180);

// ── Engine ──

pub struct LlmEngine {
    pub models_dir: PathBuf,
    pub bin_dir: PathBuf,
    pub server_url: String,
    pub gpu_layers: u32,

    // Server process
    server_process: Arc<Mutex<Option<tokio::process::Child>>>,
    /// Which model name (e.g. "qwen3.5:2b") is loaded, if any
    pub current_model: RwLock<Option<String>>,

    // Download state (polled by frontend, same pattern as WhisperEngine)
    cancel_download: AtomicBool,
    download_progress: AtomicU8,
    download_status: Mutex<Option<String>>,
    downloaded_bytes: AtomicU64,
    total_bytes: AtomicU64,
    download_speed: AtomicU64,
}

impl LlmEngine {
    /// Create engine. `models_dir` is where GGUF files go (configurable).
    /// `bin_dir` is where the llama-server binary goes (always under app data).
    pub fn new(models_dir: PathBuf, bin_dir: PathBuf) -> Self {
        std::fs::create_dir_all(&models_dir).ok();
        std::fs::create_dir_all(&bin_dir).ok();

        Self {
            models_dir,
            bin_dir,
            server_url: format!("http://127.0.0.1:{}", LLAMA_SERVER_PORT),
            gpu_layers: 0,  // Updated after binary copy in init()
            server_process: Arc::new(Mutex::new(None)),
            current_model: RwLock::new(None),
            cancel_download: AtomicBool::new(false),
            download_progress: AtomicU8::new(0),
            download_status: Mutex::new(None),
            downloaded_bytes: AtomicU64::new(0),
            total_bytes: AtomicU64::new(0),
            download_speed: AtomicU64::new(0),
        }
    }

    // ── Binary management ──

    /// Path to llama-server binary
    pub fn bin_path(&self) -> PathBuf {
        self.bin_dir.join(if cfg!(target_os = "windows") { "llama-server.exe" } else { "llama-server" })
    }

    /// Check if llama-server binary exists (and DLL on Windows)
    pub fn is_binary_ready(&self) -> bool {
        if !self.bin_path().exists() {
            return false;
        }
        #[cfg(target_os = "windows")]
        {
            if !self.bin_dir.join("llama-server-impl.dll").exists() {
                return false;
            }
        }
        true
    }

    /// Copy the bundled llama-server binary (and all companion DLLs) to bin_dir.
    /// Called once during app setup.
    pub fn copy_from_bundle(&self, bundle_path: &std::path::Path) -> Result<()> {
        if self.is_binary_ready() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.bin_dir)?;

        if !bundle_path.exists() {
            return Err(anyhow::anyhow!("bundle path not found: {:?}", bundle_path));
        }

        let mut found_exe = false;
        for entry in std::fs::read_dir(bundle_path)
            .context("读取 bundle 目录失败")?
        {
            let entry = entry?;
            let fname = entry.file_name();
            let name = fname.to_string_lossy();

            if cfg!(target_os = "windows") && (name.ends_with(".exe") || name.ends_with(".dll")) {
                let dst = self.bin_dir.join(&*name);
                std::fs::copy(entry.path(), &dst)?;
                if name == "llama-server.exe" {
                    found_exe = true;
                    log::info!("llama-server copied to {:?}", dst);
                }
            } else if name == "llama-server" || name == "llama-server-impl.dll" {
                let dst = self.bin_dir.join(&*name);
                std::fs::copy(entry.path(), &dst)?;
                found_exe = true;
            }
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(meta) = std::fs::metadata(self.bin_path()) {
                let mut perms = meta.permissions();
                perms.set_mode(0o755);
                let _ = std::fs::set_permissions(self.bin_path(), perms);
            }
        }

        if !found_exe {
            return Err(anyhow::anyhow!("bundle directory missing llama-server executable"));
        }
        log::info!("llama-server bundle copied to {:?}", self.bin_dir);
        Ok(())
    }

    /// Download llama-server binary if not present.
    pub async fn ensure_binary(&self) -> Result<()> {
        if self.is_binary_ready() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.bin_dir)
            .context("创建二进制目录失败")?;

        let tag = "b10034";
        let platform = if cfg!(target_os = "windows") {
            if self.gpu_layers > 0 { "win-cuda-12.4-x64" } else { "win-cpu-x64" }
        } else if cfg!(target_os = "macos") { "mac-arm64" }
        else { "linux-x64" };
        let zip_name = format!("llama-{tag}-bin-{platform}.zip");
        let url = format!("https://github.com/ggml-org/llama.cpp/releases/download/{tag}/{zip_name}");

        log::info!("Downloading llama-server from {}", url);
        let response = reqwest::get(&url)
            .await
            .context("下载 llama-server 失败")?;

        let bytes = response.bytes()
            .await
            .context("读取 llama-server 响应失败")?;

        // Extract the binary from zip (need to save and unzip)
        let zip_path = self.bin_dir.join(&zip_name);
        tokio::fs::write(&zip_path, &bytes)
            .await
            .context("保存 zip 文件失败")?;

        let file = std::fs::File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(file)
            .context("解析 zip 文件失败")?;

        let exe_name = if cfg!(target_os = "windows") { "llama-server.exe" } else { "llama-server" };
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            // Extract exe, DLLs (Windows), or unix binary (macOS/Linux)
            if name.ends_with(".exe") || name.ends_with(".dll") || name.ends_with("llama-server") {
                let dst = self.bin_dir.join(&name);
                let mut out = std::fs::File::create(&dst)?;
                std::io::copy(&mut entry, &mut out)?;
                log::info!("Extracted: {}", name);
            }
        }

        let _ = std::fs::remove_file(&zip_path);
        log::info!("llama-server downloaded & extracted to {:?}", self.bin_dir);
        Ok(())
    }

    // ── Server process management ──

    /// Start llama-server with the given model
    pub async fn start_server(&self, model_name: &str) -> Result<()> {
        // Stop existing server first
        self.stop_server().await;

        let gguf_file = models::get_gguf_filename(model_name)
            .ok_or_else(|| anyhow::anyhow!("未知模型: {}", model_name))?;
        let model_path = self.models_dir.join(gguf_file);
        if !model_path.exists() {
            return Err(anyhow::anyhow!("模型文件未下载: {}", gguf_file));
        }

        let ngl = self.gpu_layers;
        let bin_path = self.bin_path();
        if !bin_path.exists() {
            return Err(anyhow::anyhow!("llama-server 未就绪，请先下载"));
        }

        // Try GPU mode first, fall back to CPU on CUDA init failure
        let mut ngl = ngl;
        loop {
            log::info!("Starting llama-server: {} --n-gpu-layers {} --port {}", bin_path.display(), ngl, LLAMA_SERVER_PORT);
            let mut cmd = tokio::process::Command::new(&bin_path);
            cmd.arg("--model")
                .arg(&model_path)
                .arg("--host")
                .arg("127.0.0.1")
                .arg("--port")
                .arg(LLAMA_SERVER_PORT.to_string())
                .arg("--n-gpu-layers")
                .arg(ngl.to_string())
                .arg("--ctx-size")
                .arg("32768");

            match cmd.spawn() {
                Ok(child) => {
                    // Store process handle
                    {
                        let mut proc = self.server_process.lock().unwrap();
                        *proc = Some(child);
                    }
                    break;
                }
                Err(e) if ngl > 0 => {
                    log::warn!("llama-server GPU 模式启动失败 ({}), 降级到 CPU 模式", e);
                    ngl = 0;
                    continue;
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("启动 llama-server 失败: {}", e));
                }
            }
        }

        // Wait for health
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(|e| anyhow::anyhow!("创建 HTTP 客户端失败: {}", e))?;

        let health_url = format!("http://127.0.0.1:{}/health", LLAMA_SERVER_PORT);
        let start = std::time::Instant::now();

        loop {
            if start.elapsed() > SERVER_START_TIMEOUT {
                self.stop_server().await;
                return Err(anyhow::anyhow!("llama-server 启动超时（180s）"));
            }
            match client.get(&health_url).send().await {
                Ok(resp) if resp.status().is_success() => {
                    log::info!("llama-server ready (model: {})", model_name);
                    *self.current_model.write().await = Some(model_name.to_string());
                    return Ok(());
                }
                _ => tokio::time::sleep(std::time::Duration::from_millis(500)).await,
            }
        }
    }

    /// Stop llama-server
    pub async fn stop_server(&self) {
        let child = {
            let mut proc = self.server_process.lock().unwrap();
            proc.take()
        };
        if let Some(mut child) = child {
            let _ = child.start_kill().ok();
            let _ = child.wait().await;
            log::info!("llama-server stopped");
        }
        *self.current_model.write().await = None;
    }

    /// Check if server is currently running and healthy
    pub async fn is_server_healthy(&self) -> bool {
        {
            let proc = self.server_process.lock().unwrap();
            if proc.is_none() {
                return false;
            }
        } // MutexGuard dropped before await

        // Quick health check
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .ok();
        match client {
            Some(c) => {
                let url = format!("{}/health", self.server_url);
                c.get(&url).send().await.ok().map_or(false, |r| r.status().is_success())
            }
            None => false,
        }
    }

    /// Check if NVIDIA GPU with CUDA Runtime is available.
    /// Checks system paths AND the app's own binaries directory (for bundled CUDA DLLs).
    pub fn check_cuda(bin_dir: &std::path::Path) -> bool {
        // Must have NVIDIA driver
        let has_driver = std::process::Command::new("nvidia-smi")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if !has_driver {
            let alt = [
                "C:\\Windows\\System32\\nvidia-smi.exe",
                "C:\\Program Files\\NVIDIA Corporation\\NVSMI\\nvidia-smi.exe",
            ];
            if !alt.iter().any(|p| std::path::Path::new(p).exists()) {
                return false;
            }
        }

        // Check for CUDA Runtime DLL in PATH, System32, and app's own bin dir
        let search_dirs: Vec<std::path::PathBuf> = {
            let mut dirs = vec![
                std::path::PathBuf::from("C:\\Windows\\System32"),
                bin_dir.to_path_buf(),
            ];
            if let Some(path) = std::env::var_os("PATH") {
                for p in std::env::split_paths(&path) {
                    dirs.push(p);
                }
            }
            dirs
        };

        for dir in &search_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("cudart64_") {
                        return true;
                    }
                }
            }
        }
        false
    }

    // ── Model download ──
    // Same polling pattern as whisper/engine.rs

    pub fn get_download_state(&self) -> serde_json::Value {
        serde_json::json!({
            "status": self.download_status.lock().unwrap().clone().unwrap_or_else(|| "idle".to_string()),
            "progress": self.download_progress.load(Ordering::SeqCst),
            "downloadedBytes": self.downloaded_bytes.load(Ordering::SeqCst),
            "totalBytes": self.total_bytes.load(Ordering::SeqCst),
            "speed": self.download_speed.load(Ordering::SeqCst),
        })
    }

    pub fn cancel_download(&self) {
        self.cancel_download.store(true, Ordering::SeqCst);
    }

    pub async fn download_model(&self, name: &str) -> Result<()> {
        if self.cancel_download.load(Ordering::SeqCst) {
            self.cancel_download.store(false, Ordering::SeqCst);
        }
        self.set_status("downloading");
        self.download_progress.store(0, Ordering::SeqCst);
        self.downloaded_bytes.store(0, Ordering::SeqCst);
        self.total_bytes.store(0, Ordering::SeqCst);
        self.download_speed.store(0, Ordering::SeqCst);

        let gguf_file = models::get_gguf_filename(name)
            .ok_or_else(|| anyhow::anyhow!("未知模型: {}", name))?;
        let dest = self.models_dir.join(gguf_file);
        let partial = dest.with_extension("partial");

        // Try URLs in order
        let urls = models::get_all_download_urls(name);
        let client = Client::new();

        for url in urls {
            log::info!("Downloading GGUF model: {}", url);
            match client.get(url).send().await {
                Ok(response) => {
                    let total = response.content_length().unwrap_or(0);
                    self.total_bytes.store(total, Ordering::SeqCst);

                    let mut stream = response.bytes_stream();
                    let mut file = tokio::fs::File::create(&partial).await?;
                    use tokio::io::AsyncWriteExt;
                    let mut downloaded: u64 = 0;
                    let mut last_time = std::time::Instant::now();
                    let mut bytes_since_last: u64 = 0;

                    while let Some(chunk) = stream.next().await {
                        if self.cancel_download.load(Ordering::SeqCst) {
                            drop(file);
                            let _ = std::fs::remove_file(&partial);
                            self.set_status("cancelled");
                            return Err(anyhow::anyhow!("下载已取消"));
                        }

                        let chunk = chunk?;
                        file.write_all(&chunk).await?;
                        downloaded += chunk.len() as u64;
                        bytes_since_last += chunk.len() as u64;

                        if total > 0 {
                            let pct = (downloaded * 100 / total) as u8;
                            self.download_progress.store(pct, Ordering::SeqCst);
                        }
                        self.downloaded_bytes.store(downloaded, Ordering::SeqCst);

                        let elapsed = last_time.elapsed().as_secs_f32();
                        if elapsed >= 1.0 {
                            let speed = (bytes_since_last as f32 / elapsed) as u64;
                            self.download_speed.store(speed, Ordering::SeqCst);
                            bytes_since_last = 0;
                            last_time = std::time::Instant::now();
                        }
                    }
                    file.flush().await?;
                    drop(file);

                    // Rename partial -> final
                    std::fs::rename(&partial, &dest)
                        .context("重命名模型文件失败")?;

                    log::info!("GGUF model downloaded: {:?}", dest);
                    self.set_status("complete");
                    self.download_progress.store(100, Ordering::SeqCst);
                    return Ok(());
                }
                Err(e) => {
                    log::warn!("Download from {} failed: {}", url, e);
                    continue;
                }
            }
        }

        self.set_status("error:所有下载源均失败");
        Err(anyhow::anyhow!("所有下载源均失败"))
    }

    pub fn delete_model(&self, name: &str) -> Result<()> {
        if let Some(gguf_file) = models::get_gguf_filename(name) {
            let path = self.models_dir.join(gguf_file);
            if path.exists() {
                std::fs::remove_file(&path)?;
                log::info!("Deleted model: {:?}", path);
            }
        }
        Ok(())
    }

    // ── Inference ──

    /// Generate text by calling llama-server's OpenAI-compatible API
    pub async fn generate(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String> {
        if !self.is_server_healthy().await {
            return Err(anyhow::anyhow!("llama-server 未运行，请先加载模型"));
        }

        let client = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("创建 HTTP 客户端失败")?;

        let body = serde_json::json!({
            "model": "local",
            "messages": [
                {"role": "system", "content": system_prompt},
                {"role": "user", "content": user_prompt}
            ],
            "stream": false,
            "temperature": 0.5,
            "top_p": 0.8,
            "repeat_penalty": 1.05,
            "repeat_last_n": 256,
            "n_predict": -1,
            "stop": ["<|im_end|>", "<|end_of_turn|>"]
        });

        let resp = client
            .post(format!("{}/v1/chat/completions", self.server_url))
            .json(&body)
            .send()
            .await
            .context("连接 llama-server 失败")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!("llama-server 返回错误 {}: {}", status, text));
        }

        let json: serde_json::Value = resp
            .json()
            .await
            .context("解析 llama-server 响应失败")?;

        let choice = &json["choices"][0];
        let msg = &choice["message"];

        // Try content field first, then reasoning_content (for Qwen/R1 models)
        let content = msg["content"].as_str()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| msg["reasoning_content"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        // Strip thinking/reasoning content (e.g., <think>...</think> from Qwen3.5)
        let content = Self::strip_thinking_tags(&content);

        if content.is_empty() {
            // Log the full response for debugging
            let response_text = serde_json::to_string(&json).unwrap_or_default();
            log::error!("llama-server 返回空内容，完整响应(前500字): {}", &response_text[..response_text.len().min(500)]);
            return Err(anyhow::anyhow!("llama-server 返回了空内容"));
        }

        Ok(content)
    }

    // ── Helpers ──

    /// Strip <think>...</think> blocks from model output (Qwen3.5 / reasoning models).
    /// If no </think> found, returns the content as-is (thinking may be in progress).
    fn strip_thinking_tags(content: &str) -> String {
        // Try to find a complete think block with closing tag
        if let Some(end) = content.find("</think>") {
            // Return everything after </think>
            let after = content[end + 8..].trim();
            if !after.is_empty() {
                return after.to_string();
            }
        }
        // Also handle ​<​/​t​h​i​n​k​> (with possible zero-width chars)
        // Fallback: try removing opening tag only
        let cleaned = content.replace("<think>", "").replace("</think>", "");
        cleaned.trim().to_string()
    }

    fn set_status(&self, status: &str) {
        *self.download_status.lock().unwrap() = Some(status.to_string());
    }

    /// List models with download status
    pub fn list_models(&self) -> Vec<models::LlmModelInfo> {
        let current = self.current_model.try_read().map(|g| g.clone()).ok().flatten();
        models::list_models(&self.models_dir, current.as_deref())
    }
}

impl Drop for LlmEngine {
    fn drop(&mut self) {
        // Try to stop server on drop
        if let Ok(mut proc) = self.server_process.lock() {
            if let Some(mut child) = proc.take() {
                let _ = child.start_kill();
            }
        }
    }
}
