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
