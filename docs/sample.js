// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

const {
  createMessageConnection,
  StreamMessageReader,
  StreamMessageWriter,
} = require("vscode-jsonrpc/node");
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
        // console.info('PET: ', data.message);
        break;
      case "warning":
        console.warn("PET: ", data.message);
        break;
      case "error":
        console.error("PET: ", data.message);
        break;
      case "debug":
        // consol.debug('PET: ', data.message);
        break;
      default:
      // console.trace('PET: ', data.message);
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
 * Refresh the environment
 *
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
async function refresh(connection) {
  const configuration = {
    // List of fully qualified paths to look for Environments,
    // Generally this maps to workspace folders opened in the client, such as VS Code.
    project_directories: [process.cwd()],
    // List of fully qualified paths to look for virtual environments.
    // Leave empty, if not applicable.
    // In VS Code, users can configure custom locations where virtual environments are created.
    environment_directories: [],
    // Fully qualified path to the conda executable.
    // Leave emtpy, if not applicable.
    // In VS Code, users can provide the path to the executable as a hint to the location of where Conda is installed.
    // Note: This should only be used if its known that PET is unable to find some Conda envs.
    // However thats only a work around, ideally the issue should be reported to PET and fixed
    conda_executable: undefined,
    // Fully qualified path to the poetry executable.
    // Leave emtpy, if not applicable.
    // In VS Code, users can provide the path to the executable as a hint to the location of where Poetry is installed.
    // Note: This should only be used if its known that PET is unable to find some Poetry envs.
    // However thats only a work around, ideally the issue should be reported to PET and fixed
    poetry_executable: undefined,
  };

  return connection
    .sendRequest("refresh", configuration)
    .then(({ duration }) =>
      console.log(`Found ${environments.length} environments in ${duration}ms.`)
    );
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
 * @param {String} executable Fully qualified path to the Python executable.
 * @param {import("vscode-jsonrpc").MessageConnection} connection
 */
async function resolve(executable, connection) {
  try {
    const { environment, duration } = await connection.sendRequest(
      "resolve",
      executable
    );
    console.log(
      `Resolved (${environment.kind}, ${environment.version}) ${environment.executable} in ${duration}ms`
    );
    return environment;
  } catch (ex) {
    console.error(`Failed to resolve executable ${executable}`, ex.message);
  }
}

async function main() {
  const connection = await start();
  await refresh(connection);
  // Possible this env was discovered, and the version or prefix information is not known.
  const env = await resolve("/usr/local/bin/python3", connection);
  // Possible we have an enviornment that was never discovered and we need information about that.
  const env2 = await resolve("<some Path>/.venv/bin/python", connection);

  connection.end();
  process.exit(0);
}

main();
