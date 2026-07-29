"""PyInstaller entry point for the EasyWork agent.
Sits outside the py_backend package so that relative imports in
py_backend.main work correctly when bundled by PyInstaller.
"""
import sys
import os

# Ensure py_backend package is importable
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

if __name__ == "__main__":
    import uvicorn
    from py_backend.config import AGENT_PORT
    from py_backend.main import app
    uvicorn.run(app, host="127.0.0.1", port=AGENT_PORT, log_level="info")
