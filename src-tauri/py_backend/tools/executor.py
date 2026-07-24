"""Code execution sandbox — runs LLM-generated Python code via subprocess or Docker."""

from __future__ import annotations

import asyncio
import logging
import os
import subprocess
import sys
import textwrap

from ..config import AGENT_INPUT_DIR, AGENT_OUTPUT_DIR

EXEC_TIMEOUT = int(os.environ.get("AGENT_EXEC_TIMEOUT", "60"))

logger = logging.getLogger("agent.executor")


# ── Code execution ─────────────────────────────────────────


class CodeExecutor:
    """Executes Python code with safety restrictions and optional Docker sandboxing."""

    def __init__(self, allowed_input: str = AGENT_INPUT_DIR,
                 allowed_output: str = AGENT_OUTPUT_DIR):
        self.allowed_input = allowed_input
        self.allowed_output = allowed_output
        self._docker_sandbox = None

    async def _get_docker_sandbox(self):
        if self._docker_sandbox is None:
            from .sandbox import get_sandbox

            self._docker_sandbox = get_sandbox()
        return self._docker_sandbox

    async def execute(self, code: str, timeout: int = EXEC_TIMEOUT) -> str:
        """Execute Python code, preferring Docker sandbox, falling back to subprocess.

        Security model:
        - Docker sandbox = the REAL security boundary (hardware isolation).
        - safe_mode_filter = defense-in-depth for subprocess fallback
          (catches obvious abuse but is NOT a security boundary —
           motivated code can trivially bypass regex filters).
        - Preamble imports are for convenience, not security.
        """
        # Safety filter applies to both paths
        from .sandbox import safe_mode_filter
        safe, reason = safe_mode_filter(code)
        if not safe:
            logger.warning("Code blocked by safe_mode: %s", reason)
            return f"[错误] 代码被安全策略拦截: {reason}，请修改代码后重试"

        sandbox = await self._get_docker_sandbox()
        if await sandbox.check_available():
            logger.info("Using Docker sandbox for execution")
            return await sandbox.execute(code, timeout)
        else:
            logger.info("Docker not available, falling back to subprocess")
            return await self._execute_subprocess(code, timeout)

    async def _execute_subprocess(self, code: str, timeout: int) -> str:
        """Execute Python code in a subprocess with safety restrictions.

        Note: subprocess mode has NO memory/CPU resource isolation.
        For resource-constrained execution, use Docker sandbox (DOCKER_MODE=auto).
        """
        wrapped = _wrap_code(code)
        try:
            proc = await asyncio.create_subprocess_exec(
                sys.executable,
                "-c",
                wrapped,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=timeout
            )
            stdout_str = stdout.decode("utf-8", errors="replace").strip()
            stderr_str = stderr.decode("utf-8", errors="replace").strip()

            result = ""
            if stdout_str:
                result += f"[标准输出]\n{stdout_str}\n"
            if stderr_str:
                result += f"[标准错误]\n{stderr_str}\n"
            if proc.returncode != 0 and not result:
                result = f"进程退出码: {proc.returncode}"

            return result.strip() or "(无输出)"
        except asyncio.TimeoutError:
            return f"[错误] 代码执行超时 ({timeout}秒)"
        except Exception as e:
            logger.error("Code execution failed: %s", e, exc_info=True)
            return "[错误] 代码执行失败，请检查代码语法和逻辑后重试"


executor = CodeExecutor(AGENT_INPUT_DIR, AGENT_OUTPUT_DIR)


def _wrap_code(code: str) -> str:
    """Wrap user code in a safe execution environment with restricted globals."""
    preamble = textwrap.dedent("""
    import sys, os, json, math, re, random, statistics, collections, itertools
    import datetime, pathlib, textwrap, fractions, decimal, hashlib, base64
    import typing, copy, pprint, io, csv, string, uuid

    # Third-party
    import pandas as pd
    import numpy as np
    import openpyxl
    from openpyxl import Workbook, load_workbook
    from pathlib import Path

    AGENT_INPUT = r"{input_dir}"
    AGENT_OUTPUT = r"{output_dir}"
    os.makedirs(AGENT_OUTPUT, exist_ok=True)
    os.chdir(AGENT_OUTPUT)
    """).format(input_dir=AGENT_INPUT_DIR, output_dir=AGENT_OUTPUT_DIR)

    full_code = preamble + "\n" + code
    return full_code
