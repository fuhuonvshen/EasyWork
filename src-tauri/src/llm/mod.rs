// EasyWork - 本地 LLM 推理模块（llama.cpp HTTP server + GGUF 模型管理）

pub mod commands;
pub mod engine;
pub mod models;

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::llm::engine::LlmEngine;

/// 初始化 LLM 引擎：创建引擎，复制二进制，自动加载模型。
pub async fn init(models_dir: &Path, bin_dir: &Path, resource_dir: Option<&Path>, dev_bin_dir: Option<&Path>) -> Result<Arc<RwLock<LlmEngine>>> {
    std::fs::create_dir_all(models_dir)
        .context("创建 LLM 模型目录失败")?;
    std::fs::create_dir_all(bin_dir)
        .context("创建 LLM 二进制目录失败")?;

    // 1. Create engine (gpu_layers starts at 0, will update after binary is ready)
    let engine = Arc::new(RwLock::new(LlmEngine::new(
        models_dir.to_path_buf(),
        bin_dir.to_path_buf(),
    )));

    // 2. Ensure llama-server binary exists
    let mut binary_copied = false;
    if !engine.read().await.is_binary_ready() {
        // Try development binaries directory (src-tauri/binaries/)
        if let Some(dev_dir) = dev_bin_dir {
            if engine.read().await.copy_from_bundle(dev_dir).is_ok() {
                binary_copied = true;
            }
        }

        // Try production bundle (resource dir)
        if !binary_copied {
            if let Some(res_dir) = resource_dir {
                let bundle_path = res_dir.join("binaries");
                if engine.read().await.copy_from_bundle(&bundle_path).is_ok() {
                    binary_copied = true;
                }
            }
        }

        // Download from GitHub as last resort
        if !binary_copied {
            log::info!("llama-server 二进制不存在，正在从 GitHub 下载...");
            let eng = engine.clone();
            tokio::spawn(async move {
                if let Err(e) = eng.read().await.ensure_binary().await {
                    log::error!("llama-server 下载失败: {}", e);
                }
            });
        }
    } else {
        binary_copied = true;
    }

    // 3. Auto-detect GPU: macOS always has Metal, other platforms check CUDA
    #[cfg(target_os = "macos")]
    let has_gpu = true;
    #[cfg(not(target_os = "macos"))]
    let has_gpu = LlmEngine::check_cuda(bin_dir);
    engine.write().await.gpu_layers = if has_gpu { 99 } else { 0 };
    log::info!("GPU detection: {}, gpu_layers={}", has_gpu, if has_gpu { 99 } else { 0 });

    Ok(engine)
}
