# Python environment tools for Visual Studio Code

Performant Python environment tooling and support, such as locating all global Python installs and virtual environments. 

This project will be consumed by the [Python extension](https://marketplace.visualstudio.com/items?itemName=ms-python.python) directly. You can find the code to consume `pet` in the Python extension [source code](https://github.com/microsoft/vscode-python/blob/main/src/client/pythonEnvironments/base/locators/common/nativePythonFinder.ts). For more information on JSNORPC requests/notifications for this tool, please reference [/docs/JSONRPC.md](https://github.com/microsoft/python-environment-tools/blob/main/docs/JSONRPC.md).

## Environment Types Supported 

- python.org
- Windows Store
- PyEnv
- PyEnv-Win
- PyEnv-Virtualenv
- Conda
- Miniconda
- Miniforge
- PipEnv
- Homebrew
- VirtualEnvWrapper
- VirtualEnvWrapper-Win
- Venv
- VirtualEnv
- Python on your PATH

## Features 

- Discovery of all global Python installs
- Discovery of all Python virtual environments

## Key Methodology

Our approach prioritizes performance and efficiency by leveraging Rust. We minimize I/O operations by collecting all necessary environment information at once, which reduces repeated I/O and the need to spawn additional processes, significantly enhancing overall performance.

## Debugging Python Environment Issues

If you're experiencing issues with Python interpreter detection in VS Code (such as the Run button not working, Python not being recognized, or interpreters not persisting), you can use PET to diagnose the problem.

### Running PET for Debugging

PET can be run directly from the command line to discover all Python environments on your system. This helps identify whether the issue is with environment discovery or elsewhere.

#### Quick Start

1. **Download or build PET**:
   - Download pre-built binaries from the [releases page](https://github.com/microsoft/python-environment-tools/releases)
   - Or build from source: `cargo build --release`

2. **Run PET to find all environments**:
   ```bash
   # On Linux/macOS
   ./pet find --list --verbose
   
   # On Windows
   pet.exe find --list --verbose
   ```

3. **Share the output** with maintainers when reporting issues

#### Common Commands

- **Find all Python environments** (default behavior):
  ```bash
  pet
  ```

- **Find all environments with detailed logging**:
  ```bash
  pet find --list --verbose
  ```

- **Find all environments and output as JSON**:
  ```bash
  pet find --json
  ```

- **Search only in workspace/project directories**:
  ```bash
  pet find --list --workspace
  ```

- **Search for a specific environment type** (e.g., Conda):
  ```bash
  pet find --list --kind conda
  ```

- **Resolve a specific Python executable**:
  ```bash
  pet resolve /path/to/python
  ```

#### Understanding the Output

The output includes:

- **Discovered Environments**: List of Python installations found, including:
  - Type (Conda, Venv, System Python, etc.)
  - Executable path
  - Version
  - Prefix (sys.prefix)
  - Architecture (x64/x86)
  - Symlinks

- **Timing Information**: How long each locator took to search

- **Summary Statistics**: Count of environments by type

#### Reporting Issues

When reporting Python detection issues, please include:

1. The full output from running `pet find --list --verbose`
2. Your operating system and version
3. VS Code and Python extension versions
4. Description of the issue

This information helps maintainers diagnose whether the problem is with PET's discovery logic or elsewhere in the VS Code Python extension.

## Contributing

This project welcomes contributions and suggestions.  Most contributions require you to agree to a
Contributor License Agreement (CLA) declaring that you have the right to, and actually do, grant us
the rights to use your contribution. For details, visit https://cla.opensource.microsoft.com.

When you submit a pull request, a CLA bot will automatically determine whether you need to provide
a CLA and decorate the PR appropriately (e.g., status check, comment). Simply follow the instructions
provided by the bot. You will only need to do this once across all repos using our CLA.

This project has adopted the [Microsoft Open Source Code of Conduct](https://opensource.microsoft.com/codeofconduct/).
For more information see the [Code of Conduct FAQ](https://opensource.microsoft.com/codeofconduct/faq/) or
contact [opencode@microsoft.com](mailto:opencode@microsoft.com) with any additional questions or comments.

## Trademarks

This project may contain trademarks or logos for projects, products, or services. Authorized use of Microsoft 
trademarks or logos is subject to and must follow 
[Microsoft's Trademark & Brand Guidelines](https://www.microsoft.com/en-us/legal/intellectualproperty/trademarks/usage/general).
Use of Microsoft trademarks or logos in modified versions of this project must not cause confusion or imply Microsoft sponsorship.
Any use of third-party trademarks or logos are subject to those third-party's policies.
