# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import re
import unittest
from pathlib import Path


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


class QualityWorkflowTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
