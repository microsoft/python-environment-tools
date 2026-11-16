#!/usr/bin/env python3
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.
"""Stage, capture, and validate comparable exact-base quality measurements."""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any, Sequence

if not Path(__file__).with_name("quality_snapshot.py").is_file():
    raise SystemExit("Missing measurement helper dependency: quality_snapshot.py")

from quality_snapshot import (  # noqa: E402
    SnapshotError,
    load_json,
    run_coverage,
    run_performance,
    write_report,
)


RUNNER_FIELDS = ("RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion")
COMMON_TOOLS = {
    "rustc": ("rustc", "-vV"),
    "cargo": ("cargo", "--version"),
    "python": ("python", "--version"),
    "conda": ("conda", "--version"),
}
COVERAGE_TOOLS = {
    "cargo-llvm-cov": ("cargo", "llvm-cov", "--version"),
    "pipenv": ("pipenv", "--version"),
    "pyenv": ("pyenv", "--version"),
    "pixi": ("pixi", "--version"),
}
COMMON_INTERPRETERS = ("conda-base", "conda-named", "venv")
COVERAGE_INTERPRETERS = (
    "conda-prefix", "venv2", "pipenv", "pixi-default", "pixi-dev", "pyenv",
)
HELPER_FILES = ("quality_measurement.py", "quality_snapshot.py")
HELPER_REVISION = "helper-revision.txt"


def require_revision(value: str, description: str) -> str:
    if re.fullmatch(r"[0-9a-f]{40}", value) is None:
        raise SnapshotError(f"Missing or invalid {description}")
    return value


def command_output(*arguments: str, cwd: Path) -> str:
    executable = shutil.which(arguments[0])
    if executable is None:
        raise SnapshotError(f"Required measurement tool not found: {arguments[0]}")
    try:
        value = subprocess.check_output(
            [executable, *arguments[1:]], cwd=cwd, text=True, encoding="utf-8",
            stderr=subprocess.PIPE,
        ).strip()
    except subprocess.CalledProcessError as error:
        raise SnapshotError(
            f"Measurement command failed ({arguments[0]}): {error.stderr.strip()}"
        ) from error
    if not value:
        raise SnapshotError(f"Empty measurement output: {arguments[0]}")
    return value


def source_revision(directory: Path, expected: str) -> str:
    require_revision(expected, "expected source revision")
    actual = command_output("git", "--no-pager", "rev-parse", "HEAD", cwd=directory)
    if actual != expected:
        raise SnapshotError(f"Unexpected source revision in {directory}: {actual}")
    return actual


def helper_revision(expected: str) -> str:
    require_revision(expected, "workflow revision")
    actual = Path(__file__).with_name(HELPER_REVISION).read_text(encoding="utf-8").strip()
    if actual != expected:
        raise SnapshotError("Measurement helper is not from the tested workflow revision")
    return actual


def helper_blob(directory: Path, revision: str, filename: str) -> bytes:
    try:
        content = subprocess.check_output(
            ["git", "--no-pager", "-C", str(directory), "show", f"{revision}:scripts/{filename}"],
            stderr=subprocess.PIPE,
        )
    except subprocess.CalledProcessError as error:
        details = error.stderr.decode("utf-8", errors="replace").strip()
        raise SnapshotError(f"Missing helper {filename} at {revision}: {details}") from error
    if not content:
        raise SnapshotError(f"Empty helper {filename} at {revision}")
    return content


def run_stage(args: argparse.Namespace) -> int:
    source = args.source_dir.resolve()
    destination = args.output_dir.resolve()
    revision = source_revision(source, args.revision)
    if destination.is_relative_to(source):
        raise SnapshotError("Measurement helpers must be staged outside the discovery workspace")
    files = {name: helper_blob(source, revision, name) for name in HELPER_FILES}
    destination.mkdir(parents=True, exist_ok=True)
    for name, content in files.items():
        (destination / name).write_bytes(content)
    (destination / HELPER_REVISION).write_text(revision + "\n", encoding="utf-8")
    print(f"Staged measurement helpers from {revision}")
    return 0


def venv_python(prefix: Path) -> Path:
    return prefix / "Scripts" / "python.exe" if os.name == "nt" else prefix / "bin" / "python"


