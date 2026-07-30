# -*- mode: python ; coding: utf-8 -*-


a = Analysis(
    ['py_backend\\main.py'],
    pathex=['py_backend'],
    binaries=[],
    datas=[('py_backend/__init__.py', 'py_backend'), ('py_backend/config.py', 'py_backend'), ('py_backend/export.py', 'py_backend'), ('py_backend/main.py', 'py_backend'), ('py_backend/routes.py', 'py_backend'), ('py_backend/data', 'py_backend/data'), ('py_backend/llm', 'py_backend/llm'), ('py_backend/tools', 'py_backend/tools')],
    hiddenimports=['uvicorn', 'httpx', 'aiosqlite', 'tiktoken', 'openpyxl', 'pandas', 'pydantic', 'yaml', 'xlrd'],
    hookspath=[],
    hooksconfig={},
    runtime_hooks=[],
    excludes=[],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    a.binaries,
    a.datas,
    [],
    name='easywork-agent',
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=True,
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
)
