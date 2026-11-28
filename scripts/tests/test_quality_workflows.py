# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import copy
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from test_quality_snapshot import performance_snapshot, write_lcov


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

import quality_measurement as measurement  # noqa: E402
from quality_snapshot import SnapshotError  # noqa: E402

SAME_REPOSITORY_PR = (
    "github.event.pull_request.head.repo.full_name == github.repository"
)


def workflow_steps(path: str) -> dict[str, str]:
    workflow = (ROOT / path).read_text(encoding="utf-8")
    chunks = re.split(r"(?m)^      - name: ", workflow)[1:]
    return {chunk.splitlines()[0]: chunk for chunk in chunks}


def step_property(step: str, name: str) -> str | None:
    lines = step.splitlines()[1:]
    prefix = f"        {name}:"
    for index, line in enumerate(lines):
        if not line.startswith(prefix):
            continue

        value = line.removeprefix(prefix).strip()
        if value not in {">", ">-", "|", "|-"}:
            return value

        continuation = []
        for next_line in lines[index + 1 :]:
            if not next_line.startswith("          "):
                break
            continuation.append(next_line.strip())
        return " ".join(continuation)
    return None


def copy_measurement_helpers(directory: Path, revision: str) -> Path:
    directory.mkdir()
    for filename in ("quality_measurement.py", "quality_snapshot.py"):
        shutil.copyfile(ROOT / "scripts" / filename, directory / filename)
    (directory / "helper-revision.txt").write_text(revision + "\n", encoding="utf-8")
    return directory / "quality_measurement.py"


