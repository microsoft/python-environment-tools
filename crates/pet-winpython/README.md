# WinPython Locator

This crate provides support for detecting [WinPython](https://winpython.github.io/) environments.

## Detection Strategy

WinPython environments are identified by looking for:

1. **Marker files**: `.winpython` or `winpython.ini` file in parent directories
2. **Directory naming pattern**: Parent directory matching patterns like `WPy64-*`, `WPy32-*`, or `WPy-*`
3. **Python folder naming**: The Python installation folder typically follows the pattern `python-X.Y.Z.amd64` or `python-X.Y.Z`

## Typical WinPython Directory Structure

```
WPy64-31300/                          # Top-level WinPython directory
├── .winpython                        # Marker file (may also be winpython.ini)
├── python-3.13.0.amd64/              # Python installation
│   ├── python.exe
│   ├── pythonw.exe
│   ├── Scripts/
│   └── Lib/
├── scripts/                          # WinPython-specific scripts
│   ├── env.bat
│   └── WinPython Command Prompt.exe
├── settings/                         # Settings directory
└── notebooks/                        # Optional Jupyter notebooks
```

## Platform Support

This locator only works on Windows, as WinPython is a Windows-only distribution.