def capture_context(args: argparse.Namespace) -> tuple[str, dict[str, Any]]:
    if not args.target.strip():
        raise SnapshotError("Measurement target must not be empty")
    if args.workload == "coverage" and args.pipenv_prefix is None:
        raise SnapshotError("Coverage capture requires the created Pipenv prefix")
    source = args.source_dir.resolve()
    workspace = args.workspace.resolve()
    revision = source_revision(source, args.expected_source)
    helper = helper_revision(args.workflow_revision)
    runner = {key: os.environ.get(key, "") for key in RUNNER_FIELDS}
    if not all(value.strip() for value in runner.values()):
        raise SnapshotError("Runner image context must not be empty")
    tools = dict(COMMON_TOOLS)
    if args.workload == "coverage":
        tools.update(COVERAGE_TOOLS)
    versions = {
        name: command_output(*command, cwd=source if name in ("rustc", "cargo", "cargo-llvm-cov") else workspace)
        for name, command in tools.items()
    }
    interpreters = {
        "venv": command_output(str(venv_python(workspace / ".venv")), "--version", cwd=workspace),
        "conda-base": command_output("conda", "run", "--name", "base", "python", "--version", cwd=workspace),
        "conda-named": command_output(
            "conda", "run", "--name",
            "test-env1" if args.workload == "coverage" else "perf-test-env",
            "python", "--version", cwd=workspace,
        ),
    }
    context: dict[str, Any] = {
        "runner": runner,
        "installed_target": args.target,
        "features": "ci" if args.workload == "coverage" else "ci-perf",
        "helper_revision": helper,
        "versions": versions,
        "interpreters": interpreters,
    }
    if args.workload == "coverage":
        for name, prefix in (
            ("venv2", workspace / ".venv2"),
            ("pipenv", args.pipenv_prefix.resolve()),
        ):
            interpreters[name] = command_output(str(venv_python(prefix)), "--version", cwd=workspace)
        for name in ("default", "dev"):
            prefix = workspace / ".pixi" / "envs" / name
            executable = prefix / "python.exe" if os.name == "nt" else prefix / "bin" / "python"
            interpreters[f"pixi-{name}"] = command_output(str(executable), "--version", cwd=workspace)
        interpreters["conda-prefix"] = command_output(
            "conda", "run", "--prefix", str(workspace / "prefix-envs" / ".conda1"),
            "python", "--version", cwd=workspace,
        )
        interpreters["pyenv"] = "\n".join(sorted(
            command_output("pyenv", "versions", "--bare", cwd=workspace).splitlines()
        ))
        context["coverage_target"] = "host"
    else:
        context["discovery_workspace"] = str(workspace)
    return revision, context


def run_capture(args: argparse.Namespace) -> int:
    revision, context = capture_context(args)
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "source-revision.txt").write_text(revision + "\n", encoding="utf-8")
    (args.output_dir / "measurement-context.json").write_text(
        json.dumps(context, indent=2, sort_keys=True) + "\n", encoding="utf-8",
    )
    return 0


def read_context(directory: Path, args: argparse.Namespace) -> dict[str, Any]:
    path = directory / "measurement-context.json"
    value = load_json(path)
    required = {
        "runner": RUNNER_FIELDS,
        "versions": (*COMMON_TOOLS, *COVERAGE_TOOLS) if args.workload == "coverage" else tuple(COMMON_TOOLS),
        "interpreters": (
            (*COMMON_INTERPRETERS, *COVERAGE_INTERPRETERS)
            if args.workload == "coverage" else COMMON_INTERPRETERS
        ),
    }
    for group, keys in required.items():
        values = value.get(group)
        if not isinstance(values, dict) or any(
            not isinstance(values.get(key), str) or not values[key].strip() for key in keys
        ):
            raise SnapshotError(f"Missing {group} measurement context in {path}")
    if (
        value.get("features") != ("ci" if args.workload == "coverage" else "ci-perf")
        or value.get("installed_target") != args.target
        or value.get("helper_revision") != args.expected_head
    ):
        raise SnapshotError(f"Incompatible {args.workload} workload or helper revision in {path}")
    if args.workload == "coverage":
        if value.get("coverage_target") != "host":
            raise SnapshotError(f"Incompatible coverage target in {path}")
    elif args.workspace is None or value.get("discovery_workspace") != str(args.workspace.resolve()):
        raise SnapshotError(f"Incompatible performance discovery workspace in {path}")
    return value