class QualityWorkflowTests(unittest.TestCase):
    @unittest.skipUnless(os.name == "nt", "Windows workflow")
    def test_windows_pyenv_setup_preserves_scripts_and_stops_on_clone_failure(self) -> None:
        git = shutil.which("git")
        pwsh = shutil.which("pwsh")
        if not git or not pwsh:
            self.skipTest("git and PowerShell 7 are required")

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            upstream = root / "upstream"
            binary_dir = upstream / "pyenv-win" / "bin"
            binary_dir.mkdir(parents=True)
            script_bytes = b"#!/bin/sh\nexit 0\n"
            (binary_dir / "pyenv").write_bytes(script_bytes)
            (binary_dir / "pyenv.bat").write_text("@exit /b 0\n", encoding="utf-8")
            (upstream / ".version").write_text("3.1.1\n", encoding="utf-8")

            env = os.environ.copy()
            env.update({
                "GIT_CONFIG_NOSYSTEM": "1",
                "GIT_CONFIG_GLOBAL": str(root / "empty-gitconfig"),
                "GIT_TERMINAL_PROMPT": "0",
            })

            def git_run(*args: str) -> None:
                subprocess.run([git, *args], env=env, check=True, capture_output=True)

            git_run("init", "--initial-branch=master", str(upstream))
            git_run("-C", str(upstream), "add", ".")
            git_run(
                "-C", str(upstream), "-c", "user.name=Test", "-c",
                "user.email=test@example.com", "commit", "-m", "fixture",
            )

            configs = {}
            for succeeds, target in ((True, upstream), (False, root / "missing")):
                config = root / f"gitconfig-{succeeds}"
                git_run("config", "--file", str(config), "core.autocrlf", "true")
                git_run(
                    "config", "--file", str(config),
                    f"url.{target.as_uri()}.insteadOf",
                    "https://github.com/pyenv-win/pyenv-win.git",
                )
                configs[succeeds] = config

            for workflow in ("pr-check.yml", "coverage.yml", "coverage-baseline.yml"):
                step = workflow_steps(".github/workflows/" + workflow)["Install pyenv (Windows)"]
                self.assertEqual(step_property(step, "shell"), "pwsh")
                lines = step.split("        run: |\n", 1)[1].splitlines()
                script_lines = []
                for line in lines:
                    if not line.startswith("          "):
                        break
                    script_lines.append(line[10:])
                script = "\n".join(script_lines)
                self.assertNotIn("install-pyenv-win.ps1", script)

                for succeeds in (True, False):
                    with self.subTest(workflow=workflow, clone_succeeds=succeeds):
                        profile = root / f"{workflow}-{succeeds}"
                        profile.mkdir()
                        github_path = profile / "github-path"
                        github_env = profile / "github-env"
                        github_path.touch()
                        github_env.touch()
                        process_env = env | {
                            "USERPROFILE": str(profile),
                            "GITHUB_PATH": str(github_path),
                            "GITHUB_ENV": str(github_env),
                            "GIT_CONFIG_GLOBAL": str(configs[succeeds]),
                        }
                        result = subprocess.run(
                            [pwsh, "-NoProfile", "-NonInteractive", "-Command",
                             '$ErrorActionPreference = "Stop"\n' + script],
                            env=process_env, capture_output=True, text=True, timeout=30,
                        )
                        if succeeds:
                            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                            pyenv_root = profile / ".pyenv"
                            self.assertEqual(
                                (pyenv_root / "pyenv-win" / "bin" / "pyenv").read_bytes(),
                                script_bytes,
                            )
                            self.assertEqual(
                                github_path.read_text(encoding="utf-8").splitlines(),
                                [str(pyenv_root / "pyenv-win" / "bin")],
                            )
                            self.assertEqual(
                                github_env.read_text(encoding="utf-8").splitlines(),
                                [f"PYENV_ROOT={pyenv_root}"],
                            )
                        else:
                            self.assertNotEqual(result.returncode, 0)
                            self.assertEqual(github_path.read_bytes(), b"")
                            self.assertEqual(github_env.read_bytes(), b"")

    def test_linux_conda_setup_does_not_create_or_activate_an_extra_environment(self) -> None:
        for workflow in (
            "pr-check.yml", "coverage.yml", "coverage-baseline.yml",
            "perf-tests.yml", "perf-baseline.yml",
        ):
            with self.subTest(workflow=workflow):
                path = ".github/workflows/" + workflow
                steps = workflow_steps(path)
                setup = steps["Set up Conda (Linux)"]
                self.assertRegex(setup, r"conda-incubator/setup-miniconda@[0-9a-f]{40}\b")
                for setting in (
                    'activate-environment: ""', "auto-activate: false", "run-init: false",
                ):
                    self.assertIn(setting, setup)
                self.assertNotIn("$CONDA/condabin", (ROOT / path).read_text(encoding="utf-8"))
                self.assertIn("conda info --all", steps["Check Conda version"])

    def test_macos_runners_keep_both_native_architecture_workloads(self) -> None:
        expected = {
            "pr-check.yml": [
                ("macos-15-intel", "x86_64-apple-darwin"),
                ("macos-15", "aarch64-apple-darwin"),
            ] * 2,
            "build.yml": [
                ("macos-15-intel", "x86_64-apple-darwin"),
                ("macos-15", "aarch64-apple-darwin"),
            ],
            "perf-tests.yml": [
                ("macos-15-intel", "x86_64-apple-darwin"),
                ("macos-15", "aarch64-apple-darwin"),
            ],
            "perf-baseline.yml": [
                ("macos-15-intel", "x86_64-apple-darwin"),
                ("macos-15", "aarch64-apple-darwin"),
            ],
        }
        for workflow, targets in expected.items():
            with self.subTest(workflow=workflow):
                text = (ROOT / ".github" / "workflows" / workflow).read_text(encoding="utf-8")
                self.assertEqual(
                    re.findall(r"- os: (macos-\S+)\n\s+target: (\S+)", text),
                    targets,
                )
                self.assertNotIn("macos-14", text)
                if workflow != "build.yml":
                    self.assertIn("CONDA_ARCH: ${{ matrix.conda_arch }}", text)
                    self.assertIn('if [[ "$(uname -m)" != "$CONDA_ARCH" ]]', text)
                    self.assertIn("Miniconda3-latest-MacOSX-${CONDA_ARCH}.sh", text)
        coverage = (ROOT / ".github/workflows/coverage-macos.yml").read_text(encoding="utf-8")
        self.assertIn("runs-on: macos-15\n", coverage)
        self.assertNotIn("macos-14", coverage)

    def test_coverage_pairs_clean_exact_revision_jobs_from_the_same_attempt(self) -> None:
        path = ".github/workflows/coverage.yml"
        text = (ROOT / path).read_text(encoding="utf-8")
        steps = workflow_steps(path)
        self.assertIn("revision: [base, head]", text)
        self.assertIn(
            "ref: ${{ matrix.revision == 'base' && github.event.pull_request.base.sha || github.sha }}",
            text,
        )
        self.assertIn("needs: measurements", text)
        self.assertIn("MEASUREMENTS_RESULT: ${{ needs.measurements.result }}", text)
        self.assertIn("ref: ${{ github.sha }}", steps["Checkout Workflow"])
        self.assertLess(
            text.index("- name: Stage Coverage Measurement Helper"),
            text.index("- name: Checkout Measurement Source"),
        )
        self.assertIn(
            '--output-dir "$RUNNER_TEMP/quality-tools"', steps["Stage Coverage Measurement Helper"],
        )
        self.assertIn(
            '--revision "$GITHUB_SHA"', steps["Stage Coverage Measurement Helper"],
        )
        self.assertIn(
            '"$RUNNER_TEMP/quality-tools/quality_measurement.py" capture coverage',
            steps["Capture Coverage Measurement Context"],
        )
        self.assertIn('--expected-source "$MEASURED_SHA"', steps["Capture Coverage Measurement Context"])
        self.assertIn('--workflow-revision "$GITHUB_SHA"', steps["Capture Coverage Measurement Context"])
        self.assertNotIn("python - <<", text)
        self.assertIn('prefix="$(pipenv --venv)"', steps["Create a Pipenv Environment"])
        self.assertLess(
            text.index("- name: Create a Pipenv Environment"),
            text.index("- name: Create .venv"),
        )
        self.assertIn(
            "CREATED_PIPENV_PREFIX: ${{ steps.pipenv.outputs.prefix }}",
            steps["Capture Coverage Measurement Context"],
        )
        upload = steps["Upload Coverage Measurement"]
        self.assertIn("${{ matrix.revision }}-${{ github.run_attempt }}", upload)
        for step, revision in (
            ("Download Head Coverage Measurement", "head"),
            ("Download Exact PR Base Coverage", "base"),
        ):
            self.assertIn(f"-{revision}-${{{{ github.run_attempt }}}}", steps[step])
        self.assertNotIn("wait_for_baseline.py", text)
        self.assertNotIn("dawidd6/action-download-artifact", text)

    def test_performance_builds_each_harness_before_timing_a_shared_workspace(self) -> None:
        path = ".github/workflows/perf-tests.yml"
        text = (ROOT / path).read_text(encoding="utf-8")
        steps = workflow_steps(path)
        prepare = steps["Prepare Exact Base Worktree"]
        self.assertIn('git worktree add --detach "$RUNNER_TEMP/perf-base" "$base"', prepare)
        self.assertIn('base="$PR_BASE"', prepare)
        self.assertIn("refs/remotes/origin/$DEFAULT_BRANCH", prepare)
        build = steps["Build Release and Benchmark Harnesses"]
        self.assertIn('for source in "$RUNNER_TEMP/perf-base" "$GITHUB_WORKSPACE"', build)
        self.assertIn("--no-run", build)
        for step in (build, steps["Run Exact Base Performance Tests"], steps["Run Performance Tests"]):
            self.assertIn("--target-dir target", step)
            self.assertIn("--features ci-perf", step)
        self.assertLess(
            text.index("- name: Build Release and Benchmark Harnesses"),
            text.index("- name: Run Exact Base Performance Tests"),
        )
        self.assertIn('cd "$RUNNER_TEMP/perf-base"', steps["Run Exact Base Performance Tests"])
        self.assertNotIn("CARGO_TARGET_DIR:", text)
        self.assertNotIn("GITHUB_WORKSPACE:", text)
        self.assertNotIn("wait_for_baseline.py", text)
        self.assertNotIn("dawidd6/action-download-artifact", text)
        self.assertIn("comment_header: perf-macos-arm64", text)
        self.assertIn("comment_header: perf-macos\n", text)
        self.assertIn("capture performance", steps["Capture Performance Measurement Context"])
        self.assertIn('--source-dir "$RUNNER_TEMP/perf-base"', steps["Capture Performance Measurement Context"])
        self.assertIn('--expected-source "$BASE_SHA"', steps["Capture Performance Measurement Context"])
        self.assertNotIn("python - <<", text)
        for step in ("Extract Performance Metrics", "Extract Exact Base Performance Metrics"):
            self.assertIn("always()", step_property(steps[step], "if"))
            self.assertIn("python -m json.tool", steps[step])
        for artifact in ("baseline-perf/", "measurement-context.json", "source-revision.txt", "perf-output.txt"):
            self.assertIn(artifact, steps["Upload PR Performance Results"])

    def test_comment_steps_are_fork_safe_and_non_gating(self) -> None:
        comment_steps = []
        for path in (ROOT / ".github" / "workflows").glob("*.yml"):
            workflow = path.relative_to(ROOT).as_posix()
            text = path.read_text(encoding="utf-8")
            if "marocchino/sticky-pull-request-comment" not in text:
                continue
            self.assertNotIn("pull_request_target:", text)

            for step_name, step in workflow_steps(workflow).items():
                if "marocchino/sticky-pull-request-comment" not in step:
                    continue
                comment_steps.append((workflow, step_name))
                with self.subTest(workflow=workflow, step=step_name):
                    condition = step_property(step, "if")
                    self.assertIsNotNone(condition)
                    assert condition is not None
                    self.assertIn(SAME_REPOSITORY_PR, condition)
                    self.assertEqual(step_property(step, "continue-on-error"), "true")

        self.assertEqual(
            sorted(comment_steps),
            sorted(
                (
                    (".github/workflows/coverage.yml", "Post Coverage Comment"),
                    (".github/workflows/coverage.yml", "Post Coverage Started Comment"),
                    (".github/workflows/perf-tests.yml", "Post In-Progress Comment"),
                    (".github/workflows/perf-tests.yml", "Post Performance Comment"),
                )
            ),
        )

    def test_workflow_tokens_have_only_required_permissions(self) -> None:
        for workflow, expected in (
            ("build.yml", {"contents": "read"}),
            ("pr-check.yml", {"contents": "read"}),
            ("coverage.yml", {"contents": "read", "pull-requests": "write"}),
            ("perf-tests.yml", {"contents": "read", "pull-requests": "write"}),
        ):
            with self.subTest(workflow=workflow):
                text = (ROOT / ".github" / "workflows" / workflow).read_text(encoding="utf-8")
                match = re.search(
                    r"(?m)^permissions:\n((?:  [a-z-]+: [a-z]+\n)+)", text
                )
                self.assertIsNotNone(match)
                assert match is not None
                self.assertEqual(
                    dict(re.findall(r"(?m)^  ([a-z-]+): ([a-z]+)$", match.group(1))),
                    expected,
                )

    def test_checkout_tokens_are_not_persisted_after_fetch(self) -> None:
        for workflow in ("build.yml", "lint.yml", "perf-baseline.yml", "pr-check.yml"):
            text = (ROOT / ".github" / "workflows" / workflow).read_text(encoding="utf-8")
            checkouts = [
                step for step in re.split(r"(?m)^      - name: ", text)[1:]
                if re.search(r"(?m)^        uses: actions/checkout@", step)
            ]
            with self.subTest(workflow=workflow):
                self.assertTrue(checkouts)
                for checkout in checkouts:
                    self.assertRegex(checkout, r"(?m)^          persist-credentials: false$")

    def test_coverage_proof_and_source_reports_are_required_and_uploaded(self) -> None:
        for workflow in ("coverage.yml", "coverage-baseline.yml", "coverage-macos.yml"):
            with self.subTest(workflow=workflow):
                path = ".github/workflows/" + workflow
                text = (ROOT / path).read_text(encoding="utf-8")
                steps = workflow_steps(path)
                self.assertIn("PET_SUBPROCESS_COVERAGE_PROOF:", text)
                self.assertIn("fetch-depth: 0", text)
                for name in ("Verify Isolated Server Coverage", "Report Production and Changed Coverage"):
                    self.assertEqual(step_property(steps[name], "if"), "always()")
                    self.assertIsNone(step_property(steps[name], "continue-on-error"))
                upload = next(value for value in steps.values() if "actions/upload-artifact@" in value)
                for artifact in ("lcov.info", "production-coverage/", "subprocess-coverage/", "subprocess-coverage.json"):
                    self.assertIn(artifact, upload)

    def test_macos_coverage_measures_the_exact_base_without_a_schema_bypass(self) -> None:
        steps = workflow_steps(".github/workflows/coverage-macos.yml")
        measure = steps["Measure Exact PR Base on the Same Runner"]
        self.assertIn("github.event.pull_request.base.sha", measure)
        self.assertIn("git worktree add --detach", measure)
        self.assertIn("PET_SUBPROCESS_COVERAGE_PROOF: ${{ runner.temp }}/base-subprocess-coverage.json", measure)
        self.assertNotIn("${{ github.workspace }}/subprocess-coverage.json", measure)
        self.assertIn("cargo llvm-cov --workspace", measure)
        self.assertNotIn("--features", measure)
        self.assertIn("cargo llvm-cov --workspace", steps["Collect Native macOS Coverage"])
        compare = steps["Compare Exact Base Coverage"]
        self.assertIn("--baseline baseline-lcov.info", compare)
        self.assertIn("always()", step_property(compare, "if"))
        self.assertIsNone(step_property(compare, "continue-on-error"))

    def test_report_artifacts_are_uploaded_after_comparison(self) -> None:
        cases = (
            (
                ".github/workflows/coverage.yml",
                "Compare Coverage Snapshot",
                "Upload PR Coverage Artifact",
                "coverage-report.md",
            ),
            (
                ".github/workflows/perf-tests.yml",
                "Compare Performance Snapshot",
                "Upload PR Performance Results",
                "performance-report.md",
            ),
        )

        for workflow, comparison, upload, report in cases:
            with self.subTest(workflow=workflow):
                text = (ROOT / workflow).read_text(encoding="utf-8")
                steps = workflow_steps(workflow)
                self.assertLess(
                    text.index(f"- name: {comparison}"),
                    text.index(f"- name: {upload}"),
                )
                self.assertIn(report, steps[upload])
                self.assertEqual(step_property(steps[upload], "if"), "always()")
                self.assertIsNone(step_property(steps[comparison], "continue-on-error"))


