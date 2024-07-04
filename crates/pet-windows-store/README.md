# Windows Store

## Known Issues

- Note possible to get the `version` information, hence not returned

```rust
for directory under `<home>/AppData/Local/Microsoft/WindowsApps`:
    if directory does not start with `PythonSoftwareFoundation.Python.`:
        continue

    if `python.exe` does not exists in the directory:
        continue

    app_model_key = `HKCU/Software/Classes/Local Settings/Software/Microsoft/Windows/CurrentVersion/AppModel`;
    package_name = `<app_model_key>/SystemAppData/<directory name>/Schemas/(PackageFullName)`
    key = `<app_model_key>/Repository/Packages/<package_name>`
    env_path = `<key>/(PackageRootFolder)`
    display_name = `<key>/(DisplayName)`
    exe = `python.exe`
    // No way to get the full version information.
    👍 track this environment
```
