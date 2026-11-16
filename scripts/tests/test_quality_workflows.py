# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import copy
import json
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

from test_quality_snapshot import performance_snapshot, write_lcov


ROOT = Path(__file__).resolve().parents[2]
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


def workflow_python(step: str) -> str:
    source = step.split("          python - <<'PY'\n", 1)[1]
    return textwrap.dedent(source.split("\n          PY", 1)[0])


class QualityWorkflowTests(unittest.TestCase):
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
            "perf-tests.yml": [("macos-15-intel", "x86_64-apple-darwin")],
            "perf-baseline.yml": [("macos-15-intel", "x86_64-apple-darwin")],
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
        workflow = "coverage.yml" if kind == "coverage" else "perf-tests.yml"
        step_name = "Compare Coverage Snapshot" if kind == "coverage" else "Compare Performance Snapshot"
        code = workflow_python(workflow_steps(".github/workflows/" + workflow)[step_name])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            baseline = root / baseline_name
            baseline.mkdir()
            context = {
                "runner": dict.fromkeys(
                    ("RUNNER_OS", "RUNNER_ARCH", "ImageOS", "ImageVersion"), "runner-version"
                ),
                "installed_target": "x86_64-unknown-linux-musl",
                "features": "ci" if kind == "coverage" else "ci-perf",
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
            elif invalid not in (None, "missing", "malformed", "revision"):
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
            environment = {
                **os.environ,
                "PYTHONPATH": str(ROOT / "scripts"),
                "PYTHONDONTWRITEBYTECODE": "1",
                "GITHUB_WORKSPACE": str(root),
                "GITHUB_STEP_SUMMARY": str(root / "summary.md"),
                "PLATFORM": "Linux",
                "MEASUREMENT_TARGET": "x86_64-unknown-linux-musl",
                "HEAD_SHA": "a" * 40,
                "BASE_SHA": "b" * 40,
                "MEASUREMENTS_RESULT": "success",
                **dict.fromkeys((
                    "HEAD_BENCHMARK_OUTCOME", "BASE_BENCHMARK_OUTCOME",
                    "HEAD_METRICS_OUTCOME", "BASE_METRICS_OUTCOME",
                ), "success"),
                **(outcomes or {}),
            }
            result = subprocess.run(
                [sys.executable, "-B", "-c", code], cwd=root, env=environment,
                capture_output=True, text=True, check=False,
            )
            report_path = root / (
                "coverage-report.md" if kind == "coverage" else "performance-report.md"
            )
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
                "image", "fixture", "workload", "revision",
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


if __name__ == "__main__":
    unittest.main()
