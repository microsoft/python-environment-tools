#!/usr/bin/env python3
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.
"""Extract and validate privacy-safe long-lived session benchmark metrics."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tempfile
from pathlib import Path
from typing import Any, Sequence


METRICS_PREFIX = "SESSION_METRICS "
COUNT_KEYS = {
    "firstProcessEmptyDiskCache",
    "newProcessAfterFirstRefresh",
    "sameProcessWarm",
    "persistentCacheResolve",
    "cacheUsage",
    "inventoryResources",
    "resolveLatency",
    "resolveBatchResources",
}
SUCCESS_KEYS = {
    "status",
    "mode",
    "sizes",
    "samplesPerSize",
    "measurementCounts",
    "refreshScenarioSamples",
    "cacheUsage",
    "persistentCacheResolveSamples",
    "persistentCacheAfterColdResolve",
    "resolveConcurrency",
    "coldResolveBatches",
    "resolveLatencyUs",
    "overlapProcessesStarted",
    "overlapAmbientEnvironmentCount",
    "overlapAmbientManagerCount",
    "maxAmbientEnvironmentCount",
    "maxAmbientManagerCount",
    "latencyProcessesStarted",
    "inventoryResourceSamples",
    "resolveBatchResourceSamples",
    "preResolveResources",
    "barrierObservedResources",
    "postOverlapResources",
    "observedResourcePeak",
    "resourceAfter",
    "rssDeltaFromPreResolveBytes",
}
SCENARIOS = {
    "firstProcessEmptyDiskCache",
    "newProcessAfterFirstRefresh",
    "sameProcessWarm",
}
PERSISTENT_CACHE_SCENARIOS = {
    "cold": 1,
    "diskWarm": 0,
    "sameProcessWarm": 0,
}


class MetricsError(ValueError):
    """Raised when benchmark metrics are missing or inconsistent."""


def require_object(value: Any, name: str, keys: set[str]) -> dict[str, Any]:
    if not isinstance(value, dict):
        raise MetricsError(f"{name} must be an object")
    if set(value) != keys:
        missing = sorted(keys - set(value))
        extra = sorted(set(value) - keys)
        raise MetricsError(f"{name} has invalid keys; missing={missing}, extra={extra}")
    return value


def require_integer(value: Any, name: str, *, minimum: int = 0) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < minimum:
        raise MetricsError(f"{name} must be an integer of at least {minimum}")
    return value


def require_integer_list(value: Any, name: str, expected_length: int) -> list[int]:
    if not isinstance(value, list) or len(value) != expected_length:
        raise MetricsError(f"{name} must contain exactly {expected_length} samples")
    return [require_integer(item, f"{name}[{index}]") for index, item in enumerate(value)]


def validate_resource(value: Any, name: str) -> dict[str, Any]:
    resource = require_object(
        value, name, {"residentBytes", "threads", "handlesOrDescriptors"}
    )
    require_integer(resource["residentBytes"], f"{name}.residentBytes")
    for key in ("threads", "handlesOrDescriptors"):
        if resource[key] is not None:
            require_integer(resource[key], f"{name}.{key}")
    return resource


def observed_resource_peak(samples: Sequence[dict[str, Any]]) -> dict[str, Any]:
    def optional_max(key: str) -> int | None:
        known = [sample[key] for sample in samples if sample[key] is not None]
        return max(known) if known else None

    return {
        "residentBytes": max(sample["residentBytes"] for sample in samples),
        "threads": optional_max("threads"),
        "handlesOrDescriptors": optional_max("handlesOrDescriptors"),
    }


def validate_cache_value(value: Any, name: str) -> dict[str, Any]:
    cache = require_object(value, name, {"files", "bytes"})
    require_integer(cache["files"], f"{name}.files")
    require_integer(cache["bytes"], f"{name}.bytes")
    return cache


def validate_success_metrics(value: Any, expected_mode: str) -> dict[str, Any]:
    metrics = require_object(value, "metrics", SUCCESS_KEYS)
    if metrics["status"] != "passed":
        raise MetricsError("metrics.status must be 'passed'")
    if metrics["mode"] != expected_mode:
        raise MetricsError(
            f"metrics.mode must be {expected_mode!r}, got {metrics['mode']!r}"
        )

    expected_sizes = [1, 10, 100, 1000] if expected_mode == "stress" else [1, 10, 100]
    expected_samples = 10 if expected_mode == "stress" else 3
    expected_concurrency = 10 if expected_mode == "stress" else 4
    expected_batches = 5 if expected_mode == "stress" else 2
    require_integer_list(metrics["sizes"], "metrics.sizes", len(expected_sizes))
    require_integer(metrics["samplesPerSize"], "metrics.samplesPerSize")
    if metrics["sizes"] != expected_sizes:
        raise MetricsError(f"metrics.sizes must equal {expected_sizes}")
    if metrics["samplesPerSize"] != expected_samples:
        raise MetricsError(f"metrics.samplesPerSize must equal {expected_samples}")

    counts = require_object(metrics["measurementCounts"], "measurementCounts", COUNT_KEYS)
    for key in COUNT_KEYS:
        require_integer(counts[key], f"measurementCounts.{key}")
    expected_counts = {
        "firstProcessEmptyDiskCache": len(expected_sizes),
        "newProcessAfterFirstRefresh": len(expected_sizes),
        "sameProcessWarm": len(expected_sizes) * expected_samples,
        "persistentCacheResolve": len(PERSISTENT_CACHE_SCENARIOS),
        "cacheUsage": len(expected_sizes),
        "inventoryResources": len(expected_sizes),
        "resolveLatency": expected_concurrency * expected_batches,
        "resolveBatchResources": expected_batches,
    }
    if counts != expected_counts:
        raise MetricsError(
            f"measurementCounts must equal {expected_counts}, got {counts}"
        )

    scenarios = metrics["refreshScenarioSamples"]
    if not isinstance(scenarios, list) or len(scenarios) != len(expected_sizes) * 3:
        raise MetricsError("refreshScenarioSamples must contain three scenarios per size")
    seen_scenarios: set[tuple[int, str]] = set()
    for index, value in enumerate(scenarios):
        sample = require_object(
            value,
            f"refreshScenarioSamples[{index}]",
            {"scenario", "inventorySize", "sampleCount", "roundTripUs", "ttfeUs"},
        )
        scenario = sample["scenario"]
        size = require_integer(sample["inventorySize"], "inventorySize", minimum=1)
        if not isinstance(scenario, str) or scenario not in SCENARIOS or size not in expected_sizes:
            raise MetricsError(f"refreshScenarioSamples[{index}] has an invalid scenario or size")
        key = (size, scenario)
        if key in seen_scenarios:
            raise MetricsError(f"duplicate refresh scenario for size {size}: {scenario}")
        seen_scenarios.add(key)
        expected_count = expected_samples if scenario == "sameProcessWarm" else 1
        require_integer(sample["sampleCount"], "sampleCount", minimum=1)
        if sample["sampleCount"] != expected_count:
            raise MetricsError(
                f"refreshScenarioSamples[{index}].sampleCount must equal {expected_count}"
            )
        round_trips = require_integer_list(
            sample["roundTripUs"],
            f"refreshScenarioSamples[{index}].roundTripUs",
            expected_count,
        )
        ttfes = require_integer_list(
            sample["ttfeUs"],
            f"refreshScenarioSamples[{index}].ttfeUs",
            expected_count,
        )
        if any(ttfe > round_trip for ttfe, round_trip in zip(ttfes, round_trips)):
            raise MetricsError(
                f"refreshScenarioSamples[{index}] contains TTFE above round trip"
            )

    cache_usage = metrics["cacheUsage"]
    if not isinstance(cache_usage, list) or len(cache_usage) != len(expected_sizes):
        raise MetricsError("cacheUsage must contain one entry per size")
    seen_cache_sizes = set()
    for index, value in enumerate(cache_usage):
        cache = require_object(
            value,
            f"cacheUsage[{index}]",
            {
                "inventorySize",
                "beforeFirstProcess",
                "afterFirstRefresh",
                "afterWarmRefresh",
                "diskCacheAvailableForNewProcess",
            },
        )
        size = require_integer(cache["inventorySize"], "inventorySize", minimum=1)
        if size not in expected_sizes or size in seen_cache_sizes:
            raise MetricsError(f"cacheUsage[{index}] has an invalid or duplicate size")
        seen_cache_sizes.add(size)
        before = validate_cache_value(
            cache["beforeFirstProcess"], f"cacheUsage[{index}].beforeFirstProcess"
        )
        after_first = validate_cache_value(
            cache["afterFirstRefresh"], f"cacheUsage[{index}].afterFirstRefresh"
        )
        validate_cache_value(
            cache["afterWarmRefresh"], f"cacheUsage[{index}].afterWarmRefresh"
        )
        if before != {"files": 0, "bytes": 0}:
            raise MetricsError("first-process cache must be empty")
        available = cache["diskCacheAvailableForNewProcess"]
        if not isinstance(available, bool) or available != (after_first["bytes"] > 0):
            raise MetricsError(
                "diskCacheAvailableForNewProcess must exactly reflect nonzero cached bytes"
            )

    cache_control = metrics["persistentCacheResolveSamples"]
    if (
        not isinstance(cache_control, list)
        or len(cache_control) != len(PERSISTENT_CACHE_SCENARIOS)
    ):
        raise MetricsError(
            "persistentCacheResolveSamples must contain cold, disk-warm, and same-process samples"
        )
    seen_cache_control_scenarios = set()
    for index, value in enumerate(cache_control):
        sample = require_object(
            value,
            f"persistentCacheResolveSamples[{index}]",
            {"scenario", "sampleCount", "latencyUs", "interpreterProcessesStarted"},
        )
        scenario = sample["scenario"]
        if (
            not isinstance(scenario, str)
            or scenario not in PERSISTENT_CACHE_SCENARIOS
            or scenario in seen_cache_control_scenarios
        ):
            raise MetricsError(
                f"persistentCacheResolveSamples[{index}] has an invalid or duplicate scenario"
            )
        seen_cache_control_scenarios.add(scenario)
        if require_integer(sample["sampleCount"], "sampleCount", minimum=1) != 1:
            raise MetricsError(
                f"persistentCacheResolveSamples[{index}].sampleCount must equal 1"
            )
        require_integer_list(
            sample["latencyUs"],
            f"persistentCacheResolveSamples[{index}].latencyUs",
            1,
        )
        processes = require_integer(
            sample["interpreterProcessesStarted"], "interpreterProcessesStarted"
        )
        if processes != PERSISTENT_CACHE_SCENARIOS[scenario]:
            raise MetricsError(
                f"persistentCacheResolveSamples[{index}] has an invalid interpreter process count"
            )
    persistent_cache = validate_cache_value(
        metrics["persistentCacheAfterColdResolve"],
        "persistentCacheAfterColdResolve",
    )
    if persistent_cache["files"] == 0 or persistent_cache["bytes"] == 0:
        raise MetricsError("cold real resolve must produce a nonempty persistent cache")

    require_integer(metrics["resolveConcurrency"], "resolveConcurrency", minimum=1)
    require_integer(metrics["coldResolveBatches"], "coldResolveBatches", minimum=1)
    if metrics["resolveConcurrency"] != expected_concurrency:
        raise MetricsError(f"resolveConcurrency must equal {expected_concurrency}")
    if metrics["coldResolveBatches"] != expected_batches:
        raise MetricsError(f"coldResolveBatches must equal {expected_batches}")
    require_integer_list(
        metrics["resolveLatencyUs"],
        "resolveLatencyUs",
        expected_concurrency * expected_batches,
    )
    for name in (
        "overlapProcessesStarted",
        "overlapAmbientEnvironmentCount",
        "overlapAmbientManagerCount",
        "maxAmbientEnvironmentCount",
        "maxAmbientManagerCount",
        "latencyProcessesStarted",
    ):
        require_integer(metrics[name], name)
    if metrics["overlapProcessesStarted"] != expected_concurrency:
        raise MetricsError("overlapProcessesStarted does not match resolveConcurrency")
    if metrics["latencyProcessesStarted"] != expected_concurrency * expected_batches:
        raise MetricsError("latencyProcessesStarted does not match resolve batch work")

    inventory_resources = metrics["inventoryResourceSamples"]
    if not isinstance(inventory_resources, list) or len(inventory_resources) != len(expected_sizes):
        raise MetricsError("inventoryResourceSamples must contain one entry per size")
    seen_resource_sizes = set()
    resource_samples = []
    for index, value in enumerate(inventory_resources):
        sample = require_object(
            value,
            f"inventoryResourceSamples[{index}]",
            {"inventorySize", "resources"},
        )
        size = require_integer(sample["inventorySize"], "inventorySize", minimum=1)
        if size not in expected_sizes or size in seen_resource_sizes:
            raise MetricsError(
                f"inventoryResourceSamples[{index}] has an invalid or duplicate size"
            )
        seen_resource_sizes.add(size)
        resource_samples.append(
            validate_resource(
                sample["resources"], f"inventoryResourceSamples[{index}].resources"
            )
        )

    batch_resources = metrics["resolveBatchResourceSamples"]
    if not isinstance(batch_resources, list) or len(batch_resources) != expected_batches:
        raise MetricsError("resolveBatchResourceSamples must contain one entry per batch")
    seen_batches = set()
    for index, value in enumerate(batch_resources):
        sample = require_object(
            value,
            f"resolveBatchResourceSamples[{index}]",
            {"batch", "resources"},
        )
        batch = require_integer(sample["batch"], "batch")
        if batch not in range(expected_batches) or batch in seen_batches:
            raise MetricsError(
                f"resolveBatchResourceSamples[{index}] has an invalid or duplicate batch"
            )
        seen_batches.add(batch)
        resource_samples.append(
            validate_resource(
                sample["resources"], f"resolveBatchResourceSamples[{index}].resources"
            )
        )

    for name in (
        "preResolveResources",
        "barrierObservedResources",
        "postOverlapResources",
        "resourceAfter",
    ):
        resource_samples.append(validate_resource(metrics[name], name))
    reported_peak = validate_resource(
        metrics["observedResourcePeak"], "observedResourcePeak"
    )
    expected_peak = observed_resource_peak(resource_samples)
    if reported_peak != expected_peak:
        raise MetricsError(
            f"observedResourcePeak must equal {expected_peak}, got {reported_peak}"
        )

    reported_delta = require_integer(
        metrics["rssDeltaFromPreResolveBytes"],
        "rssDeltaFromPreResolveBytes",
        minimum=-(2**63),
    )
    expected_delta = (
        metrics["resourceAfter"]["residentBytes"]
        - metrics["preResolveResources"]["residentBytes"]
    )
    if reported_delta != expected_delta:
        raise MetricsError(
            "rssDeltaFromPreResolveBytes must equal "
            f"{expected_delta}, got {reported_delta}"
        )
    return metrics


def failed_metrics(mode: str, reason: str) -> dict[str, Any]:
    return {
        "status": "failed",
        "mode": mode,
        "metricsProduced": False,
        "failureReason": reason,
        "measurementCounts": {key: 0 for key in sorted(COUNT_KEYS)},
    }


def write_json(path: Path, value: dict[str, Any]) -> None:
    with tempfile.NamedTemporaryFile(
        mode="w", encoding="utf-8", dir=path.parent, prefix=f".{path.name}.", delete=False
    ) as temporary:
        temporary_path = Path(temporary.name)
        try:
            temporary.write(json.dumps(value, indent=2, sort_keys=True) + "\n")
        except (OSError, TypeError, ValueError):
            temporary.close()
            temporary_path.unlink(missing_ok=True)
            raise
    try:
        os.replace(temporary_path, path)
    finally:
        temporary_path.unlink(missing_ok=True)


def extract_metrics(
    input_path: Path,
    output_path: Path,
    benchmark_status: int,
    expected_mode: str,
) -> dict[str, Any]:
    try:
        payloads = [
            line.split(METRICS_PREFIX, 1)[1]
            for line in input_path.read_text(encoding="utf-8").splitlines()
            if METRICS_PREFIX in line
        ]
        if len(payloads) != 1:
            raise MetricsError(
                f"expected exactly one SESSION_METRICS payload, found {len(payloads)}"
            )
        try:
            decoded = json.loads(payloads[0])
        except json.JSONDecodeError as error:
            raise MetricsError("SESSION_METRICS payload is not valid JSON") from error
        metrics = validate_success_metrics(decoded, expected_mode).copy()
    except (MetricsError, OSError, UnicodeError) as error:
        write_json(output_path, failed_metrics(expected_mode, str(error)))
        raise MetricsError(str(error)) from error

    metrics["status"] = "passed" if benchmark_status == 0 else "failed"
    metrics["metricsProduced"] = True
    write_json(output_path, metrics)
    return metrics


def validate_artifact(value: Any, mode: str) -> None:
    if not isinstance(value, dict) or not isinstance(value.get("metricsProduced"), bool):
        raise MetricsError("artifact must declare whether metrics were produced")
    if value["metricsProduced"]:
        if value.get("status") not in ("passed", "failed"):
            raise MetricsError("artifact status must be passed or failed")
        metrics = value.copy()
        del metrics["metricsProduced"]
        metrics["status"] = "passed"
        validate_success_metrics(metrics, mode)
    else:
        require_object(value, "failed artifact", {
            "status", "mode", "metricsProduced", "failureReason", "measurementCounts"
        })
        if value["status"] != "failed" or value["mode"] != mode:
            raise MetricsError("failed artifact has invalid status or mode")
        if not isinstance(value["failureReason"], str) or not value["failureReason"]:
            raise MetricsError("failed artifact must include a failure reason")
        counts = require_object(value["measurementCounts"], "measurementCounts", COUNT_KEYS)
        for key, count in counts.items():
            if require_integer(count, f"measurementCounts.{key}") != 0:
                raise MetricsError("failed artifact without metrics must have zero counts")


def ensure_failure_metrics(output_path: Path, mode: str, reason: str) -> None:
    if output_path.exists():
        try:
            validate_artifact(json.loads(output_path.read_text(encoding="utf-8")), mode)
            return
        except (MetricsError, OSError, UnicodeError, json.JSONDecodeError) as error:
            print(f"Replacing invalid session metrics artifact: {error}", file=sys.stderr)
    write_json(output_path, failed_metrics(mode, reason))


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    subparsers = parser.add_subparsers(dest="command", required=True)

    extract = subparsers.add_parser("extract")
    extract.add_argument("--input", type=Path, required=True)
    extract.add_argument("--output", type=Path, required=True)
    extract.add_argument("--benchmark-status", type=int, required=True)
    extract.add_argument("--mode", choices=("fast", "stress"), required=True)

    ensure = subparsers.add_parser("ensure-failure")
    ensure.add_argument("--output", type=Path, required=True)
    ensure.add_argument("--mode", choices=("fast", "stress"), required=True)
    ensure.add_argument("--reason", required=True)

    args = parser.parse_args(argv)
    if args.command == "ensure-failure":
        ensure_failure_metrics(args.output, args.mode, args.reason)
        return 0
    try:
        extract_metrics(
            args.input,
            args.output,
            args.benchmark_status,
            args.mode,
        )
    except MetricsError as error:
        parser.exit(1, f"session metrics validation failed: {error}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
