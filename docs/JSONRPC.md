# JSONPRC Messages

The tool supports JSONRPC messages for communication.
The messages are sent over a stdio/stdout. The messages are in the form of a JSON object.

The messages/notifications supported are listed below.

This document assumes the reader is familiar with the JSONRPC 2.0 specification.
Hence there's no mention of the `jsonrpc` property in the messages.
For samples using JSONRPC, please have a look at the [sample.js](./sample.js) file.

# Initialize/Configuration/Handshake Request

At the moment there is no support for a configuration/handshake request.
The assumption is that all consumers of this tool require discovery of Python environments, hence the `refresh` method is currently treated as the initialization/handshake request.

# Refresh Request

This should always be the first request sent to the tool.
The request is expected to contain the configuraiton information for the tool to use.
All properties of the configuration are optional.

_Request_:

- method: `refresh`
- params: `RefreshParams` defined as below.

_Response_:

- result: `RefreshResult` defined as below.

```typescript
interface RefreshParams {
    /**
     * This is a list of project directories.
     * Useful for poetry, pipenv, virtualenvwrapper and the like to discover virtual environments that belong to specific project directories.
     * E.g. `workspace folders` in vscode.
     *
     * If not provided, then environments such as poetry, pipenv, and the like will not be reported.
     * This is because poetry, pipenv, and the like are project specific enviornents.
     */
    project_directories:? string[];
    /**
     * This is a list of directories where we should look for virtual environments.
     * This is useful when the virtual environments are stored in some custom locations.
     *
     * Useful for VS Code so users can configure where they store virtual environments.
     */
    environment_directories: getCustomVirtualEnvDirs(),
    /**
     * This is the path to the conda executable.
     * If conda is installed in the usual location, there's no need to update this value.
     *
     * Useful for VS Code so users can configure where they have installed Conda.
     */
    conda_executable: getPythonSettingAndUntildify<string>(CONDAPATH_SETTING_KEY),
    /**
     * This is the path to the conda executable.
     * If Poetry is installed in the usual location, there's no need to update this value.
     *
     * Useful for VS Code so users can configure where they have installed Poetry.
     */
    poetry_executable: getPythonSettingAndUntildify<string>('poetryPath'),
}

interface RefreshResult {
  /**
   * Total time taken to refresh the list of Python environments.
   * Duration is in milliseconds.
   */
  duration: number;
}
```

# Resolve Request

Use this request to resolve a Python environment from a given Python path.
Note: This request will generally end up spawning the Python process to get the environment information.
Hence it is advisable to use this request sparingly and rely on Python environments being discovered or relying on the information returned by the `refresh` request.

_Request_:

- method: `resolve`
- params: `ResolveParams` defined as below.

_Response_:

- result: `Environment` defined as below.

```typescript
interface ResolveParams {
  /**
   * The fully qualified path to the Pyton executable.
   */
  executable: string;
}

interface Environment {
  /**
   * The display name of the enviornment.
   * Generally empty, however some tools such as Windows Registry may provide a display name.
   */
  disdplay_name?: string;
  /**
   * The name of the envirionment.
   * Generally empty, however some tools such as Conda may provide a display name.
   * In the case of conda, this is the name of the conda environment and is used in activation of the conda environment.
   */
  name?: string;
  /**
   * The fully qualified path to the executable of the envirionment.
   * Generally non-empty, however in the case of conda environmentat that do not have Python installed in them, this may be empty.
   *
   * Some times this may not be the same as the `sys.executable` retured by the Python runtime.
   * This is because this path is the shortest and/or most user friendly path to the Python executable.
   * For instance its simpler for users to remember and use /usr/local/bin/python3 as opposed to /Library/Frameworks/Python.framework/Versions/Current/bin/python3
   *
   * All known symlinks to the executable are returned in the `symlinks` property.
   */
  executable?: string;
  /**
   * The type/category of the environment.
   *
   * If an environment is discovered and the kind is not know, then `Unknown` is used.
   * I.e this is never Optional.
   */
  category:
    | "Conda" // Conda environment
    | "Homebrew" // Homebrew installed Python
    | "Pyenv" // Pyenv installed Python
    | "GlobalPaths" // Unknown Pyton environment, found in the PATH environment variable
    | "PyenvVirtualEnv" // pyenv-virtualenv environment
    | "Pipenv" // Pipenv environment
    | "Poetry" // Poetry environment
    | "MacPythonOrg" // Python installed from python.org on Mac
    | "MacCommandLineTools" // Python installed from the Mac command line tools
    | "LinuxGlobal" // Python installed from the system package manager on Linux
    | "MacXCode" // Python installed from XCode on Mac
    | "Unknown" // Unknown Python environment
    | "Venv" // Python venv environment (generally created using the `venv` module)
    | "VirtualEnv" // Python virtual environment
    | "VirtualEnvWrapper" // Virtualenvwrapper Environment
    | "WindowsStore" // Python installed from the Windows Store
    | "WindowsRegistry"; // Python installed & found in Windows Registry
  /**
   * The version of the python executable.
   * This will at a minimum contain the 3 parts of the version such as `3.8.1`.
   * Somtime it might also contain other parts of the version such as `3.8.1+` or `3.8.1.final.0`
   */
  version?: string;
  /**
   * The prefix of the Python environment as returned by `sys.prefix` in the Python runtime.
   */
  prefix?: string;
  /**
   * The bitness of the Python environment.
   */
  arch?: "x64" | "x86";
  /**
   * The list of known symlinks to the Python executable.
   * Note: These are not all the symlinks, but only the known ones.
   * & they might not necessarily be symlinks as known in the strict sense, however they are all the known executables that point to the same Python Environment.
   *
   * E.g. the exes <sys prefix>/bin/python and <sys prefix>/bin/python3 are symlinks to the same Python environment.
   */
  symlinks?: string;
  /**
   * The project folder this Python environment belongs to.
   * Poetry, Pipenv, Virtualenvwrapper and the like are project specific environments.
   * This is the folder where the project is located.
   */
  project?: string;
  /**
   * The associated manager.
   * E.g. `poetry`, `conda`, `pyenv` and the like.
   *
   * Even if a conda environment is discovered, the manager can still be empty.
   * This happens when we're unable to determine the manager associated with the environment.
   *
   * Note, just because this tool discoveres other conda environments and they all have managers associated with them, it does not mean that we can use the same manager for this environment when not know.
   * Thats because there could be multiple conda installations on the system, hence we try not to make any assumptions.
   */
  manager?: Manager;
}

interface Manager {
  /**
   * The fully qualified path to the executable of the manager.
   * E.g. fully qualified path to the conda exe.
   */
  executable: string;
  /**
   * The type of the Manager.
   */
  tool: "conda" | "poetry" | "pyenv";
  /**
   * The version of the manager/tool.
   * In the case of conda, this is the version of conda.
   */
  version?: string;
}
```

# Log Notification

Sent by the server to log messages

_Notification_:

- method: `resolve`
- params: `LogParams` defined as below.

```typescript
interface LogParams {
  /**
   * The level of the log message.
   */
  level: "info" | "warning" | "error" | "debug" | "trace";
  /**
   * Message to log.
   */
  message: string;
}
```

# Manager Notification

Sent by the server whenever an Environment Manager is discovered.

_Notification_:

- method: `manager`
- params: `Manager` defined earlier.

# Environment Notification

Sent by the server whenever an Environment is discovered.

_Notification_:

- method: `environment`
- params: `Environment` defined earlier.