class MeasurementComparisonTests(unittest.TestCase):
    def run_measurement(self, kind, *, regression=False, invalid=None, outcomes=None):
        baseline_name = "baseline-coverage" if kind == "coverage" else "baseline-perf"
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory)
            root = parent / "workspace"
            root.mkdir()
            helper = copy_measurement_helpers(parent / "tools", "a" * 40)
            baseline = root / baseline_name
            baseline.mkdir()
            context = {
                "runner": dict.fromkeys(
                    ("RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion"), "runner-version"
                ),
                "installed_target": "x86_64-unknown-linux-musl",
                "features": "ci" if kind == "coverage" else "ci-perf",
                "helper_revision": "a" * 40,
                "versions": dict.fromkeys(("rustc", "cargo", "python", "conda"), "tool-version"),
                "interpreters": dict.fromkeys(
                    ("conda-base", "conda-named", "venv"), "Python 3.12.1"
                ),
            }
            if kind == "coverage":
                context["coverage_target"] = "host"
                context["versions"].update(dict.fromkeys(
                    ("cargo-llvm-cov", "pipenv", "pyenv", "pixi"), "tool-version"
                ))
                context["interpreters"].update(dict.fromkeys(
                    ("conda-prefix", "venv2", "pipenv", "pixi-default", "pixi-dev", "pyenv"),
                    "Python 3.12.1",
                ))
                for path, hits in ((root, 8000 if regression else 9000), (baseline, 9000)):
                    write_lcov(
                        path / "lcov.info", lines_hit=hits, lines_found=10000,
                        functions_hit=900, functions_found=1000,
                    )
            else:
                context["discovery_workspace"] = str(root.resolve())
                for path, startup in ((root, 5000 if regression else 20), (baseline, 20)):
                    snapshot = performance_snapshot(
                        schema_version=3, inventory_schema_version=2, startup_p95=startup,
                    )
                    (path / "metrics.json").write_text(json.dumps(snapshot), encoding="utf-8")
            current_context = copy.deepcopy(context)
            base_context = copy.deepcopy(context)
            if invalid == "tool":
                base_context["versions"]["rustc"] = "different compiler"
            elif invalid == "image":
                base_context["runner"]["ImageVersion"] = "different runner image"
            elif invalid == "fixture":
                base_context["interpreters"]["venv"] = "Python 3.13.1"
            elif invalid == "incomplete":
                del current_context["versions"]["rustc"]
                del base_context["versions"]["rustc"]
            elif invalid == "workload":
                current_context["features"] = base_context["features"] = "other-features"
            elif invalid == "helper":
                base_context["helper_revision"] = "b" * 40
            elif invalid == "coverage-only":
                del current_context["versions"]["pixi"]
                del base_context["versions"]["pixi"]
            elif invalid not in (
                None, "missing", "malformed", "revision", "missing-helper", "missing-dependency",
            ):
                raise ValueError(f"Unknown invalid measurement case: {invalid}")
            (root / "measurement-context.json").write_text(json.dumps(current_context), encoding="utf-8")
            (baseline / "measurement-context.json").write_text(json.dumps(base_context), encoding="utf-8")
            (root / "source-revision.txt").write_text("a" * 40 + "\n", encoding="utf-8")
            (baseline / "source-revision.txt").write_text(
                ("a" if invalid == "revision" else "b") * 40 + "\n", encoding="utf-8"
            )
            if invalid == "missing":
                (baseline / "measurement-context.json").unlink()
            elif invalid == "malformed":
                (baseline / "measurement-context.json").write_text("{", encoding="utf-8")
            elif invalid == "missing-helper":
                (helper.parent / "helper-revision.txt").unlink()
            elif invalid == "missing-dependency":
                (helper.parent / "quality_snapshot.py").unlink()
            environment = {
                **os.environ,
                "PYTHONPATH": str(ROOT / "scripts"),
                "PYTHONDONTWRITEBYTECODE": "1",
            }
            results = {
                "MEASUREMENTS_RESULT": "success",
                **dict.fromkeys((
                    "HEAD_BENCHMARK_OUTCOME", "BASE_BENCHMARK_OUTCOME",
                    "HEAD_METRICS_OUTCOME", "BASE_METRICS_OUTCOME",
                ), "success"),
                **(outcomes or {}),
            }
            report_path = root / (
                "coverage-report.md" if kind == "coverage" else "performance-report.md"
            )
            arguments = [
                sys.executable, "-B", str(helper), "compare", kind,
                "--current-dir", str(root), "--baseline-dir", str(baseline),
                "--expected-head", "a" * 40, "--expected-base", "b" * 40,
                "--target", "x86_64-unknown-linux-musl", "--workspace", str(root),
                "--platform", "Linux", "--report", str(report_path),
                "--summary", str(root / "summary.md"),
            ]
            for variable, flag in (
                ("MEASUREMENTS_RESULT", "--measurements-result"),
                ("HEAD_BENCHMARK_OUTCOME", "--head-benchmark-outcome"),
                ("BASE_BENCHMARK_OUTCOME", "--base-benchmark-outcome"),
                ("HEAD_METRICS_OUTCOME", "--head-metrics-outcome"),
                ("BASE_METRICS_OUTCOME", "--base-metrics-outcome"),
            ):
                arguments.extend((flag, results[variable]))
            result = subprocess.run(
                arguments, cwd=root, env=environment,
                capture_output=True, text=True, check=False,
            )
            if invalid == "missing-dependency":
                self.assertFalse(report_path.exists())
                return result, ""
            self.assertTrue(report_path.is_file(), result.stderr)
            return result, report_path.read_text(encoding="utf-8")

    def test_complete_measurements_retain_the_numerical_regression_gates(self) -> None:
        for kind in ("coverage", "performance"):
            for regression in (False, True):
                with self.subTest(kind=kind, regression=regression):
                    result, report = self.run_measurement(kind, regression=regression)
                    self.assertEqual(result.returncode, int(regression), result.stderr)
                    self.assertNotIn("Invalid snapshot", report)
                    self.assertIn(
                        "Regression detected" if regression else "Within regression budget", report
                    )

    def test_invalid_provenance_or_context_cannot_pass_unchanged_snapshots(self) -> None:
        for kind in ("coverage", "performance"):
            for invalid in (
                "missing", "malformed", "incomplete", "tool",
                "image", "fixture", "workload", "revision", "helper", "missing-helper",
            ):
                with self.subTest(kind=kind, invalid=invalid):
                    result, report = self.run_measurement(kind, invalid=invalid)
                    self.assertEqual(result.returncode, 1, result.stderr)
                    self.assertIn("Invalid snapshot", report)
                    self.assertNotIn("Within regression budget", report)

    def test_failed_execution_or_extraction_cannot_pass_complete_snapshots(self) -> None:
        for variable in (
            "MEASUREMENTS_RESULT", "HEAD_BENCHMARK_OUTCOME", "BASE_BENCHMARK_OUTCOME",
            "HEAD_METRICS_OUTCOME", "BASE_METRICS_OUTCOME",
        ):
            kind = "coverage" if variable == "MEASUREMENTS_RESULT" else "performance"
            with self.subTest(variable=variable):
                result, report = self.run_measurement(kind, outcomes={variable: "failure"})
                self.assertEqual(result.returncode, 1, result.stderr)
                self.assertIn("Invalid snapshot", report)
                self.assertNotIn("Within regression budget", report)

    def test_missing_staged_dependency_cannot_fall_back_to_pythonpath(self) -> None:
        result, _ = self.run_measurement("coverage", invalid="missing-dependency")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Missing measurement helper dependency", result.stderr)

    def test_coverage_still_requires_its_manager_specific_context(self) -> None:
        result, report = self.run_measurement("coverage", invalid="coverage-only")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("Missing versions measurement context", report)


