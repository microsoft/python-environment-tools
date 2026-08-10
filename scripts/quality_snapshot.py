#!/usr/bin/env python3
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.
"""Validate and compare PET performance and coverage snapshots."""

from __future__ import annotations

import argparse
import json
import math
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Sequence


class SnapshotError(ValueError):
    """Raised when snapshot data is missing or malformed."""


@dataclass(frozen=True)
class RegressionBudget:
    absolute_ms: float
    relative_percent: float


@dataclass(frozen=True)
class MetricSpec:
    label: str
    group: str
    percentile: str


@dataclass(frozen=True)
class MetricComparison:
    label: str
    current: float
    baseline: float
    budget: RegressionBudget

    @property
    def delta(self) -> float:
        return self.current - self.baseline

    @property
    def percent_change(self) -> float:
        if self.baseline == 0:
            return math.inf if self.current > 0 else 0.0
        return self.delta / self.baseline * 100

    @property
    def regressed(self) -> bool:
        return self.delta > self.budget.absolute_ms and self.percent_change > self.budget.relative_percent


PERFORMANCE_METRICS = (
    MetricSpec('Server startup P50', 'server_startup', 'p50'),
    MetricSpec('Server startup P95', 'server_startup', 'p95'),
    MetricSpec('Full refresh P50', 'full_refresh', 'p50'),
    MetricSpec('Full refresh P95', 'full_refresh', 'p95'),
    MetricSpec('Time to first environment P50', 'time_to_first_env', 'p50'),
    MetricSpec('Time to first environment P95', 'time_to_first_env', 'p95'),
)
PERFORMANCE_BUDGETS = {
    'linux': (
        RegressionBudget(5, 100),
        RegressionBudget(50, 200),
        RegressionBudget(25, 30),
        RegressionBudget(1_000, 100),
        RegressionBudget(20, 100),
        RegressionBudget(250, 100),
    ),
    'windows': (
        RegressionBudget(10, 50),
        RegressionBudget(50, 100),
        RegressionBudget(50, 30),
        RegressionBudget(5_000, 100),
        RegressionBudget(25, 50),
        RegressionBudget(500, 100),
    ),
    'macos': (
        RegressionBudget(100, 50),
        RegressionBudget(10_000, 100),
        RegressionBudget(100, 50),
        RegressionBudget(5_000, 25),
        RegressionBudget(150, 50),
        RegressionBudget(10_000, 100),
    ),
}
COVERAGE_BUDGET_PERCENTAGE_POINTS = 0.01


def platform_key(platform: str) -> str:
    normalized = platform.casefold()
    if 'windows' in normalized:
        return 'windows'
    if 'macos' in normalized:
        return 'macos'
    if 'linux' in normalized or 'ubuntu' in normalized:
        return 'linux'
    raise SnapshotError(f'Unsupported performance platform: {platform}')


def performance_specs(platform: str) -> list[tuple[MetricSpec, RegressionBudget]]:
    return list(zip(PERFORMANCE_METRICS, PERFORMANCE_BUDGETS[platform_key(platform)], strict=True))


def load_json(path: Path) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding='utf-8'))
    except FileNotFoundError as error:
        raise SnapshotError(f'Snapshot file does not exist: {path}') from error
    except json.JSONDecodeError as error:
        raise SnapshotError(f'Snapshot file is not valid JSON: {path}: {error}') from error
    if not isinstance(value, dict):
        raise SnapshotError(f'Snapshot root must be an object: {path}')
    return value


def require_number(value: Any, name: str, *, minimum: float = 0) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)) or not math.isfinite(value):
        raise SnapshotError(f'{name} must be a finite number')
    numeric = float(value)
    if numeric < minimum:
        raise SnapshotError(f'{name} must be at least {minimum}')
    return numeric


def require_integer(value: Any, name: str, *, minimum: int = 0) -> int:
    numeric = require_number(value, name, minimum=minimum)
    if not numeric.is_integer():
        raise SnapshotError(f'{name} must be an integer')
    return int(numeric)


