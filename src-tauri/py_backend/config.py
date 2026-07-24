"""EasyWork Agent Server configuration. All paths and settings are resolved at startup."""

import os
from pathlib import Path

_package_dir = Path(__file__).resolve().parent

# Load .env file if it exists (simple parser, no dependency needed)
_env_file = _package_dir / ".env"
if _env_file.exists():
    for _line in _env_file.read_text(encoding="utf-8").splitlines():
        _line = _line.strip()
        if _line and not _line.startswith("#") and "=" in _line:
            _key, _, _val = _line.partition("=")
            _key = _key.strip()
            _val = _val.strip().strip('"').strip("'")
            if _key not in os.environ:
                os.environ[_key] = _val

# Server
AGENT_PORT = int(os.environ.get("AGENT_PORT", "9876"))

# LLM backend: "deepseek" or "llamacpp"
LLM_BACKEND = os.environ.get("LLM_BACKEND", "deepseek")

# DeepSeek (OpenAI-compatible API)
DEEPSEEK_API_KEY = os.environ.get("DEEPSEEK_API_KEY", "")
DEEPSEEK_BASE_URL = os.environ.get("DEEPSEEK_BASE_URL", "https://api.deepseek.com")
DEEPSEEK_MODEL = os.environ.get("DEEPSEEK_MODEL", "deepseek-chat")
DEEPSEEK_TIMEOUT = int(os.environ.get("DEEPSEEK_TIMEOUT", "180"))

# llama.cpp (OpenAI-compatible, built-in)
LLAMACPP_URL = os.environ.get("LLAMACPP_URL", "http://127.0.0.1:11435")
LLAMACPP_MODEL = os.environ.get("LLAMACPP_MODEL", "local")
LLAMACPP_TIMEOUT = int(os.environ.get("LLAMACPP_TIMEOUT", "300"))

# Shared
EXTRACTION_TIMEOUT = int(os.environ.get("EXTRACTION_TIMEOUT", "30"))

# Project root: parent of py_backend/ or from AGENT_PROJECT_DIR env var
_project_dir = os.environ.get("AGENT_PROJECT_DIR")
if _project_dir:
    PROJECT_ROOT = Path(_project_dir)
else:
    PROJECT_ROOT = _package_dir.parent

# SQLite database (shared with Rust)
DB_PATH = os.environ.get("AGENT_DB_PATH")
if not DB_PATH:
    _candidates = [
        PROJECT_ROOT / "easework.db",
        PROJECT_ROOT / "app_data" / "easework.db",
    ]
    for _c in _candidates:
        if _c.exists():
            DB_PATH = str(_c)
            break
    if not DB_PATH:
        DB_PATH = str(PROJECT_ROOT / "easework.db")

# Skills directory
SKILLS_DIR = os.environ.get(
    "AGENT_SKILLS_DIR",
    str(PROJECT_ROOT / "src" / "agent" / "skills"),
)

# Memories directory
MEMORIES_DIR = os.environ.get(
    "AGENT_MEMORIES_DIR",
    str(PROJECT_ROOT / "src" / "agent" / "memories"),
)
MEMORY_FILE = "MEMORY.md"

# Agent input/output directories
AGENT_INPUT_DIR = os.environ.get(
    "AGENT_INPUT_DIR",
    str(PROJECT_ROOT / "agent_input"),
)
AGENT_OUTPUT_DIR = os.environ.get(
    "AGENT_OUTPUT_DIR",
    str(PROJECT_ROOT / "agent_output"),
)

# Docker sandbox
DOCKER_MODE = os.environ.get("DOCKER_MODE", "auto").lower()
DOCKER_IMAGE = os.environ.get("DOCKER_IMAGE", "easework-sandbox:latest")
DOCKER_MEMORY_LIMIT = os.environ.get("DOCKER_MEMORY_LIMIT", "512m")
DOCKER_CPU_LIMIT = float(os.environ.get("DOCKER_CPU_LIMIT", "1.0"))
DOCKER_BUILD_TIMEOUT = int(os.environ.get("DOCKER_BUILD_TIMEOUT", "120"))

# Tool execution
TOOL_TIMEOUT = int(os.environ.get("TOOL_TIMEOUT", "120"))

# Email / Graph API
GRAPH_CLIENT_ID = os.environ.get("GRAPH_CLIENT_ID", "")
GRAPH_TENANT_ID = os.environ.get("GRAPH_TENANT_ID", "")
AGENT_TOKEN_DIR = os.environ.get(
    "AGENT_TOKEN_DIR",
    str(PROJECT_ROOT / "agent_tokens"),
)

# Agent behaviour
MAX_REACT_ITERATIONS = int(os.environ.get("AGENT_MAX_REACT", "10"))
KEEP_RECENT_COUNT = int(os.environ.get("AGENT_KEEP_RECENT", "8"))
COMPRESS_TRIGGER_TOKENS = int(os.environ.get("AGENT_COMPRESS_TOKENS", "20000"))
