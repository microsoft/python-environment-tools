# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import copy
import json
import os
import runpy
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch


ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "scripts"))

from session_metrics import (MetricsError, ensure_failure_metrics, extract_metrics,
                             failed_metrics, write_json)  # noqa: E402


def resource():
    return {
        "residentBytes": 100,
        "threads": 2,
        "handlesOrDescriptors": 3,
    }


def valid_metrics():
    sizes = [1, 10, 100]
    scenarios = []
    for size in sizes:
        for scenario, count in (
            ("firstProcessEmptyDiskCache", 1),
            ("newProcessAfterFirstRefresh", 1),
            ("sameProcessWarm", 3),
        ):
            scenarios.append(
                {
                    "scenario": scenario,
                    "inventorySize": size,
                    "sampleCount": count,
                    "roundTripUs": [20] * count,
                    "ttfeUs": [10] * count,
                }
            )
    return {
        "status": "passed",
        "mode": "fast",
        "sizes": sizes,
        "samplesPerSize": 3,
        "measurementCounts": {
            "firstProcessEmptyDiskCache": 3,
            "newProcessAfterFirstRefresh": 3,
            "sameProcessWarm": 9,
            "persistentCacheResolve": 3,
            "cacheUsage": 3,
            "inventoryResources": 3,
            "resolveLatency": 8,
            "resolveBatchResources": 2,
        },
        "refreshScenarioSamples": scenarios,
        "cacheUsage": [
            {
                "inventorySize": size,
                "beforeFirstProcess": {"files": 0, "bytes": 0},
                "afterFirstRefresh": {"files": 0, "bytes": 0},
                "afterWarmRefresh": {"files": 0, "bytes": 0},
                "diskCacheAvailableForNewProcess": False,
            }
            for size in sizes
        ],
        "persistentCacheResolveSamples": [
            {
                "scenario": scenario,
                "sampleCount": 1,
                "latencyUs": [50],
                "interpreterProcessesStarted": processes,
            }
            for scenario, processes in (
                ("cold", 1),
                ("diskWarm", 0),
                ("sameProcessWarm", 0),
            )
        ],
        "persistentCacheAfterColdResolve": {"files": 1, "bytes": 100},
        "resolveConcurrency": 4,
        "coldResolveBatches": 2,
        "resolveLatencyUs": [50] * 8,
        "overlapProcessesStarted": 4,
        "overlapAmbientEnvironmentCount": 1,
        "overlapAmbientManagerCount": 1,
        "maxAmbientEnvironmentCount": 1,
        "maxAmbientManagerCount": 1,
        "latencyProcessesStarted": 8,
        "inventoryResourceSamples": [
            {"inventorySize": size, "resources": resource()} for size in sizes
        ],
        "resolveBatchResourceSamples": [
            {"batch": batch, "resources": resource()} for batch in range(2)
        ],
        "preResolveResources": resource(),
        "barrierObservedResources": resource(),
        "postOverlapResources": resource(),
        "observedResourcePeak": resource(),
        "resourceAfter": resource(),
        "rssDeltaFromPreResolveBytes": -10,
    }


class SessionBarrierTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.resolve_root = self.root / "resolve-environments"
        self.prefix = self.resolve_root / "venv"
        self.prefix.mkdir(parents=True)
        self.barrier = self.root / "barrier"
        self.barrier.mkdir()
        (self.barrier / "release").write_text("release", encoding="ascii")

    def run_fixture(self, prefix, resolve_root):
        with patch.dict(os.environ, {
            "PET_SESSION_RESOLVE_BARRIER": str(self.barrier),
            "PET_SESSION_RESOLVE_ROOT": str(resolve_root),
        }), patch.object(sys, "prefix", str(prefix)):
            runpy.run_path(str(ROOT / "crates/pet/tests/fixtures/session_sitecustomize.py"))
        return (self.barrier / f"entered-{os.getpid()}").exists()

    def test_barrier_accepts_only_fixture_environment_prefixes(self):
        self.assertFalse(self.run_fixture(self.root / "ambient", self.resolve_root))
        self.assertFalse(self.run_fixture(self.root / "resolve-environments-other", self.resolve_root))
        self.assertTrue(self.run_fixture(self.prefix, self.resolve_root))
        self.assertTrue((self.barrier / f"released-{os.getpid()}").is_file())

    @unittest.skipIf(os.name == "nt", "creating directory symlinks requires Windows privileges")
    def test_barrier_matches_real_and_symlinked_temp_directory_spellings(self):
        alias = self.root / "alias"
        alias.symlink_to(self.resolve_root, target_is_directory=True)
        self.assertTrue(self.run_fixture(self.prefix, alias))
        (self.barrier / f"entered-{os.getpid()}").unlink()
        self.assertTrue(self.run_fixture(alias / "venv", self.resolve_root))


class SessionMetricsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.input = self.root / "output.txt"
        self.output = self.root / "metrics.json"

    def tearDown(self):
        self.temp.cleanup()

    def write_payloads(self, *payloads):
        self.input.write_text(
            "\n".join(f"test benchmark ... SESSION_METRICS {payload}" for payload in payloads),
            encoding="utf-8",
        )

    def artifact(self):
        return json.loads(self.output.read_text(encoding="utf-8"))

    def test_no_metrics_fails_closed_and_persists_failure(self):
        self.input.write_text("benchmark output only\n", encoding="utf-8")
        with self.assertRaisesRegex(MetricsError, "exactly one"):
            extract_metrics(self.input, self.output, 0, "fast")
        self.assertEqual(self.artifact()["status"], "failed")
        self.assertFalse(self.artifact()["metricsProduced"])

    def test_malformed_metrics_fails_closed(self):
        self.write_payloads("{not-json")
        with self.assertRaisesRegex(MetricsError, "not valid JSON"):
            extract_metrics(self.input, self.output, 0, "fast")
        self.assertEqual(self.artifact()["status"], "failed")

    def test_duplicate_metrics_fails_closed(self):
        payload = json.dumps(valid_metrics())
        self.write_payloads(payload, payload)
        with self.assertRaisesRegex(MetricsError, "found 2"):
            extract_metrics(self.input, self.output, 0, "fast")

    def test_count_mismatch_fails_closed(self):
        metrics = valid_metrics()
        metrics["measurementCounts"]["sameProcessWarm"] = 8
        self.write_payloads(json.dumps(metrics))
        with self.assertRaisesRegex(MetricsError, "measurementCounts"):
            extract_metrics(self.input, self.output, 0, "fast")

    def test_persistent_cache_control_requires_cache_hit_proof(self):
        for field, invalid in (
            (("persistentCacheAfterColdResolve", "bytes"), 0),
            (("persistentCacheResolveSamples", 1, "interpreterProcessesStarted"), 1),
            (("persistentCacheResolveSamples", 0, "interpreterProcessesStarted"), 0),
        ):
            with self.subTest(field=field):
                metrics = copy.deepcopy(valid_metrics())
                container = metrics
                for key in field[:-1]:
                    container = container[key]
                container[field[-1]] = invalid
                self.write_payloads(json.dumps(metrics))
                with self.assertRaises(MetricsError):
                    extract_metrics(self.input, self.output, 0, "fast")

    def test_inconsistent_mode_fails_closed(self):
        self.write_payloads(json.dumps(valid_metrics()))
        with self.assertRaisesRegex(MetricsError, "metrics.mode"):
            extract_metrics(self.input, self.output, 0, "stress")

    def test_failed_cargo_with_valid_metrics_persists_real_failed_metrics(self):
        self.write_payloads(json.dumps(valid_metrics()))
        metrics = extract_metrics(self.input, self.output, 101, "fast")
        self.assertEqual(metrics["status"], "failed")
        self.assertTrue(metrics["metricsProduced"])
        self.assertEqual(metrics["measurementCounts"]["sameProcessWarm"], 9)

    def test_success_preserves_validated_metrics(self):
        self.write_payloads(json.dumps(valid_metrics()))
        metrics = extract_metrics(self.input, self.output, 0, "fast")
        self.assertEqual(metrics["status"], "passed")
        self.assertTrue(metrics["metricsProduced"])
        self.assertEqual(len(metrics["refreshScenarioSamples"]), 9)

    def test_malformed_nested_types_fail_closed_with_failure_artifacts(self):
        fields = [
            ("sizes", 0), ("samplesPerSize",), ("resolveConcurrency",),
            ("coldResolveBatches",), ("refreshScenarioSamples", 0, "inventorySize"),
            ("refreshScenarioSamples", 0, "sampleCount"),
            ("refreshScenarioSamples", 0, "scenario"), ("cacheUsage", 0, "inventorySize"),
            ("persistentCacheResolveSamples", 0, "sampleCount"),
            ("persistentCacheResolveSamples", 0, "scenario"),
            ("persistentCacheAfterColdResolve", "files"),
            ("inventoryResourceSamples", 0, "inventorySize"),
            ("resolveBatchResourceSamples", 0, "batch"),
        ]
        for field in fields:
            for invalid in [True, False, 1.0, 3.0, 4.0, [], {}]:
                with self.subTest(field=field, invalid=invalid):
                    metrics = copy.deepcopy(valid_metrics())
                    container = metrics
                    for key in field[:-1]:
                        container = container[key]
                    container[field[-1]] = invalid
                    self.write_payloads(json.dumps(metrics))
                    with self.assertRaises(MetricsError):
                        extract_metrics(self.input, self.output, 0, "fast")
                    self.assertEqual(self.artifact()["status"], "failed")
                    self.assertFalse(self.artifact()["metricsProduced"])

    def test_timeout_fallback_replaces_corrupt_artifacts(self):
        for text in ['{', 'null', '{}', json.dumps(valid_metrics())]:
            with self.subTest(text=text):
                self.output.write_text(text, encoding="utf-8")
                ensure_failure_metrics(self.output, "fast", "interrupted")
                self.assertEqual(self.artifact(), failed_metrics("fast", "interrupted"))
        self.output.unlink()
        ensure_failure_metrics(self.output, "fast", "interrupted")
        self.assertEqual(self.artifact(), failed_metrics("fast", "interrupted"))

    def test_timeout_fallback_preserves_valid_success_and_failure_artifacts(self):
        for benchmark_status in [0, 101]:
            with self.subTest(benchmark_status=benchmark_status):
                self.write_payloads(json.dumps(valid_metrics()))
                extract_metrics(self.input, self.output, benchmark_status, "fast")
                before = self.output.read_bytes()
                ensure_failure_metrics(self.output, "fast", "interrupted")
                self.assertEqual(self.output.read_bytes(), before)
        failure = failed_metrics("fast", "original failure")
        write_json(self.output, failure)
        ensure_failure_metrics(self.output, "fast", "interrupted")
        self.assertEqual(self.artifact(), failure)

    def test_atomic_write_preserves_previous_artifact_if_replace_fails(self):
        write_json(self.output, failed_metrics("fast", "original failure"))
        before = self.output.read_bytes()
        with patch("session_metrics.os.replace", side_effect=OSError("replace failed")):
            with self.assertRaisesRegex(OSError, "replace failed"):
                write_json(self.output, failed_metrics("fast", "new failure"))
        self.assertEqual(self.output.read_bytes(), before)
        self.assertEqual(list(self.root.glob(".metrics.json.*")), [])

    def test_workflow_uses_validator_and_timeout_fallback(self):
        workflow = (ROOT / ".github/workflows/session-benchmarks.yml").read_text(
            encoding="utf-8"
        )
        self.assertIn("python -B scripts/session_metrics.py extract", workflow)
        self.assertIn("python -B scripts/session_metrics.py ensure-failure", workflow)
        self.assertIn("if: always()", workflow)


if __name__ == "__main__":
    unittest.main()