def performance_value(snapshot: dict[str, Any], spec: MetricSpec, source: str) -> float:
    stats = snapshot.get('stats')
    if not isinstance(stats, dict):
        raise SnapshotError(f'{source}.stats must be an object')
    group = stats.get(spec.group)
    if not isinstance(group, dict):
        raise SnapshotError(f'{source}.stats.{spec.group} must be an object')
    require_integer(group.get('count'), f'{source}.stats.{spec.group}.count', minimum=5)
    return require_number(group.get(spec.percentile), f'{source}.stats.{spec.group}.{spec.percentile}')


def compare_performance(
    current: dict[str, Any], baseline: dict[str, Any], platform: str
) -> tuple[list[MetricComparison], list[str]]:
    current_envs = require_integer(current.get('environments_count'), 'current.environments_count', minimum=1)
    baseline_envs = require_integer(baseline.get('environments_count'), 'baseline.environments_count', minimum=1)
    current_managers = require_integer(current.get('managers_count'), 'current.managers_count')
    baseline_managers = require_integer(baseline.get('managers_count'), 'baseline.managers_count')

    failures: list[str] = []
    if current_envs != baseline_envs:
        failures.append(f'Environment inventory changed: current={current_envs}, baseline={baseline_envs}')
    if current_managers != baseline_managers:
        failures.append(f'Manager inventory changed: current={current_managers}, baseline={baseline_managers}')

    comparisons = [
        MetricComparison(
            spec.label,
            performance_value(current, spec, 'current'),
            performance_value(baseline, spec, 'baseline'),
            budget,
        )
        for spec, budget in performance_specs(platform)
    ]
    failures.extend(
        f'{comparison.label} regressed by {comparison.delta:.0f}ms ({comparison.percent_change:.1f}%)'
        for comparison in comparisons
        if comparison.regressed
    )
    return comparisons, failures


def parse_lcov(path: Path) -> tuple[int, int, int, int]:
    try:
        lines = path.read_text(encoding='utf-8', errors='replace').splitlines()
    except FileNotFoundError as error:
        raise SnapshotError(f'Coverage file does not exist: {path}') from error
    lines_found = lines_hit = functions_found = functions_hit = 0
    try:
        for line in lines:
            if line.startswith('LF:'):
                lines_found += int(line[3:])
            elif line.startswith('LH:'):
                lines_hit += int(line[3:])
            elif line.startswith('FNF:'):
                functions_found += int(line[4:])
            elif line.startswith('FNH:'):
                functions_hit += int(line[4:])
    except ValueError as error:
        raise SnapshotError(f'Coverage file has a malformed summary count: {path}') from error
    if lines_found == 0 or functions_found == 0:
        raise SnapshotError(f'Coverage file has no line/function summary data: {path}')
    if lines_hit > lines_found or functions_hit > functions_found:
        raise SnapshotError(f'Coverage file has invalid hit totals: {path}')
    return lines_hit, lines_found, functions_hit, functions_found


def coverage_percent(hit: int, found: int) -> float:
    return hit / found * 100


def compare_coverage(current: Path, baseline: Path) -> tuple[dict[str, float], list[str]]:
    current_lh, current_lf, current_fnh, current_fnf = parse_lcov(current)
    baseline_lh, baseline_lf, baseline_fnh, baseline_fnf = parse_lcov(baseline)
    values = {
        'current_lines': coverage_percent(current_lh, current_lf),
        'baseline_lines': coverage_percent(baseline_lh, baseline_lf),
        'current_functions': coverage_percent(current_fnh, current_fnf),
        'baseline_functions': coverage_percent(baseline_fnh, baseline_fnf),
    }
    values['line_delta'] = values['current_lines'] - values['baseline_lines']
    values['function_delta'] = values['current_functions'] - values['baseline_functions']
    failures = []
    if values['line_delta'] < -COVERAGE_BUDGET_PERCENTAGE_POINTS:
        failures.append(f"Line coverage decreased by {abs(values['line_delta']):.3f} percentage points")
    if values['function_delta'] < -COVERAGE_BUDGET_PERCENTAGE_POINTS:
        failures.append(f"Function coverage decreased by {abs(values['function_delta']):.3f} percentage points")
    return values, failures


def status_icon(failed: bool, delta: float) -> str:
    if failed:
        return ':x:'
    if delta < 0:
        return ':white_check_mark:'
    if delta > 0:
        return ':small_red_triangle:'
    return ':heavy_minus_sign:'