class MeasurementCaptureTests(unittest.TestCase):
    def test_capture_keeps_base_source_separate_from_helper_and_discovery_workspace(self) -> None:
        for workload in ("coverage", "performance"):
            with self.subTest(workload=workload), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source = root / "old-base"
                workspace = root / "workspace"
                source.mkdir()
                workspace.mkdir()
                helper = copy_measurement_helpers(root / "tools", "a" * 40)
                output = workspace / "baseline"
                arguments = [
                    "capture", workload, "--source-dir", str(source),
                    "--workspace", str(workspace), "--output-dir", str(output),
                    "--target", "aarch64-apple-darwin", "--expected-source", "b" * 40,
                    "--workflow-revision", "a" * 40,
                ]
                if workload == "coverage":
                    arguments.extend(("--pipenv-prefix", str(workspace / "created-pipenv")))
                calls = []

                def command(*args, cwd):
                    calls.append((args, cwd))
                    return "b" * 40 if args[0] == "git" else "version"

                with (
                    patch.object(measurement, "__file__", str(helper)),
                    patch.object(measurement, "command_output", side_effect=command),
                    patch.dict(os.environ, dict.fromkeys(measurement.RUNNER_FIELDS, "runner")),
                ):
                    self.assertEqual(measurement.main(arguments), 0)
                context = json.loads((output / "measurement-context.json").read_text(encoding="utf-8"))
                self.assertEqual((output / "source-revision.txt").read_text().strip(), "b" * 40)
                self.assertEqual(context["helper_revision"], "a" * 40)
                self.assertFalse((source / "scripts" / "quality_measurement.py").exists())
                for args, cwd in calls:
                    self.assertEqual(
                        cwd, source.resolve() if args[0] in ("git", "rustc", "cargo") else workspace.resolve()
                    )
                if workload == "coverage":
                    self.assertIn("pipenv", context["versions"])
                    self.assertIn("pixi-dev", context["interpreters"])
                    self.assertIn("coverage_target", context)
                    self.assertTrue(any("test-env1" in args for args, _ in calls))
                else:
                    self.assertNotIn("pipenv", context["versions"])
                    self.assertNotIn("pixi-dev", context["interpreters"])
                    self.assertEqual(context["discovery_workspace"], str(workspace.resolve()))
                    self.assertTrue(any("perf-test-env" in args for args, _ in calls))

    def test_capture_rejects_head_reused_as_base_before_writing_evidence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = root / "evidence"
            arguments = [
                "capture", "performance", "--source-dir", str(root),
                "--workspace", str(root), "--output-dir", str(output),
                "--target", "aarch64-apple-darwin", "--expected-source", "b" * 40,
                "--workflow-revision", "a" * 40,
            ]
            with patch.object(measurement, "command_output", return_value="a" * 40):
                self.assertEqual(measurement.main(arguments), 1)
            self.assertFalse(output.exists())

    def test_stage_exports_immutable_helpers_without_copying_measured_source(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "workflow"
            source.mkdir()
            destination = root / "tools"
            files = {
                name: (ROOT / "scripts" / name).read_bytes()
                for name in ("quality_measurement.py", "quality_snapshot.py")
            }
            arguments = [
                "stage", "--source-dir", str(source), "--output-dir", str(destination),
                "--revision", "a" * 40,
            ]
            with (
                patch.object(measurement, "command_output", return_value="a" * 40),
                patch.object(
                    measurement, "helper_blob",
                    side_effect=lambda path, revision, filename: files[filename],
                ) as blobs,
            ):
                self.assertEqual(measurement.main(arguments), 0)
            for name, content in files.items():
                self.assertEqual((destination / name).read_bytes(), content)
            self.assertEqual((destination / "helper-revision.txt").read_text().strip(), "a" * 40)
            self.assertEqual(list(source.iterdir()), [])
            self.assertEqual(blobs.call_count, 2)
            self.assertTrue(all(call.args[1] == "a" * 40 for call in blobs.call_args_list))

    def test_stage_rejects_missing_helpers_and_workspace_injection(self) -> None:
        for invalid in ("revision", "missing-helper", "inside-workspace"):
            with self.subTest(invalid=invalid), tempfile.TemporaryDirectory() as directory:
                root = Path(directory)
                source = root / "workflow"
                source.mkdir()
                destination = source / "tools" if invalid == "inside-workspace" else root / "tools"
                arguments = [
                    "stage", "--source-dir", str(source), "--output-dir", str(destination),
                    "--revision", "a" * 40,
                ]
                with (
                    patch.object(
                        measurement, "command_output",
                        return_value=("b" if invalid == "revision" else "a") * 40,
                    ),
                    patch.object(measurement, "helper_blob", side_effect=SnapshotError("Missing helper")),
                ):
                    self.assertEqual(measurement.main(arguments), 1)
                self.assertFalse((destination / "helper-revision.txt").exists())


if __name__ == "__main__":
    unittest.main()
