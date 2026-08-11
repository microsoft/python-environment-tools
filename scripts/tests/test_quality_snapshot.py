# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import argparse
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from quality_snapshot import (  # noqa: E402
    PERFORMANCE_BUDGETS,
    SnapshotError,
    compare_coverage,
    compare_performance,
    load_json,
    performance_specs,
    run_coverage,
    run_performance,
)


def performance_snapshot(
    *, refresh_p50=100, refresh_p95=500, startup_p50=10, startup_p95=20,
    first_p50=15, first_p95=30, environments=5, managers=1
):
    return {
        'server_startup_ms': startup_p50,
        'full_refresh_ms': refresh_p50,
        'time_to_first_env_ms': first_p50,
        'environments_count': environments,
        'managers_count': managers,
        'stats': {
            'server_startup': {'count': 10, 'p50': startup_p50, 'p95': startup_p95},
            'full_refresh': {'count': 10, 'p50': refresh_p50, 'p95': refresh_p95},
            'time_to_first_env': {'count': 10, 'p50': first_p50, 'p95': first_p95},
        },
    }


def write_lcov(path, *, lines_hit, lines_found, functions_hit, functions_found):
    path.write_text(
        f'SF:example.rs\nLF:{lines_found}\nLH:{lines_hit}\n'
        f'FNF:{functions_found}\nFNH:{functions_hit}\nend_of_record\n',
        encoding='utf-8',
    )


class PerformanceSnapshotTests(unittest.TestCase):
    def test_unchanged_snapshot_passes(self):
        comparisons, failures = compare_performance(performance_snapshot(), performance_snapshot(), 'Windows')
        self.assertEqual(len(comparisons), 6)
        self.assertEqual(failures, [])

    def test_p50_regression_fails_when_both_budgets_are_exceeded(self):
        current = performance_snapshot(refresh_p50=180)
        _, failures = compare_performance(current, performance_snapshot(refresh_p50=100), 'Windows')
        self.assertTrue(any('Full refresh P50' in failure for failure in failures))

    def test_p95_regression_fails_even_when_p50_is_unchanged(self):
        current = performance_snapshot(refresh_p95=7_000)
        _, failures = compare_performance(current, performance_snapshot(refresh_p95=500), 'Windows')
        self.assertTrue(any('Full refresh P95' in failure for failure in failures))

    def test_post_fix_macos_tail_variance_passes(self):
        baseline = performance_snapshot(startup_p95=621, refresh_p95=1_343, first_p95=649)
        current = performance_snapshot(startup_p95=691, refresh_p95=1_435, first_p95=745)

        _, failures = compare_performance(current, baseline, 'macOS')

        self.assertEqual(failures, [])

    def test_tightened_macos_tail_budgets_reject_multi_second_regressions(self):
        baseline = performance_snapshot(startup_p95=621, refresh_p95=1_343, first_p95=649)
        current = performance_snapshot(startup_p95=1_500, refresh_p95=2_500, first_p95=1_500)

        _, failures = compare_performance(current, baseline, 'macOS')

        for label in (
            'Server startup P95',
            'Full refresh P95',
            'Time to first environment P95',
        ):
            self.assertTrue(any(label in failure for failure in failures))

    def test_noise_inside_absolute_budget_passes(self):
        current = performance_snapshot(refresh_p50=140)
        _, failures = compare_performance(current, performance_snapshot(refresh_p50=100), 'Windows')
        self.assertEqual(failures, [])


    def test_relative_budget_must_also_be_exceeded(self):
        current = performance_snapshot(refresh_p50=1_060)
        _, failures = compare_performance(current, performance_snapshot(refresh_p50=1_000), 'Windows')
        self.assertEqual(failures, [])

    def test_platform_specific_budget_changes_decision(self):
        current = performance_snapshot(refresh_p50=140)
        baseline = performance_snapshot(refresh_p50=100)
        _, windows_failures = compare_performance(current, baseline, 'Windows')
        _, linux_failures = compare_performance(current, baseline, 'Linux')
        self.assertEqual(windows_failures, [])
        self.assertTrue(any('Full refresh P50' in failure for failure in linux_failures))

    def test_budget_metric_mismatch_is_invalid(self):
        original = PERFORMANCE_BUDGETS['windows']
        PERFORMANCE_BUDGETS['windows'] = original[:-1]
        try:
            with self.assertRaisesRegex(SnapshotError, 'does not match metric count'):
                performance_specs('Windows')
        finally:
            PERFORMANCE_BUDGETS['windows'] = original

    def test_unknown_platform_is_invalid(self):
        with self.assertRaises(SnapshotError):
            compare_performance(performance_snapshot(), performance_snapshot(), 'unknown')

    def test_inventory_mismatch_fails(self):
        current = performance_snapshot(environments=6, managers=2)
        _, failures = compare_performance(current, performance_snapshot(), 'Windows')
        self.assertTrue(any('Environment inventory changed' in failure for failure in failures))
        self.assertTrue(any('Manager inventory changed' in failure for failure in failures))

    def test_missing_metric_is_invalid(self):
        current = performance_snapshot()
        del current['stats']['full_refresh']['p95']
        with self.assertRaises(SnapshotError):
            compare_performance(current, performance_snapshot(), 'Windows')

    def test_too_few_samples_is_invalid(self):
        current = performance_snapshot()
        current['stats']['full_refresh']['count'] = 1
        with self.assertRaises(SnapshotError):
            compare_performance(current, performance_snapshot(), 'Windows')


