#!/usr/bin/env python3
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.
"""Wait for an exact-commit GitHub Actions baseline artifact."""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from dataclasses import dataclass
from typing import Any, Callable, Protocol, Sequence
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen


class BaselineError(RuntimeError):
    """Raised when an exact-base workflow or artifact cannot be used."""


@dataclass(frozen=True)
class BaselineArtifact:
    run_id: int
    artifact_id: int


class BaselineClient(Protocol):
    def workflow_runs(
        self, repository: str, workflow: str, commit: str
    ) -> list[dict[str, Any]]:
        ...

    def artifacts(self, repository: str, run_id: int) -> list[dict[str, Any]]:
        ...


class GitHubClient:
    def __init__(self, token: str, api_url: str = 'https://api.github.com') -> None:
        if not token:
            raise BaselineError('GITHUB_TOKEN is required')
        self._token = token
        self._api_url = api_url.rstrip('/')

    def _get_json(self, path: str, query: dict[str, Any] | None = None) -> dict[str, Any]:
        url = f'{self._api_url}{path}'
        if query:
            url = f'{url}?{urlencode(query)}'
        request = Request(
            url,
            headers={
                'Accept': 'application/vnd.github+json',
                'Authorization': f'Bearer {self._token}',
                'User-Agent': 'python-environment-tools-quality-snapshots',
                'X-GitHub-Api-Version': '2022-11-28',
            },
        )
        try:
            with urlopen(request, timeout=30) as response:
                raw = response.read()
        except HTTPError as error:
            details = error.read().decode('utf-8', errors='replace').strip()
            raise BaselineError(
                f'GitHub API request failed with HTTP {error.code} for {url}: {details}'
            ) from error
        except URLError as error:
            raise BaselineError(f'GitHub API request failed for {url}: {error.reason}') from error
        try:
            value = json.loads(raw)
        except json.JSONDecodeError as error:
            raise BaselineError(f'GitHub API returned invalid JSON for {url}: {error}') from error
        if not isinstance(value, dict):
            raise BaselineError(f'GitHub API response for {url} must be an object')
        return value

    def workflow_runs(
        self, repository: str, workflow: str, commit: str
    ) -> list[dict[str, Any]]:
        repository_path = quote_repository(repository)
        workflow_path = quote(workflow, safe='')
        response = self._get_json(
            f'/repos/{repository_path}/actions/workflows/{workflow_path}/runs',
            {'event': 'push', 'head_sha': commit, 'per_page': 100},
        )
        runs = response.get('workflow_runs')
        if not isinstance(runs, list) or not all(isinstance(run, dict) for run in runs):
            raise BaselineError('GitHub workflow-runs response must contain a workflow_runs array')
        return runs

    def artifacts(self, repository: str, run_id: int) -> list[dict[str, Any]]:
        repository_path = quote_repository(repository)
        response = self._get_json(
            f'/repos/{repository_path}/actions/runs/{run_id}/artifacts',
            {'per_page': 100},
        )
        artifacts = response.get('artifacts')
        if not isinstance(artifacts, list) or not all(
            isinstance(artifact, dict) for artifact in artifacts
        ):
            raise BaselineError('GitHub artifacts response must contain an artifacts array')
        return artifacts


def quote_repository(repository: str) -> str:
    parts = repository.split('/')
    if len(parts) != 2 or not all(parts):
        raise BaselineError(f'Repository must have owner/name form: {repository!r}')
    return '/'.join(quote(part, safe='') for part in parts)


def latest_push_run(
    runs: Sequence[dict[str, Any]], commit: str
) -> dict[str, Any] | None:
    matching = [
        run
        for run in runs
        if run.get('head_sha') == commit and run.get('event') == 'push'
    ]
    if not matching:
        return None
    return max(
        matching,
        key=lambda run: (
            run.get('run_number') if isinstance(run.get('run_number'), int) else 0,
            run.get('run_attempt') if isinstance(run.get('run_attempt'), int) else 0,
            run.get('id') if isinstance(run.get('id'), int) else 0,
        ),
    )