def performance_report(
    platform: str,
    comparisons: Sequence[MetricComparison],
    failures: Sequence[str],
    current: dict[str, Any],
    baseline: dict[str, Any],
) -> str:
    rows = []
    for comparison in comparisons:
        rows.append(
            f'| {comparison.label} | {comparison.current:.0f}ms | {comparison.baseline:.0f}ms | '
            f'{comparison.delta:+.0f}ms | {comparison.percent_change:+.1f}% | '
            f'>{comparison.budget.absolute_ms:.0f}ms and >{comparison.budget.relative_percent:.0f}% | '
            f"{status_icon(comparison.regressed, comparison.delta)} |"
        )
    result = ':x: Regression detected' if failures else ':white_check_mark: Within regression budgets'
    report = [
        f'## Performance Report ({platform})',
        '',
        f'**Result:** {result}',
        '',
        '| Metric | PR | Baseline | Delta | Change | Blocking budget | Status |',
        '|--------|----|----------|-------|--------|-----------------|--------|',
        *rows,
        '',
        '| Workload | PR | Baseline |',
        '|----------|---:|---------:|',
        f"| Environments | {current['environments_count']} | {baseline['environments_count']} |",
        f"| Managers | {current['managers_count']} | {baseline['managers_count']} |",
    ]
    if failures:
        report.extend(['', '### Blocking findings', *[f'- {failure}' for failure in failures]])
    report.extend([
        '',
        '> A regression must exceed both the documented absolute and relative budget. '
        'Environment and manager inventories must match exactly.',
    ])
    return '\n'.join(report) + '\n'


def coverage_report(platform: str, values: dict[str, float], failures: Sequence[str]) -> str:
    result = ':x: Regression detected' if failures else ':white_check_mark: Within regression budget'
    report = [
        f'## Test Coverage Report ({platform})',
        '',
        f'**Result:** {result}',
        '',
        '| Metric | PR | Baseline | Delta |',
        '|--------|----|----------|-------|',
        f"| Lines | {values['current_lines']:.3f}% | {values['baseline_lines']:.3f}% | {values['line_delta']:+.3f}pp |",
        f"| Functions | {values['current_functions']:.3f}% | {values['baseline_functions']:.3f}% | {values['function_delta']:+.3f}pp |",
    ]
    if failures:
        report.extend(['', '### Blocking findings', *[f'- {failure}' for failure in failures]])
    report.extend(['', f'> Allowed numerical tolerance: {COVERAGE_BUDGET_PERCENTAGE_POINTS:.2f} percentage points.'])
    return '\n'.join(report) + '\n'


def write_report(report: str, report_path: Path, summary_path: Path | None) -> None:
    report_path.write_text(report, encoding='utf-8')
    if summary_path is not None:
        with summary_path.open('a', encoding='utf-8') as summary:
            summary.write(report)


def run_performance(args: argparse.Namespace) -> int:
    try:
        current = load_json(args.current)
        baseline = load_json(args.baseline)
        comparisons, failures = compare_performance(current, baseline, args.platform)
        report = performance_report(args.platform, comparisons, failures, current, baseline)
    except SnapshotError as error:
        failures = [str(error)]
        report = f'## Performance Report ({args.platform})\n\n:x: **Invalid snapshot:** {error}\n'
    write_report(report, args.report, args.summary)
    return 1 if failures else 0


def run_coverage(args: argparse.Namespace) -> int:
    try:
        values, failures = compare_coverage(args.current, args.baseline)
        report = coverage_report(args.platform, values, failures)
    except SnapshotError as error:
        failures = [str(error)]
        report = f'## Test Coverage Report ({args.platform})\n\n:x: **Invalid snapshot:** {error}\n'
    write_report(report, args.report, args.summary)
    return 1 if failures else 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest='command', required=True)
    for command, handler in (('performance', run_performance), ('coverage', run_coverage)):
        subparser = subparsers.add_parser(command)
        subparser.add_argument('--current', type=Path, required=True)
        subparser.add_argument('--baseline', type=Path, required=True)
        subparser.add_argument('--platform', required=True)
        subparser.add_argument('--report', type=Path, required=True)
        subparser.add_argument('--summary', type=Path)
        subparser.set_defaults(handler=handler)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    return args.handler(args)


if __name__ == '__main__':
    sys.exit(main())