def run_compare(args: argparse.Namespace) -> int:
    title = "Test Coverage Report" if args.workload == "coverage" else "Performance Report"
    try:
        if not args.target.strip():
            raise SnapshotError("Measurement target must not be empty")
        helper_revision(args.expected_head)
        if args.workload == "coverage":
            if args.measurements_result != "success":
                raise SnapshotError("One or more coverage measurements or required proofs failed")
        else:
            for name in (
                "head_benchmark_outcome", "base_benchmark_outcome",
                "head_metrics_outcome", "base_metrics_outcome",
            ):
                if getattr(args, name) != "success":
                    raise SnapshotError(f"Required benchmark or JSON extraction failed: {name}")
        for directory, expected in (
            (args.current_dir, args.expected_head), (args.baseline_dir, args.expected_base),
        ):
            require_revision(expected, "expected source revision")
            path = directory / "source-revision.txt"
            if path.read_text(encoding="utf-8").strip() != expected:
                raise SnapshotError(f"Unexpected source revision in {path}")
        current = read_context(args.current_dir, args)
        baseline = read_context(args.baseline_dir, args)
        if current != baseline:
            raise SnapshotError(
                f"{args.workload.capitalize()} runner, tool, or fixture versions differ; inspect both measurement contexts"
            )
    except (OSError, UnicodeError, SnapshotError) as error:
        print(f"Invalid {args.workload} measurement: {error}", file=sys.stderr)
        write_report(
            f"## {title} ({args.platform})\n\n:x: **Invalid snapshot:** {error}\n",
            args.report, args.summary,
        )
        return 1
    filename = "lcov.info" if args.workload == "coverage" else "metrics.json"
    comparator = run_coverage if args.workload == "coverage" else run_performance
    return comparator(argparse.Namespace(
        current=args.current_dir / filename, baseline=args.baseline_dir / filename,
        platform=args.platform, report=args.report, summary=args.summary,
    ))


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    stage = commands.add_parser("stage", help="Export the tested workflow's helpers before checking out old source")
    stage.add_argument("--source-dir", type=Path, required=True)
    stage.add_argument("--output-dir", type=Path, required=True)
    stage.add_argument("--revision", required=True)
    stage.set_defaults(handler=run_stage)
    capture = commands.add_parser("capture", help="Record a revision's tool and fixture context")
    capture.add_argument("workload", choices=("coverage", "performance"))
    capture.add_argument("--source-dir", type=Path, required=True)
    capture.add_argument("--workspace", type=Path, required=True)
    capture.add_argument("--output-dir", type=Path, required=True)
    capture.add_argument("--target", required=True)
    capture.add_argument("--expected-source", required=True)
    capture.add_argument("--workflow-revision", required=True)
    capture.add_argument("--pipenv-prefix", type=Path)
    capture.set_defaults(handler=run_capture)
    compare = commands.add_parser("compare", help="Validate provenance and run the unchanged numerical gate")
    compare.add_argument("workload", choices=("coverage", "performance"))
    compare.add_argument("--current-dir", type=Path, required=True)
    compare.add_argument("--baseline-dir", type=Path, required=True)
    compare.add_argument("--expected-head", required=True)
    compare.add_argument("--expected-base", required=True)
    compare.add_argument("--target", required=True)
    compare.add_argument("--workspace", type=Path)
    compare.add_argument("--platform", required=True)
    compare.add_argument("--report", type=Path, required=True)
    compare.add_argument("--summary", type=Path)
    compare.add_argument("--measurements-result")
    for name in ("head-benchmark-outcome", "base-benchmark-outcome", "head-metrics-outcome", "base-metrics-outcome"):
        compare.add_argument("--" + name)
    compare.set_defaults(handler=run_compare)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    try:
        return args.handler(args)
    except (OSError, UnicodeError, SnapshotError) as error:
        print(f"Invalid measurement helper operation: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