def matching_artifact(
    artifacts: Sequence[dict[str, Any]], artifact_name: str
) -> dict[str, Any] | None:
    return next(
        (
            artifact
            for artifact in artifacts
            if artifact.get('name') == artifact_name and artifact.get('expired') is False
        ),
        None,
    )


def wait_for_baseline(
    client: BaselineClient,
    repository: str,
    workflow: str,
    commit: str,
    artifact_name: str,
    *,
    timeout_seconds: float,
    poll_seconds: float,
    monotonic: Callable[[], float] = time.monotonic,
    sleep: Callable[[float], None] = time.sleep,
    log: Callable[[str], None] = print,
) -> BaselineArtifact:
    if timeout_seconds <= 0:
        raise BaselineError('Timeout must be greater than zero')
    if poll_seconds <= 0:
        raise BaselineError('Poll interval must be greater than zero')

    deadline = monotonic() + timeout_seconds
    successful_run_without_artifact: int | None = None
    while True:
        run = latest_push_run(client.workflow_runs(repository, workflow, commit), commit)
        if run is None:
            log(f'Waiting for {workflow} at {commit}: no push run found')
        else:
            run_id = run.get('id')
            status = run.get('status')
            if not isinstance(run_id, int) or not isinstance(status, str):
                raise BaselineError('Workflow run must contain integer id and string status')
            if status == 'completed':
                conclusion = run.get('conclusion')
                if conclusion != 'success':
                    raise BaselineError(
                        f'Exact-base workflow {workflow} run {run_id} for {commit} '
                        f'completed with conclusion {conclusion!r}'
                    )
                artifact = matching_artifact(client.artifacts(repository, run_id), artifact_name)
                if artifact is not None:
                    artifact_id = artifact.get('id')
                    if not isinstance(artifact_id, int):
                        raise BaselineError('Artifact must contain an integer id')
                    log(
                        f'Found exact-base artifact {artifact_name} in {workflow} '
                        f'run {run_id} for {commit}'
                    )
                    return BaselineArtifact(run_id, artifact_id)
                successful_run_without_artifact = run_id
                log(
                    f'Waiting for artifact {artifact_name}: successful {workflow} '
                    f'run {run_id} has not published it'
                )
            else:
                log(f'Waiting for {workflow} run {run_id} at {commit}: status={status}')

        remaining = deadline - monotonic()
        if remaining <= 0:
            if successful_run_without_artifact is not None:
                raise BaselineError(
                    f'Timed out after {timeout_seconds:g}s waiting for artifact '
                    f'{artifact_name} from successful {workflow} run '
                    f'{successful_run_without_artifact} at {commit}'
                )
            raise BaselineError(
                f'Timed out after {timeout_seconds:g}s waiting for exact-base '
                f'workflow {workflow} at {commit}'
            )
        sleep(min(poll_seconds, remaining))


def create_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repository', required=True)
    parser.add_argument('--workflow', required=True)
    parser.add_argument('--commit', required=True)
    parser.add_argument('--artifact', required=True)
    parser.add_argument('--timeout-seconds', type=float, default=1_200)
    parser.add_argument('--poll-seconds', type=float, default=20)
    parser.add_argument(
        '--api-url',
        default=os.environ.get('GITHUB_API_URL', 'https://api.github.com'),
    )
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = create_parser().parse_args(argv)
    try:
        client = GitHubClient(os.environ.get('GITHUB_TOKEN', ''), args.api_url)
        wait_for_baseline(
            client,
            args.repository,
            args.workflow,
            args.commit,
            args.artifact,
            timeout_seconds=args.timeout_seconds,
            poll_seconds=args.poll_seconds,
        )
    except BaselineError as error:
        print(f'error: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
