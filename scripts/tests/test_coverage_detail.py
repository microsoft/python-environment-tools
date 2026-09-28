# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import coverage_detail as detail


class DetailCoverageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def lcov(self, text):
        path = self.root / 'lcov.info'
        path.write_text(text, encoding='utf-8')
        return path

    def record(self, name='crates/pet/src/lib.rs', lines='DA:1,1\nDA:2,0\n'):
        entries = [line for line in lines.splitlines() if line.startswith('DA:')]
        hits = sum(int(line.split(',')[1]) > 0 for line in entries)
        return (f'SF:C:\\checkout\\{name}\n{lines}LF:{len(entries)}\nLH:{hits}\n'
                'FNF:1\nFNH:1\nend_of_record\n')

    def test_absolute_platform_paths_share_relative_identity(self):
        for name in ['C:\\checkout\\crates\\pet\\src\\lib.rs', '/tmp/base/crates/pet/src/lib.rs', '/tmp/crates/checkout/crates/pet/src/lib.rs']:
            self.assertEqual(detail.relative_source(name), 'crates/pet/src/lib.rs')
        for name in ['/tmp/other.rs', '/tmp/crates/pet/../secret.rs']:
            with self.assertRaises(detail.SnapshotError):
                detail.relative_source(name)

    def test_lcov_rejects_missing_empty_malformed_and_duplicate_records(self):
        for text in ['', self.record(lines='DA:0,1\n'), self.record(lines='DA:1,-1\n'),
                     self.record(lines='DA:1,1\nDA:1,2\n'), self.record() * 2,
                     self.record().replace('end_of_record\n', '')]:
            with self.subTest(text=text), self.assertRaises((ValueError, detail.SnapshotError)):
                detail.line_records(self.lcov(text))
        self.assertEqual(detail.line_records(self.lcov(self.record())),
                         {'crates/pet/src/lib.rs': detail.SourceCoverage({1: 1, 2: 0})})

    def test_lcov_line_summaries_cannot_hide_truncated_or_overcounted_data(self):
        valid = self.record()
        for text in [valid.replace('LH:1', 'LH:0'),
                     valid.replace('LF:2', 'LF:1'),
                     valid.replace('LF:2\n', ''),
                     valid.replace('LH:1\n', ''),
                     valid.replace('LF:2', 'LF:2\nLF:2')]:
            with self.subTest(text=text), self.assertRaises(detail.SnapshotError):
                detail.line_records(self.lcov(text))

    def test_test_item_types_do_not_terminate_at_nested_semicolons_or_braces(self):
        for signature in ['fn helper() -> [u8; 4]',
                          'fn helper<const N: usize>() -> [u8; { 2 + 2 }]',
                          'fn helper() -> Buffer<{ 2 + 2 }>',
                          'fn helper(arg: [u8; 4])']:
            source = '#[cfg(test)]\n' + signature + ' {\n let value = 4;\n}\nfn real() {}\n'
            with self.subTest(signature=signature):
                self.assertEqual(detail.test_lines(source), {1, 2, 3, 4})

    def test_unmapped_llvm_summary_lines_remain_uncovered_production(self):
        name = 'crates/pet/src/lib.rs'
        path = self.root / name
        path.parent.mkdir(parents=True)
        path.write_text('fn real() {}\nfn missing() {}\n')
        records = detail.line_records(self.lcov(self.record().replace('DA:2,0\n', '')))
        self.assertEqual(records[name].unmapped, 1)
        result = detail.summarize(self.root, records, {name: {2}})['files'][0]
        self.assertEqual(result['production_found'], 2)
        self.assertEqual(result['production_hit'], 1)
        self.assertEqual(result['unmapped_summary_lines'], 1)
        self.assertEqual(result['changed_lines_without_line_records'], [2])
        records = detail.line_records(self.lcov(self.record().replace('DA:1,1\n', '')))
        self.assertEqual(records[name].unmapped_hits, 1)
        result = detail.summarize(self.root, records, {name: {1}})['files'][0]
        self.assertEqual(result['production_found'], 2)
        self.assertEqual(result['production_hit'], 0)
        self.assertEqual(result['changed_lines_without_line_records'], [1])

    def test_comparison_and_shift_operators_are_not_generic_type_delimiters(self):
        for declaration in ['const LESS: bool = 1 < 2;',
                            'static SHIFT: usize = 1 << 2;',
                            'const BLOCK: bool = { 1 < 2 };',
                            'type Array = [bool; { (1 < 2) as usize }];',
                            'fn helper() -> Buffer<{ (1 < 2) as usize }> {}',
                            'fn helper<F: Fn() -> bool>() {}']:
            source = '#[cfg(test)]\n' + declaration + '\nfn real() {}\n'
            with self.subTest(declaration=declaration):
                self.assertEqual(detail.test_lines(source), {1, 2})

    def test_inline_tests_do_not_hide_later_production_items(self):
        source = 'fn before() {}\n#[cfg(test)]\nmod tests {\n fn check() {}\n}\nfn after() {}\n'
        self.assertEqual(detail.test_lines(source), {2, 3, 4, 5})

    def test_literals_nested_comments_and_attributes_do_not_move_boundaries(self):
        source = ('// #[cfg(test)] fake {\n'
                  'const TEXT: &str = r###"#[cfg(test)] }"###;\n'
                  '#[cfg(test)]\n#[allow(dead_code)]\nmod tests {\n'
                  ' /* outer { /* nested } */ } */\n'
                  ' let ch = \'{\'; let escaped = "}\\"{";\n'
                  ' let raw = br##"{{}}"##;\n}\nfn after() {}\n')
        self.assertEqual(detail.test_lines(source), set(range(3, 10)))
        self.assertEqual(detail.code_mask(source).count('\n'), source.count('\n'))

    def test_cfg_test_function_and_external_module_are_test_code(self):
        source = '#[cfg(test)]\nfn helper() {}\n#[cfg(test)]\nmod fixtures;\nfn real() {}\n'
        self.assertEqual(detail.test_lines(source), {1, 2, 3, 4})
        self.assertEqual(detail.test_lines("fn real<'a>(x: &'a str) { let c = '\\u{7b}'; }\n"), set())

    def test_unterminated_source_is_an_error_not_a_smaller_denominator(self):
        for source in ['/* never closed', 'let raw = r#"never closed',
                       '#[cfg(test)] mod tests {', '#[cfg(test)]']:
            with self.subTest(source=source), self.assertRaises(detail.SnapshotError):
                detail.test_lines(source)

    def test_reports_uncovered_changed_production_and_all_test_lines(self):
        name = 'crates/pet/src/lib.rs'
        path = self.root / name
        path.parent.mkdir(parents=True)
        path.write_text('fn real() {}\n#[cfg(test)]\nmod tests {\n fn test() {}\n}\nfn later() {}\n')
        integration = 'crates/pet/tests/native.rs'
        target = self.root / integration
        target.parent.mkdir()
        target.write_text('fn native() {}\n')
        data = detail.summarize(self.root, {name: detail.SourceCoverage({1: 2, 4: 1, 6: 0}),
                                            integration: detail.SourceCoverage({1: 1})},
                                {name: {1, 4, 6}, 'crates/pet/src/unmeasured.rs': {1}})
        row = next(f for f in data['files'] if f['path'] == name)
        self.assertEqual((row['production_hit'], row['production_found']), (1, 2))
        self.assertEqual((row['test_hit'], row['test_found']), (1, 1))
        self.assertEqual(row['uncovered_changed_production'], [6])
        self.assertEqual(row['changed_production_found'], 2)
        self.assertEqual(data['changed_files_without_instrumentation'], ['crates/pet/src/unmeasured.rs'])
        data['branches'] = (0, 0)
        report = detail.details_report(data)
        self.assertIn('not reported as 100%', report)
        self.assertIn('1/2 covered', report)
        with self.assertRaises(detail.SnapshotError):
            detail.summarize(self.root, {name: detail.SourceCoverage({100: 1})}, {})

    def test_changed_line_hunks_handle_add_delete_and_rename_without_count_substitution(self):
        diff = ('+++ b/crates/pet/src/lib.rs\n@@ -1,0 +2,2 @@\n+x\n+y\n'
                '@@ -8 +10 @@\n-x\n+y\n@@ -15,2 +17,0 @@\n-x\n-y\n'
                '+++ /dev/null\n@@ -1,4 +0,0 @@\n')
        with patch.object(detail, 'run', return_value=diff) as command:
            self.assertEqual(detail.changed_lines(self.root, 'exact-base'),
                             {'crates/pet/src/lib.rs': {2, 3, 10}})
        self.assertIn('--no-renames', command.call_args.args)
        self.assertIn('exact-base', command.call_args.args)

    def test_branch_records_are_optional_but_invalid_values_fail(self):
        self.assertEqual(detail.branch_counts(self.lcov(self.record())), (0, 0))
        self.assertEqual(detail.branch_counts(self.lcov('BRDA:1,0,0,2\nBRDA:1,0,1,-\n')), (1, 2))
        with self.assertRaises(detail.SnapshotError):
            detail.branch_counts(self.lcov('BRDA:1,0,0,-2\n'))

    def test_missing_exact_child_profiles_fail_before_export(self):
        manifest = self.root / 'proof.json'
        manifest.write_text(json.dumps({'binary': 'pet', 'profiles': {'idle': ['absent']}}))
        with patch.object(detail, 'run', side_effect=['/toolchain', 'host: unit-test']), self.assertRaises(detail.SnapshotError):
            detail.verify_proof(self.root, manifest, self.root / 'proof')

    def test_info_proof_requires_all_three_positive_differential_witnesses(self):
        witnesses = [('crates/pet/src/jsonrpc.rs', 'pub fn handle_info('),
                     ('crates/pet-jsonrpc/src/server.rs', 'if handle_payload(handlers, &payload).is_err() {'),
                     ('crates/pet-jsonrpc/src/output.rs', 'if let Err(error) = writer.write_all(&frame) {')]
        for name, marker in witnesses:
            path = self.root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(marker + '\n')
        raw = self.root / 'child.profraw'
        raw.write_bytes(b'nonempty')
        manifest = self.root / 'manifest.json'
        manifest.write_text(json.dumps({'binary': 'pet', 'profiles': {'idle': [str(raw)], 'info': [str(raw)]}}))
        for counts in [(0, 1), (1, 1), (0, 0)]:
            with self.subTest(counts=counts):
                before = ''.join(self.record(n, f'DA:1,{counts[0]}\n') for n, _ in witnesses)
                after = ''.join(self.record(n, f'DA:1,{counts[1]}\n') for n, _ in witnesses)
                with patch.object(detail, 'run', side_effect=['/toolchain', 'host: unit-test', '', before, '', after]):
                    if counts == (0, 1):
                        detail.verify_proof(self.root, manifest, self.root / 'proof')
                        evidence = json.loads((self.root / 'proof/proof.json').read_text())
                        self.assertEqual(set(evidence), {'handler', 'transport', 'writer'})
                    else:
                        with self.assertRaises(detail.SnapshotError):
                            detail.verify_proof(self.root, manifest, self.root / 'proof')

    def test_cli_failure_does_not_create_success_report(self):
        script = Path(detail.__file__)
        result = subprocess.run([sys.executable, str(script), '--root', str(self.root), 'report',
                                 '--lcov', 'missing', '--base', 'absent', '--output', str(self.root / 'out')],
                                capture_output=True, text=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('Coverage evidence failed', result.stderr)
        self.assertFalse((self.root / 'out/details.json').exists())


if __name__ == '__main__':
    unittest.main()
