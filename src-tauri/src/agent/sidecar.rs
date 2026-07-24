// EasyWork - Agent sidecar: HTTP proxy to Python agent server + process lifecycle.
//
// The Python server handles all LLM logic (Ollama calls, context building,
// memory management, ReAct loop, skill system, Excel execution).
// Rust only forwards requests through this thin HTTP proxy.

use reqwest::Client;
use serde::de::DeserializeOwned;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

const DEFAULT_PORT: u16 = 9876;
const HEALTH_CHECK_INTERVAL: Duration = Duration::from_millis(500);
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(30);

/// Manages the Python agent sidecar process and HTTP connection.
pub struct AgentSidecar {
    client: Client,
    base_url: RwLock<String>,
}

impl AgentSidecar {
    pub fn new(port: u16) -> Self {
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(180))
                .build()
                .expect("Failed to create HTTP client"),
            base_url: RwLock::new(format!("http://127.0.0.1:{}", port)),
        }
    }

    pub async fn set_port(&self, port: u16) {
        *self.base_url.write().await = format!("http://127.0.0.1:{}", port);
    }

    /// Call an endpoint on the Python agent server.
    pub async fn call<T: DeserializeOwned>(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> Result<T, String> {
        let url = format!("{}{}", self.base_url.read().await, path);
        let mut req = self.client.request(method, &url);
        if let Some(b) = body {
            req = req.json(b);
        }
        let resp = req.send().await.map_err(|e| format!("Agent 服务不可达: {}", e))?;
        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| format!("读取响应失败: {}", e))?;
        if !status.is_success() {
            return Err(format!("Agent 服务错误 ({}): {}", status.as_u16(), text));
        }
        serde_json::from_str::<T>(&text).map_err(|e| format!("解析响应失败: {} — body: {}", e, &text[..text.len().min(200)]))
    }

    /// Convenience: POST with JSON body.
    pub async fn post<T: DeserializeOwned>(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<T, String> {
        self.call(reqwest::Method::POST, path, Some(body)).await
    }

    /// Convenience: GET without body.
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        self.call(reqwest::Method::GET, path, None).await
    }
}

/// Find an available TCP port starting from `start_port`.
/// Returns `None` if no port is available within `max_attempts`.
pub fn find_available_port(start_port: u16, max_attempts: u16) -> Option<u16> {
    for port in start_port..start_port.saturating_add(max_attempts) {
        if std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).is_ok() {
            return Some(port);
        }
    }
    None
}

/// Spawn the Python agent server.
/// Uses local llama-server by default; can use DeepSeek online if configured in settings.
pub async fn spawn_python_server(
    project_dir: &std::path::Path,
    manifest_dir: &std::path::Path,
    db_path: &std::path::Path,
    port: u16,
    settings: &std::collections::HashMap<String, String>,
) -> Result<tokio::process::Child, String> {
    let python_cmd = find_python();
    let server_dir = manifest_dir.join("py_backend");

    if !server_dir.join("main.py").exists() {
        return Err(format!(
            "Python backend server not found at {}",
            server_dir.display()
        ));
    }

    let llm_backend = settings.get("agent_llm_backend")
        .filter(|s| !s.is_empty())
        .map(|s| s.as_str())
        .unwrap_or("local");

    let mut cmd = tokio::process::Command::new(&python_cmd);
    cmd.arg("-u")
        .arg("-m")
        .arg("py_backend.main")
        .current_dir(manifest_dir)
        .env("AGENT_PORT", port.to_string())
        .env("AGENT_DB_PATH", db_path.to_string_lossy().to_string())
        .env("AGENT_PROJECT_DIR", project_dir.to_string_lossy().to_string())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match llm_backend {
        "online" => {
            cmd.env("LLM_BACKEND", "deepseek")  // Python agent uses "deepseek" for OpenAI-compatible
                .env("DEEPSEEK_BASE_URL", settings.get("agent_online_url")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_str())
                    .unwrap_or("https://api.openai.com"))
                .env("DEEPSEEK_MODEL", settings.get("agent_online_model")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_str())
                    .unwrap_or("gpt-4o"))
                .env("DEEPSEEK_API_KEY", settings.get("agent_online_key")
                    .filter(|s| !s.is_empty())
                    .map(|s| s.as_str())
                    .unwrap_or(""));
        }
        _ => {
            // Default: local llama-server (OpenAI-compatible API)
            cmd.env("LLM_BACKEND", "deepseek")
                .env("DEEPSEEK_BASE_URL", "http://127.0.0.1:11435")
                .env("DEEPSEEK_MODEL", "local")
                .env("DEEPSEEK_API_KEY", "");
        }
    }

    let child = cmd.spawn()
        .map_err(|e| format!("无法启动 Python agent 服务器 ({}): {}", python_cmd, e))?;

    Ok(child)
}

/// Poll /health until the Python server responds or timeout expires.
pub async fn wait_for_healthy(sidecar: &AgentSidecar, timeout: Duration) -> Result<(), String> {
    let start = std::time::Instant::now();
    loop {
        if start.elapsed() > timeout {
            return Err("Agent 服务启动超时".to_string());
        }
        match sidecar
            .get::<serde_json::Value>("/health")
            .await
        {
            Ok(_) => return Ok(()),
            Err(_) => tokio::time::sleep(HEALTH_CHECK_INTERVAL).await,
        }
    }
}

/// Find a working Python command on the system.
/// On Windows, also tries common installation paths to handle GUI-mode PATH issues.
fn find_python() -> String {
    // First try common command names
    for cmd in &["python", "python3", "py"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
        {
            return cmd.to_string();
        }
    }

    // On Windows, try common installation paths
    if cfg!(target_os = "windows") {
        let username = std::env::var("USERNAME").unwrap_or_default();
        let userprofile = std::env::var("USERPROFILE").unwrap_or_default();
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            format!(r"C:\Users\{}\AppData\Local\Programs\Python\Python312\python.exe", username),
            format!(r"C:\Users\{}\AppData\Local\Programs\Python\Python313\python.exe", username),
            format!(r"C:\Users\{}\AppData\Local\Programs\Python\Python311\python.exe", username),
            format!(r"{}\AppData\Local\Programs\Python\Python312\python.exe", userprofile),
            format!(r"{}\AppData\Local\Programs\Python\Python313\python.exe", userprofile),
            format!(r"{}\AppData\Local\Programs\Python\Python311\python.exe", userprofile),
            r"C:\Python312\python.exe".to_string(),
            r"C:\Python313\python.exe".to_string(),
            r"C:\Python311\python.exe".to_string(),
        ];
        for path in &candidates {
            if std::path::Path::new(path).exists() {
                return path.to_string();
            }
        }
    }

    "python".to_string()
}

/// Guard that kills the Python child process on drop.
pub struct ProcessGuard {
    child: Arc<tokio::sync::Mutex<Option<tokio::process::Child>>>,
}

impl ProcessGuard {
    pub fn new(child: tokio::process::Child) -> Self {
        Self {
            child: Arc::new(tokio::sync::Mutex::new(Some(child))),
        }
    }

    pub fn arc_clone(&self) -> Arc<tokio::sync::Mutex<Option<tokio::process::Child>>> {
        self.child.clone()
    }
}

impl Drop for ProcessGuard {
    fn drop(&mut self) {
        // We can't do async in Drop, so spawn a blocking kill.
        // In practice Tauri's process exit will clean up child processes.
        if let Ok(mut guard) = self.child.try_lock() {
            if let Some(mut child) = guard.take() {
                let _ = child.start_kill();
            }
        }
    }
}
