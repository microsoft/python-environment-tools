#!/usr/bin/env python3
# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.
"""Supplement raw LCOV gates with source-classified lines and subprocess proof."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

from quality_snapshot import SnapshotError, parse_lcov


def run(*args: str, cwd: Path) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True, encoding='utf-8')


def relative_source(value: str) -> str:
    parts = value.replace('\\', '/').split('/')
    try:
        index = len(parts) - 1 - parts[::-1].index('crates')
    except ValueError as error:
        raise SnapshotError(f'Non-workspace LCOV source: {value}') from error
    parts = parts[index:]
    if any(part in {'', '.', '..'} for part in parts) or not parts[-1].endswith('.rs'):
        raise SnapshotError(f'Invalid workspace LCOV source: {value}')
    return '/'.join(parts)


@dataclass
class SourceCoverage:
    lines: dict[int, int]
    unmapped: int = 0
    unmapped_hits: int = 0


def line_records(path: Path) -> dict[str, SourceCoverage]:
    parse_lcov(path)
    records: dict[str, SourceCoverage] = {}
    name = None
    summaries: dict[str, int] = {}
    for text in path.read_text(encoding='utf-8').splitlines():
        if text.startswith('SF:'):
            if name is not None:
                raise SnapshotError('LCOV record missing terminator')
            name = relative_source(text[3:])
            if name in records:
                raise SnapshotError(f'Duplicate LCOV source: {name}')
            records[name] = SourceCoverage({})
            summaries = {}
        elif text.startswith('DA:'):
            if name is None:
                raise SnapshotError('LCOV line has no source')
            fields = text[3:].split(',')
            if len(fields) not in (2, 3):
                raise SnapshotError('Malformed LCOV line')
            number, hits = map(int, fields[:2])
            if number <= 0 or hits < 0 or number in records[name].lines:
                raise SnapshotError('Invalid or duplicate LCOV line')
            records[name].lines[number] = hits
        elif text.startswith(('LF:', 'LH:')):
            key, value = text.split(':', 1)
            if name is None or key in summaries or int(value) < 0:
                raise SnapshotError('Missing source or invalid/duplicate line summary')
            summaries[key] = int(value)
        elif text == 'end_of_record':
            if name is None or summaries.keys() != {'LF', 'LH'}:
                raise SnapshotError('LCOV source missing line summaries')
            row = records[name]
            row.unmapped = summaries['LF'] - len(row.lines)
            row.unmapped_hits = summaries['LH'] - sum(count > 0 for count in row.lines.values())
            if row.unmapped < 0 or not 0 <= row.unmapped_hits <= row.unmapped:
                raise SnapshotError(f'LCOV line records disagree with summaries: {name}')
            # LLVM summaries can count more entries than its unique DA source lines.
            # Unlocated entries remain conservatively uncovered in the source-focused report.
            name = None
    if name is not None or not records or not any(row.lines or row.unmapped for row in records.values()):
        raise SnapshotError('LCOV source/line records are incomplete')
    return records


def code_mask(source: str) -> str:
    # Mask Rust literals/comments without moving line or character positions.
    chars = list(source)
    i = 0
    while i < len(source):
        end = i
        if source.startswith('//', i):
            end = source.find('\n', i)
            if end == -1:
                end = len(source)
        elif source.startswith('/*', i):
            depth, end = 1, i + 2
            while depth and end < len(source):
                if source.startswith('/*', end):
                    depth += 1
                    end += 2
                elif source.startswith('*/', end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                raise SnapshotError('Unterminated Rust block comment')
        else:
            raw = re.match(r'(?:br|cr|r)(#*)"', source[i:])
            if raw:
                close = '"' + raw[1]
                end = source.find(close, i + raw.end())
                if end == -1:
                    raise SnapshotError('Unterminated Rust raw string')
                end += len(close)
            elif source[i] == '"':
                end = i + 1
                while end < len(source):
                    if source[end] == chr(92):
                        end += 2
                    elif source[end] == '"':
                        end += 1
                        break
                    else:
                        end += 1
                else:
                    raise SnapshotError('Unterminated Rust string')
            elif source[i] == "'":
                char = re.match(r"'(?:[^'\\\n]|\\(?:u\{[0-9a-fA-F_]+\}|x[0-9a-fA-F]{2}|.))'", source[i:])
                if char:
                    end = i + char.end()
        if end > i:
            for n in range(i, end):
                if chars[n] != '\n':
                    chars[n] = ' '
            i = end
        else:
            i += 1
    return ''.join(chars)


def test_lines(source: str) -> set[int]:
    masked = code_mask(source)
    excluded: set[int] = set()
    for match in re.finditer(r'#\s*\[\s*(?:cfg\s*\(\s*test\s*\)|test)\s*\]', masked):
        start = match.start()
        end = match.end()
        # Other attributes belong to this same item, not its body.
        while True:
            attr = re.match(r'\s*#\s*\[', masked[end:])
            if not attr:
                break
            end += attr.end()
            depth = 1
            while depth and end < len(masked):
                depth += (masked[end] == '[') - (masked[end] == ']')
                end += 1
            if depth:
                raise SnapshotError('Unterminated Rust attribute')
        delimiters: list[str] = []
        closing = {')': '(', ']': '[', '>': '<', '}': '{'}
        initializer = False
        while end < len(masked):
            char = masked[end]
            end += 1
            if char == '>' and end >= 2 and masked[end - 2] == '-':
                continue
            if not delimiters:
                if char == '=':
                    initializer = True
                if char == ';' or (char == '{' and not initializer):
                    break
            type_context = not initializer and not any(c in '[{' for c in delimiters)
            if char in '([{' or (char == '<' and type_context):
                delimiters.append(char)
            elif char in closing and delimiters and delimiters[-1] == closing[char]:
                delimiters.pop()
        else:
            raise SnapshotError('Test attribute has no item')
        if masked[end - 1] == '{':
            depth = 1
            while depth and end < len(masked):
                depth += (masked[end] == '{') - (masked[end] == '}')
                end += 1
            if depth:
                raise SnapshotError('Unterminated test item')
        excluded.update(range(source.count('\n', 0, start) + 1, source.count('\n', 0, end) + 2))
    return excluded


def changed_lines(root: Path, base: str) -> dict[str, set[int]]:
    text = run('git', 'diff', '--no-ext-diff', '--no-renames', '--unified=0', base,
               '--', '*.rs', cwd=root)
    changed: dict[str, set[int]] = {}
    name = None
    for line in text.splitlines():
        if line.startswith('+++ b/'):
            name = line[6:]
            changed.setdefault(name, set())
        elif line.startswith('+++ '):
            name = None
        elif line.startswith('@@ '):
            hunk = re.match(r'@@ -[0-9]+(?:,[0-9]+)? \+([0-9]+)(?:,([0-9]+))? @@', line)
            if not hunk or name is None:
                continue
            start, length = int(hunk[1]), int(hunk[2] or '1')
            changed[name].update(range(start, start + length))
    return changed


def summarize(root: Path, records: dict[str, SourceCoverage], changed: dict[str, set[int]]) -> dict:
    files = []
    for name, record in sorted(records.items()):
        lines = record.lines
        source = (root / name).read_text(encoding='utf-8')
        if any(n > len(source.splitlines()) for n in lines):
            raise SnapshotError(f'LCOV source revision mismatch: {name}')
        parts = Path(name).parts
        excluded = set(lines) if 'tests' in parts or 'benches' in parts else test_lines(source)
        production = {n: count for n, count in lines.items() if n not in excluded}
        tests = {n: count for n, count in lines.items() if n in excluded}
        edits = production.keys() & changed.get(name, set())
        files.append({
            'path': name, 'production_found': len(production) + record.unmapped,
            'unmapped_summary_lines': record.unmapped,
            'unmapped_summary_hits': record.unmapped_hits,
            'changed_lines_without_line_records': sorted(changed.get(name, set()) - lines.keys() - excluded),
            'production_hit': sum(n > 0 for n in production.values()),
            'test_found': len(tests), 'test_hit': sum(n > 0 for n in tests.values()),
            'uncovered_production': sorted(n for n, hits in production.items() if not hits),
            'changed_production_found': len(edits),
            'uncovered_changed_production': sorted(n for n in edits if not production[n]),
        })
    return {'schema_version': 1, 'files': files,
            'changed_files_without_instrumentation': sorted(name for name, lines in changed.items()
                                                           if lines and name not in records)}


def branch_counts(path: Path) -> tuple[int, int]:
    found = hit = 0
    for line in path.read_text(encoding='utf-8').splitlines():
        if not line.startswith('BRDA:'):
            continue
        fields = line[5:].split(',')
        if len(fields) != 4:
            raise SnapshotError('Malformed LCOV branch record')
        count = 0 if fields[3] == '-' else int(fields[3])
        if count < 0:
            raise SnapshotError('Negative branch count')
        found += 1
        hit += count > 0
    return hit, found


def details_report(data: dict) -> str:
    files = data['files']
    totals = {key: sum(f[key] for f in files) for key in
              ('production_hit', 'production_found', 'test_hit', 'test_found', 'changed_production_found')}
    uncovered = sum(len(f['uncovered_changed_production']) for f in files)
    lines = ['## Production-focused coverage', '',
             'Supplemental schema 1; the whole-workspace exact-base line/function gate is unchanged.', '',
             f"Production lines: {totals['production_hit']}/{totals['production_found']}; "
             f"test lines: {totals['test_hit']}/{totals['test_found']}.",
             f"Changed executable production lines: {totals['changed_production_found'] - uncovered}/"
             f"{totals['changed_production_found']} covered.", '',
             '| Source | Production hit/total | Test hit/total | Unmapped summary lines | Uncovered changed production lines |',
             '| --- | ---: | ---: | ---: | --- |']
    for f in files:
        missing = ', '.join(map(str, f['uncovered_changed_production'])) or '-'
        lines.append(f"| {f['path']} | {f['production_hit']}/{f['production_found']} | "
                     f"{f['test_hit']}/{f['test_found']} | {f['unmapped_summary_lines']} | {missing} |")
    lines += ['', 'Unmapped LF summary entries are conservatively counted as uncovered production.',
              'Changed lines lacking DA records (including non-executable syntax) are retained in details.json;',
              'they are not assumed covered.', '',
              'Changed Rust files without instrumentation (not assumed covered):']
    lines += [f'- {name}' for name in data['changed_files_without_instrumentation']] or ['- None']
    hit, found = data['branches']
    lines += ['', f'LCOV branches (whole workspace): {hit}/{found}.' if found else
              'Branch outcomes: unavailable from this stable Rust instrumentation; not reported as 100%.',
              'Line hits do not prove both outcomes of a condition. See QUALITY_SNAPSHOTS.md for limits.', '']
    return '\n'.join(lines)


def verify_proof(root: Path, manifest: Path, output: Path) -> None:
    proof = json.loads(manifest.read_text(encoding='utf-8'))
    sysroot = Path(run('rustc', '--print', 'sysroot', cwd=root).strip())
    host = next(line[6:] for line in run('rustc', '-vV', cwd=root).splitlines() if line.startswith('host: '))
    tools = sysroot / 'lib' / 'rustlib' / host / 'bin'
    suffix = '.exe' if sys.platform == 'win32' else ''
    output.mkdir(parents=True, exist_ok=True)
    witnesses = (
        ('handler', 'crates/pet/src/jsonrpc.rs', 'pub fn handle_info('),
        ('transport', 'crates/pet-jsonrpc/src/server.rs', 'if handle_payload(handlers, &payload).is_err() {'),
        ('writer', 'crates/pet-jsonrpc/src/output.rs', 'if let Err(error) = writer.write_all(&frame) {'),
    )
    exports = {}
    for label in ('idle', 'info'):
        paths = proof['profiles'][label]
        if not paths or any(not Path(p).is_file() or Path(p).stat().st_size == 0 for p in paths):
            raise SnapshotError(f'Missing or empty {label} subprocess profiles')
        merged = output / f'{label}.profdata'
        run(str(tools / f'llvm-profdata{suffix}'), 'merge', '-sparse', *paths, '-o', str(merged), cwd=root)
        text = run(str(tools / f'llvm-cov{suffix}'), 'export', proof['binary'],
                   *(str(root / name) for _, name, _ in witnesses),
                   f'-instr-profile={merged}', '-format=lcov', cwd=root)
        lcov = output / f'{label}.info'
        lcov.write_text(text, encoding='utf-8')
        exports[label] = line_records(lcov)
    evidence = {}
    for label, name, marker in witnesses:
        locations = [i for i, line in enumerate((root / name).read_text(encoding='utf-8').splitlines(), 1)
                     if marker in line]
        if len(locations) != 1:
            raise SnapshotError(f'Coverage witness changed: {name}: {marker}')
        line = locations[0]
        before, after = [exports[k].get(name, SourceCoverage({})).lines.get(line) for k in ('idle', 'info')]
        if before != 0 or after is None or after <= 0:
            raise SnapshotError(f'{label} not proved by isolated info request: idle={before}, info={after}')
        evidence[label] = {'source': name, 'line': line, 'idle_hits': before, 'info_hits': after}
    (output / 'proof.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path.cwd())
    sub = parser.add_subparsers(dest='command', required=True)
    report = sub.add_parser('report')
    report.add_argument('--lcov', type=Path, required=True)
    report.add_argument('--base', required=True)
    report.add_argument('--output', type=Path, required=True)
    proof = sub.add_parser('proof')
    proof.add_argument('--manifest', type=Path, required=True)
    proof.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == 'proof':
            verify_proof(args.root, args.manifest, args.output)
        else:
            base = run('git', 'rev-parse', '--verify', f'{args.base}^{{commit}}', cwd=args.root).strip()
            data = summarize(args.root, line_records(args.lcov), changed_lines(args.root, base))
            data.update(base_commit=base, branches=branch_counts(args.lcov))
            args.output.mkdir(parents=True, exist_ok=True)
            (args.output / 'details.json').write_text(json.dumps(data, indent=2) + '\n', encoding='utf-8')
            (args.output / 'report.md').write_text(details_report(data), encoding='utf-8')
    except (OSError, ValueError, KeyError, StopIteration, subprocess.CalledProcessError) as error:
        print(f'Coverage evidence failed: {error}', file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
