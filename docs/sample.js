// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

const {
  createMessageConnection,
  StreamMessageReader,
  StreamMessageWriter,
} = require("vscode-jsonrpc/node");
const path = require("path");
const { spawn } = require("child_process");
const { PassThrough } = require("stream");

const PET_EXE = "../target/debug/pet";

const environments = [];

async function start() {
  const readable = new PassThrough();
  const writable = new PassThrough();
  const proc = spawn(PET_EXE, ["server"], {
    env: process.env,
  });
  proc.stdout.pipe(readable, { end: false });
  proc.stderr.on("data", (data) => console.error(data.toString()));
  writable.pipe(proc.stdin, { end: false });
  const connection = createMessageConnection(
    new StreamMessageReader(readable),
    new StreamMessageWriter(writable)
  );
  connection.onError((ex) => console.error("Connection Error:", ex));
  connection.onClose(() => proc.kill());
  handleLogMessages(connection);
  handleDiscoveryMessages(connection);
  connection.listen();
  return connection;
}

/**
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
function handleLogMessages(connection) {
  connection.onNotification("log", (data) => {
    switch (data.level) {
      case "info":
        // console.info("PET: ", data.message);
        break;
      case "warning":
        console.warn("PET: ", data.message);
        break;
      case "error":
        console.error("PET: ", data.message);
        break;
      case "debug":
        // console.debug('PET: ', data.message);
        break;
      default:
        console.log("PET: ", data.message);
    }
  });
}

/**
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
function handleDiscoveryMessages(connection) {
  connection.onNotification("manager", (mgr) =>
    console.log(`Discovered Manager (${mgr.tool}) ${mgr.executable}`)
  );
  connection.onNotification("environment", (env) => {
    environments.push(env);
  });
}

/**
 * Configurating the server.
 *
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
async function configure(connection) {
  // Cache directory to store information about the environments.
  // This is optional, but recommended (e.g. spawning conda can be very slow, sometimes upto 30s).
  const cacheDir = path.join(
    process.env.TMPDIR || process.env.TEMP || path.join(process.cwd(), "temp"),
    "cache"
  );

  const configuration = {
    // List of fully qualified paths to look for Environments,
    // Generally this maps to workspace folders opened in the client, such as VS Code.
    workspaceDirectories: [process.cwd()],
    // List of fully qualified paths to look for virtual environments.
    // Leave empty, if not applicable.
    // In VS Code, users can configure custom locations where virtual environments are created.
    environmentDirectories: [],
    // Cache directory to store information about the environments.
    cacheDirectory: path.join(process.cwd(), "temp/cache"),
  };
  // This must always be the first request to the server.
  // There's no need to send this every time, unless the configuration changes.
  await connection.sendRequest("configure", configuration);
}

/**
 * Refresh the environment
 *
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 * @param {undefined | { searchKind?: string } | { searchPaths?: string[] } } search Defaults to searching for all environments on the current machine.
 * Have a look at the JSONRPC.md file for more information.
 */
async function refresh(connection, search) {
  environments.length = 0;
  const { duration } = await connection.sendRequest("refresh", search);
  const scope = search
    ? ` (in ${JSON.stringify(search)})`
    : "(in machine scope)";
  console.log(
    `Found ${environments.length} environments in ${duration}ms ${scope}`
  );
}

/**
 * Clear the cache
 *
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
async function clear(connection) {
  await connection.sendRequest("clear");
}

/**
 * Gets all possible information about the Python executable provided.
 * This will spawn the Python executable (if not already done in the past).
 * This must be used only if some of the information already avaialble is not sufficient.
 *
 * E.g. if a Python env was discovered and the version information is not know,
 * but is requried, then call this method.
 * If on the other hand, all of the information is already available, then there's no need to call this method.
 * In fact it would be better to avoid calling this method, as it will spawn a new process & consume resouces.
 *
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 * @param {String} executable Fully qualified path to the Python executable.
 */
async function resolve(connection, executable) {
  try {
    const environment = await connection.sendRequest("resolve", { executable });
    console.log(
      `Resolved (${environment.kind}, ${environment.version}) ${environment.executable}`
    );
    return environment;
  } catch (ex) {
    console.error(`Failed to resolve executable ${executable}`, ex.message);
  }
}

async function main() {
  const connection = await start();

  // First request to the server, to configure the server.
  await configure(connection);

  await refresh(connection);

  // Search for environments in the specified folders.
  // This could be a folder thats not part of the workspace and not in any known location
  // I.e. it could contain environments that have not been discovered (due to the fact that its not a common/known location).
  await refresh(connection, {
    searchPaths: [
      "/Users/user_name/temp",
      "/Users/user_name/demo/.venv",
      "/Users/user_name/demo/.venv/bin/python",
    ],
  });
  // Search for environments in the specified python environment directory.
  await refresh(connection, {
    searchPaths: ["/Users/user_name/demo/.venv/bin", "/usr/local/bin/python3"],
  });
  // Search for environments of a particular kind.
  await refresh(connection, { searchKind: "Conda" });

  // Possible this env was discovered, and the version or prefix information is not known.
  await resolve(connection, "/usr/local/bin/python3");
  await resolve(connection, "/usr/local/bin/python3"); // With cache directory provided, the Python exe will be spawned only once and cached info will be used.

  // Possible we have an enviornment that was never discovered and we need information about that.
  await resolve(connection, "/Users/user_name/demo/.venv/bin/python");

  connection.end();
  process.exit(0);
}

main();
