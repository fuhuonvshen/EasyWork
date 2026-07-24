"""Unified LLM client — supports DeepSeek (cloud) and llama.cpp (local)."""

from __future__ import annotations

import asyncio
import json
import logging
import time

import httpx

from .. import config as _cfg

logger = logging.getLogger("agent.llm")


class LLMError(Exception):
    """Base for all LLM errors."""
    def __init__(self, message: str):
        self.message = message
        super().__init__(message)


class LLMTimeoutError(LLMError): ...
class LLMAuthError(LLMError): ...
class LLMOverloadError(LLMError): ...
class LLMAPiError(LLMError): ...
class LLMUnexpectedError(LLMError): ...


async def llm_chat(
    messages: list[dict],
    tools: list[dict] | None = None,
    *,
    timeout: int | None = None,
    **kwargs,
) -> dict | None:
    """Send chat request to the configured LLM backend.

    kwargs: overrides for model parameters (temperature, max_tokens, top_p, etc.)
    """
    if _cfg.LLM_BACKEND == "deepseek":
        return await _deepseek_chat(messages, tools, timeout=timeout, **kwargs)
    elif _cfg.LLM_BACKEND == "llamacpp":
        return await _llamacpp_chat(messages, tools, timeout=timeout, **kwargs)
    else:
        logger.error("Unknown LLM backend: %s", _cfg.LLM_BACKEND)
        return None


async def llm_chat_text(
    messages: list[dict],
    tools: list[dict] | None = None,
    *,
    timeout: int | None = None,
    **kwargs,
) -> str:
    """Like llm_chat but returns just the text content, or empty string on error."""
    try:
        msg = await llm_chat(messages, tools, timeout=timeout, **kwargs)
        if msg is None:
            return ""
        return msg.get("content", "") or ""
    except LLMError:
        return ""

# ── DeepSeek backend ──────────────────────────────────────────

def _build_deepseek_headers() -> dict:
    return {
        "Authorization": f"Bearer {_cfg.DEEPSEEK_API_KEY}",
        "Content-Type": "application/json",
    }


async def _deepseek_chat(
    messages: list[dict],
    tools: list[dict] | None = None,
    *,
    timeout: int | None = None,
    **kwargs,
) -> dict | None:
    url = f"{_cfg.DEEPSEEK_BASE_URL}/v1/chat/completions"

    clean_messages = _clean_messages_for_openai(messages)

    body: dict = {
        "model": _cfg.DEEPSEEK_MODEL,
        "messages": clean_messages,
        "max_tokens": 4096,
        "stream": False,
    }
    if tools:
        body["tools"] = _convert_tools_for_openai(tools)
        body["tool_choice"] = "auto"
    body.update(kwargs)

    req_timeout = timeout or _cfg.DEEPSEEK_TIMEOUT
    max_retries = 2
    for attempt in range(max_retries):
        _log_request("deepseek", clean_messages, _cfg.DEEPSEEK_MODEL)
        t0 = time.time()
        try:
            async with httpx.AsyncClient(timeout=httpx.Timeout(req_timeout)) as client:
                resp = await client.post(
                    url,
                    json=body,
                    headers=_build_deepseek_headers(),
                )
            resp.raise_for_status()
            data = resp.json()
            choice = data.get("choices", [{}])[0]
            msg = choice.get("message", {})
            elapsed = time.time() - t0
            _log_response("deepseek", msg, data.get("usage", {}).get("completion_tokens"))
            logger.info("[deepseek] done in %.1fs, tokens=%s", elapsed, data.get("usage", {}).get("completion_tokens"))
            return msg
        except httpx.TimeoutException:
            elapsed = time.time() - t0
            logger.error("DeepSeek request timed out (%.1fs, attempt %d/%d)", elapsed, attempt + 1, max_retries)
            if attempt < max_retries - 1:
                await asyncio.sleep(1)
                continue
            raise LLMTimeoutError("请求超时，请检查网络连接")
        except json.JSONDecodeError:
            logger.error("DeepSeek returned non-JSON response: %s", resp.text[:500])
            raise LLMAPiError("API 返回了非 JSON 格式的响应")
        except httpx.HTTPStatusError as e:
            status = e.response.status_code
            detail = e.response.text[:300]
            logger.error("DeepSeek HTTP %s: %s (attempt %d/%d)", status, detail, attempt + 1, max_retries)
            if status in (429, 503) and attempt < max_retries - 1:
                await asyncio.sleep(1.5)
                continue
            if status == 401:
                raise LLMAuthError("DeepSeek API Key 无效，请在设置中检查")
            err_cls = LLMOverloadError if status in (429, 503) else LLMAPiError
            raise err_cls(f"DeepSeek 返回错误 {status}")
        except Exception as e:
            logger.error("DeepSeek request failed: %s (attempt %d/%d)", e, attempt + 1, max_retries)
            if attempt < max_retries - 1:
                await asyncio.sleep(1)
                continue
            raise LLMUnexpectedError(f"请求异常: {e}")


