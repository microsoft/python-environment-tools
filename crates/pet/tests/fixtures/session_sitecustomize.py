# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import os
from pathlib import Path
import sys
import time


barrier = os.environ.get("PET_SESSION_RESOLVE_BARRIER")
resolve_root = os.environ.get("PET_SESSION_RESOLVE_ROOT")
if barrier and resolve_root:
    executable = os.path.normcase(os.path.abspath(sys.executable))
    root_executable_prefix = os.path.normcase(os.path.abspath(resolve_root)) + os.sep
    if not executable.startswith(root_executable_prefix):
        barrier = None

if barrier:
    root = Path(barrier)
    (root / f"entered-{os.getpid()}").write_text("entered", encoding="ascii")
    deadline = time.monotonic() + 20
    release = root / "release"
    while not release.exists():
        if time.monotonic() >= deadline:
            (root / f"failed-{os.getpid()}").write_text("timeout", encoding="ascii")
            os._exit(86)
        time.sleep(0.005)
    (root / f"released-{os.getpid()}").write_text("released", encoding="ascii")