class CoverageSnapshotTests(unittest.TestCase):
    def compare(self, current_values, baseline_values):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / 'current.info'
            baseline = Path(directory) / 'baseline.info'
            write_lcov(current, **current_values)
            write_lcov(baseline, **baseline_values)
            return compare_coverage(current, baseline)

    def test_coverage_increase_passes(self):
        _, failures = self.compare(
            dict(lines_hit=91, lines_found=100, functions_hit=46, functions_found=50),
            dict(lines_hit=90, lines_found=100, functions_hit=45, functions_found=50),
        )
        self.assertEqual(failures, [])

    def test_line_coverage_decrease_fails(self):
        _, failures = self.compare(
            dict(lines_hit=89, lines_found=100, functions_hit=45, functions_found=50),
            dict(lines_hit=90, lines_found=100, functions_hit=45, functions_found=50),
        )
        self.assertTrue(any('Line coverage decreased' in failure for failure in failures))

    def test_function_coverage_decrease_fails(self):
        _, failures = self.compare(
            dict(lines_hit=90, lines_found=100, functions_hit=44, functions_found=50),
            dict(lines_hit=90, lines_found=100, functions_hit=45, functions_found=50),
        )
        self.assertTrue(any('Function coverage decreased' in failure for failure in failures))

    def test_invalid_lcov_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / 'current.info'
            baseline = Path(directory) / 'baseline.info'
            current.write_text('SF:example.rs\nend_of_record\n', encoding='utf-8')
            write_lcov(baseline, lines_hit=1, lines_found=1, functions_hit=1, functions_found=1)
            with self.assertRaises(SnapshotError):
                compare_coverage(current, baseline)

    def test_negative_lcov_count_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / 'current.info'
            baseline = Path(directory) / 'baseline.info'
            current.write_text('SF:example.rs\nLF:-1\nLH:-1\nFNF:1\nFNH:1\n', encoding='utf-8')
            write_lcov(baseline, lines_hit=1, lines_found=1, functions_hit=1, functions_found=1)
            with self.assertRaisesRegex(SnapshotError, 'negative summary counts'):
                compare_coverage(current, baseline)

    def test_malformed_lcov_count_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            current = Path(directory) / 'current.info'
            baseline = Path(directory) / 'baseline.info'
            current.write_text('SF:example.rs\nLF:not-a-number\nLH:1\nFNF:1\nFNH:1\n', encoding='utf-8')
            write_lcov(baseline, lines_hit=1, lines_found=1, functions_hit=1, functions_found=1)
            with self.assertRaises(SnapshotError):
                compare_coverage(current, baseline)


class JsonSnapshotTests(unittest.TestCase):
    def test_malformed_json_is_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'metrics.json'
            path.write_text('{', encoding='utf-8')
            with self.assertRaises(SnapshotError):
                load_json(path)


class CommandTests(unittest.TestCase):
    def test_invalid_performance_snapshot_returns_failure_and_writes_report(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            current = directory / 'current.json'
            baseline = directory / 'baseline.json'
            report = directory / 'report.md'
            current.write_text('{', encoding='utf-8')
            baseline.write_text(json.dumps(performance_snapshot()), encoding='utf-8')

            exit_code = run_performance(
                argparse.Namespace(
                    current=current,
                    baseline=baseline,
                    platform='Windows',
                    report=report,
                    summary=None,
                )
            )

            self.assertEqual(exit_code, 1)
            self.assertIn('Invalid snapshot', report.read_text(encoding='utf-8'))

    def test_coverage_regression_returns_failure_and_writes_report(self):
        with tempfile.TemporaryDirectory() as directory:
            directory = Path(directory)
            current = directory / 'current.info'
            baseline = directory / 'baseline.info'
            report = directory / 'report.md'
            write_lcov(current, lines_hit=89, lines_found=100, functions_hit=44, functions_found=50)
            write_lcov(baseline, lines_hit=90, lines_found=100, functions_hit=45, functions_found=50)

            exit_code = run_coverage(
                argparse.Namespace(
                    current=current,
                    baseline=baseline,
                    platform='test',
                    report=report,
                    summary=None,
                )
            )

            self.assertEqual(exit_code, 1)
            self.assertIn('Blocking findings', report.read_text(encoding='utf-8'))


if __name__ == '__main__':
    unittest.main()