def _clean_messages_for_openai(messages: list[dict]) -> list[dict]:
    """Convert internal messages to OpenAI/DeepSeek format."""
    cleaned = []

    for m in messages:
        role = m.get("role", "user")

        if role == "assistant" and m.get("tool_calls"):
            cleaned.append(m)
        elif role == "tool":
            cleaned.append({
                "role": "tool",
                "tool_call_id": m.get("tool_call_id", "call_auto"),
                "content": m.get("content", ""),
            })
        elif role in ("system", "user", "assistant"):
            cleaned.append({"role": role, "content": m.get("content", "")})
    return cleaned


def _convert_tools_for_openai(tools: list[dict]) -> list[dict]:
    """Normalize tool definitions to OpenAI-compatible format."""
    result = []
    for t in tools:
        func = t.get("function", {})
        params = func.get("parameters") or {"type": "object", "properties": {}}
        result.append({
            "type": "function",
            "function": {
                "name": func.get("name", ""),
                "description": func.get("description", ""),
                "parameters": params,
            },
        })
    return result


# ── llama.cpp backend (OpenAI-compatible, built-in) ─────────────

async def _llamacpp_chat(
    messages: list[dict],
    tools: list[dict] | None = None,
    *,
    timeout: int | None = None,
    **kwargs,
) -> dict | None:
    url = f"{_cfg.LLAMACPP_URL}/v1/chat/completions"

    clean_messages = _clean_messages_for_openai(messages)

    body: dict = {
        "model": _cfg.LLAMACPP_MODEL,
        "messages": clean_messages,
        "max_tokens": 8192,
        "stream": False,
        "temperature": 0.5,
        "top_p": 0.8,
    }
    if tools:
        body["tools"] = _convert_tools_for_openai(tools)
        body["tool_choice"] = "auto"
    body.update(kwargs)

    req_timeout = timeout or _cfg.LLAMACPP_TIMEOUT
    max_retries = 2
    for attempt in range(max_retries):
        _log_request("llamacpp", clean_messages, _cfg.LLAMACPP_MODEL)
        t0 = time.time()
        try:
            async with httpx.AsyncClient(timeout=httpx.Timeout(req_timeout)) as client:
                resp = await client.post(url, json=body)
            resp.raise_for_status()
            data = resp.json()
            choice = data.get("choices", [{}])[0]
            msg = choice.get("message", {})
            elapsed = time.time() - t0
            _log_response("llamacpp", msg, data.get("usage", {}).get("completion_tokens"))
            logger.info("[llamacpp] done in %.1fs, tokens=%s", elapsed, data.get("usage", {}).get("completion_tokens"))
            return msg
        except httpx.TimeoutException:
            elapsed = time.time() - t0
            logger.error("llama.cpp request timed out (%.1fs, attempt %d/%d)", elapsed, attempt + 1, max_retries)
            if attempt < max_retries - 1:
                await asyncio.sleep(1)
                continue
            raise LLMTimeoutError("llama.cpp 请求超时，请检查本地服务是否运行")
        except json.JSONDecodeError:
            logger.error("llama.cpp returned non-JSON response: %s", resp.text[:500])
            raise LLMAPiError("API 返回了非 JSON 格式的响应")
        except httpx.HTTPStatusError as e:
            status = e.response.status_code
            detail = e.response.text[:300]
            logger.error("llama.cpp HTTP %s: %s (attempt %d/%d)", status, detail, attempt + 1, max_retries)
            raise LLMAPiError(f"llama.cpp 返回错误 {status}")
        except Exception as e:
            logger.error("llama.cpp request failed: %s (attempt %d/%d)", e, attempt + 1, max_retries)
            if attempt < max_retries - 1:
                await asyncio.sleep(1)
                continue
            raise LLMUnexpectedError(f"llama.cpp 请求异常: {e}")


# ── Logging helpers ───────────────────────────────────────────

def _log_request(backend: str, messages: list[dict], model: str):
    total_chars = sum(len(m.get("content", "") or "") for m in messages)
    logger.debug("[%s] request: %d msgs, ~%d chars, model=%s", backend, len(messages), total_chars, model)


def _log_response(backend: str, msg: dict, token_info):
    content = msg.get("content", "") or ""
    tc_count = len(msg.get("tool_calls", []) or [])
    logger.debug("[%s] response: %d chars, %d tool_calls, tokens=%s", backend, len(content), tc_count, token_info)
