# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from wait_for_baseline import (  # noqa: E402
    BaselineArtifact,
    BaselineError,
    latest_push_run,
    quote_repository,
    wait_for_baseline,
)


def workflow_run(
    *,
    run_id=7,
    status='in_progress',
    conclusion=None,
    commit='base-sha',
    event='push',
    run_number=1,
    run_attempt=1,
):
    return {
        'id': run_id,
        'status': status,
        'conclusion': conclusion,
        'head_sha': commit,
        'event': event,
        'run_number': run_number,
        'run_attempt': run_attempt,
    }


def artifact(*, artifact_id=11, name='baseline-linux', expired=False):
    return {'id': artifact_id, 'name': name, 'expired': expired}


class FakeClock:
    def __init__(self):
        self.now = 0.0
        self.sleeps = []

    def monotonic(self):
        return self.now

    def sleep(self, seconds):
        self.sleeps.append(seconds)
        self.now += seconds


class FakeClient:
    def __init__(self, run_responses, artifact_responses=None):
        self.run_responses = list(run_responses)
        self.artifact_responses = list(artifact_responses or [[]])
        self.run_requests = []
        self.artifact_requests = []

    @staticmethod
    def _next(responses):
        if len(responses) > 1:
            return responses.pop(0)
        return responses[0]

    def workflow_runs(self, repository, workflow, commit):
        self.run_requests.append((repository, workflow, commit))
        return self._next(self.run_responses)

    def artifacts(self, repository, run_id):
        self.artifact_requests.append((repository, run_id))
        return self._next(self.artifact_responses)


class WaitForBaselineTests(unittest.TestCase):
    def wait(self, client, *, timeout=20, poll=5):
        clock = FakeClock()
        logs = []
        result = wait_for_baseline(
            client,
            'microsoft/python-environment-tools',
            'perf-baseline.yml',
            'base-sha',
            'baseline-linux',
            timeout_seconds=timeout,
            poll_seconds=poll,
            monotonic=clock.monotonic,
            sleep=clock.sleep,
            log=logs.append,
        )
        return result, clock, logs

    def test_pending_workflow_is_polled_until_artifact_exists(self):
        client = FakeClient(
            [
                [workflow_run(status='queued')],
                [workflow_run(status='in_progress')],
                [workflow_run(status='completed', conclusion='success')],
            ],
            [[artifact()]],
        )

        result, clock, logs = self.wait(client)

        self.assertEqual(result, BaselineArtifact(run_id=7, artifact_id=11))
        self.assertEqual(clock.sleeps, [5, 5])
        self.assertIn('status=queued', logs[0])
        self.assertIn('Found exact-base artifact', logs[-1])

    def test_failed_exact_base_workflow_fails_immediately(self):
        client = FakeClient(
            [[workflow_run(status='completed', conclusion='failure')]]
        )

        with self.assertRaisesRegex(BaselineError, "conclusion 'failure'"):
            self.wait(client)

        self.assertEqual(client.artifact_requests, [])

    def test_missing_workflow_times_out_at_bound(self):
        client = FakeClient([[]])
        clock = FakeClock()

        with self.assertRaisesRegex(BaselineError, 'Timed out after 10s'):
            wait_for_baseline(
                client,
                'microsoft/python-environment-tools',
                'perf-baseline.yml',
                'base-sha',
                'baseline-linux',
                timeout_seconds=10,
                poll_seconds=4,
                monotonic=clock.monotonic,
                sleep=clock.sleep,
                log=lambda _: None,
            )

        self.assertEqual(clock.now, 10)

    def test_successful_workflow_missing_artifact_times_out_clearly(self):
        client = FakeClient(
            [[workflow_run(status='completed', conclusion='success')]],
            [[]],
        )

        with self.assertRaisesRegex(
            BaselineError,
            'waiting for artifact baseline-linux from successful perf-baseline.yml run 7',
        ):
            self.wait(client, timeout=10, poll=5)

    def test_wrong_or_expired_artifacts_are_ignored_while_polling(self):
        client = FakeClient(
            [[workflow_run(status='completed', conclusion='success')]],
            [
                [
                    artifact(name='other'),
                    artifact(name='baseline-linux', expired=True),
                ],
                [artifact(artifact_id=12)],
            ],
        )

        result, clock, _ = self.wait(client)

        self.assertEqual(result, BaselineArtifact(run_id=7, artifact_id=12))
        self.assertEqual(clock.sleeps, [5])

    def test_latest_exact_push_run_is_selected(self):
        selected = latest_push_run(
            [
                workflow_run(run_id=1, commit='other-sha', run_number=99),
                workflow_run(run_id=2, event='workflow_dispatch', run_number=10),
                workflow_run(run_id=3, run_number=1),
                workflow_run(run_id=4, run_number=2, run_attempt=1),
                workflow_run(run_id=5, run_number=2, run_attempt=2),
            ],
            'base-sha',
        )

        self.assertEqual(selected['id'], 5)

    def test_repository_requires_owner_and_name(self):
        self.assertEqual(
            quote_repository('microsoft/python-environment-tools'),
            'microsoft/python-environment-tools',
        )
        with self.assertRaisesRegex(BaselineError, 'owner/name'):
            quote_repository('python-environment-tools')

    def test_non_positive_intervals_are_rejected(self):
        client = FakeClient([[]])
        with self.assertRaisesRegex(BaselineError, 'Timeout'):
            self.wait(client, timeout=0)
        with self.assertRaisesRegex(BaselineError, 'Poll interval'):
            self.wait(client, poll=0)


if __name__ == '__main__':
    unittest.main()
