# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import errno
import os
from pathlib import Path
import subprocess
import sys
import time


def try_lock(stream):
    try:
        if os.name == "nt":
            import msvcrt
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        else:
            import fcntl
            fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        return True
    except OSError as error:
        if error.errno not in (errno.EACCES, errno.EAGAIN, errno.EDEADLK):
            raise
        return False


def parent_test_is_alive():
    try:
        stream = open(os.environ["PET_SHUTDOWN_CONTROL"], "r+b")
    except FileNotFoundError:
        return False
    with stream:
        return not try_lock(stream)


def wait_for_shutdown():
    deadline = time.monotonic() + 30
    while parent_test_is_alive() and time.monotonic() < deadline:
        time.sleep(0.01)


if sys.argv[-1] == "child":
    with open(os.environ["PET_SHUTDOWN_LEASE"], "w+b") as lease:
        lease.write(b"x")
        lease.flush()
        if not try_lock(lease):
            raise RuntimeError("fixture descendant could not acquire its lease")
        Path(os.environ["PET_SHUTDOWN_READY"]).write_text("ready", encoding="ascii")
        wait_for_shutdown()
else:
    child = subprocess.Popen(
        [sys.executable, "-S", __file__, "child"],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    try:
        wait_for_shutdown()
    finally:
        child.terminate()
        child.wait(timeout=2)
