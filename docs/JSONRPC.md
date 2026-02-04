# JSONPRC Messages

The tool supports JSONRPC messages for communication.
The messages are sent over a stdio/stdout. The messages are in the form of a JSON object.

The messages/notifications supported are listed below.

This document assumes the reader is familiar with the JSONRPC 2.0 specification.
Hence there's no mention of the `jsonrpc` property in the messages.
For samples using JSONRPC, please have a look at the [sample.js](./sample.js) file.

Any requests/notifications not documented here are not supported.

# Configuration Request

This should always be the first request sent to the tool.
This request should be sent again, only if any of the configuration options change.

The request is expected to contain the configuraiton information for the tool to use.
All properties of the configuration are optional.

_Request_:

- method: `configure`
- params: `ConfigureParams` defined as below.

_Response_:

- result: `null`

```typescript
interface ConfigureParams {
  /**
   * This is a list of project directories.
   * Useful for poetry, pipenv, virtualenvwrapper and the like to discover virtual environments that belong to specific project directories.
   * E.g. `workspace folders` in vscode.
   *
   * If not provided, then environments such as poetry, pipenv, and the like will not be reported.
   * This is because poetry, pipenv, and the like are project specific enviornents.
   *
   * Glob patterns are supported (e.g., "/home/user/projects/*", "**/.venv").
   */
  workspaceDirectories?: string[];
  /**
   * This is a list of directories where we should look for python environments such as Virtual Environments created/managed by the user.
   * This is useful when the virtual environments are stored in some custom locations.
   *
   * Useful for VS Code so users can configure where they store virtual environments.
   *
   * Glob patterns are supported (e.g., "/home/user/envs/*", "/home/user/*/venv").
   */
  environmentDirectories?: string[];
  /**
   * This is the path to the conda executable.
   *
   * Useful for VS Code so users can configure where they have installed Conda.
   */
  condaExecutable?: string;
  /**
   * This is the path to the conda executable.
   *
   * Useful for VS Code so users can configure where they have installed Poetry.
   */
  poetryExecutable?: string;
  /**
   * Directory to cache Python environment details.
   * WARNING: This directory will be deleted in the `clearCache` request.
   * It is advisable to use a directory that is not used by other tools, instead have a dedicated directory just for this tool.
   *
   * Data in this directory can be deleted at any time by the client.
   */
  cacheDirectory?: string;
}
```

# Refresh Request

Performs a refresh/discovery of Python environments and reports them via `environment` and `manager` notifications.
All properties of the configuration are optional.

_Request_:

- method: `refresh`
- params: `RefreshParams` defined as below.

_Response_:

- result: `RefreshResult` defined as below.

```typescript
interface RefreshParams {
  /**
   * Limits the search to a specific kind of Python environment.
   * Ignores workspace folders passed in configuration request.
   */
  searchKind?: PythonEnvironmentKind;
} | {
  /**
   * Limits the search to a specific set of paths.
   * searchPaths can either by directories or Python prefixes/executables or combination of both.
   * Ignores workspace folders passed in configuration request.
   *
   * Glob patterns are supported:
   * - `*` matches any sequence of characters in a path component
   * - `?` matches any single character
   * - `**` matches any sequence of path components (recursive)
   * - `[...]` matches any character inside the brackets
   *
   * Examples:
   * - "/home/user/projects/*" - all directories under projects
   * - "/home/user/**/venv" - all venv directories recursively
   * - "/home/user/project[0-9]" - project0, project1, etc.
   */
  searchPaths?: string[];
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

**Notes:**

- This request will generally end up spawning the Python process to get the environment information.
  Hence it is advisable to use this request sparingly and rely on Python environments being discovered or relying on the information returned by the `refresh` request.
- If the `cacheDirectory` has been provided and the same python executable was previously spanwed (resolved), then the tool will return the cached information.

_Why use this over the `refresh` request?_

Some of the information in the Python environment returned as a result of the `refresh` request might not be available is not available in the `Environment` object.
For instance sometimes the `version` and `prefix` can be empty.
In such cases, this `resolve` request can be used to get this missing information.

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

enum PythonEnvironmentKind {
  Conda,
  Pixi,
  Homebrew,
  Pyenv,
  GlobalPaths, // Python found in global locations like PATH, /usr/bin etc.
  PyenvVirtualEnv, // Pyenv virtualenvs.
  Pipenv,
  Poetry,
  MacPythonOrg, // Python installed from python.org on Mac
  MacCommandLineTools,
  LinuxGlobal, // Python installed in Linux in paths such as `/usr/bin`, `/usr/local/bin` etc.
  MacXCode,
  Venv,
  VirtualEnv,
  VirtualEnvWrapper,
  WindowsStore,
  WindowsRegistry,
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
   * The kind of the environment.
   */
  kind?: PythonEnvironmentKind;
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
  symlinks?: string[];
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
  /**
   * An error message if the environment is known to be in a bad state.
   * For example: "Python executable is a broken symlink"
   * If undefined, no known issues have been detected (but this doesn't guarantee
   * the environment is fully functional - we don't spawn Python to verify).
   */
  error?: string;
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
  tool: "Conda" | "Poetry" | "Pyenv";
  /**
   * The version of the manager/tool.
   * In the case of conda, this is the version of conda.
   */
  version?: string;
}
```

# Clear Cache Request

Use this request to clear the cache that the tool uses to store Python environment details.

**Notes:**

- This is a noop, if a `cacheDirectory` has not been provided in the `configure` request.

**Warning:**

- The directory provided in the `cacheDirectory` in the `configure` request will be deleted.
  Hence it is advisable to use a directory that is not used by other tools, instead have a dedicated directory just for this tool.

_Request_:

- method: `find`
- params: `null`

_Response_:

- result: `null`

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
