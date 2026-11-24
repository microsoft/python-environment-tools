# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]


class DevDivPipelineTests(unittest.TestCase):
    def test_notice_follows_a_registered_component_scan_after_build(self):
        pipeline = (ROOT / 'azure-pipelines' / 'devdiv-pre-release.yml').read_text(encoding='utf-8')
        build = pipeline.index('- script: cargo build --release')
        scan = pipeline.index('- task: ComponentGovernanceComponentDetection@0')
        notice = pipeline.index('- task: notice@0')

        self.assertLess(build, scan)
        self.assertLess(scan, notice)
        scan_step = pipeline[scan:notice]
        self.assertIn('scanType: Register', scan_step)
        self.assertNotIn('continueOnError: true', scan_step)
        self.assertNotIn('condition:', scan_step)


if __name__ == '__main__':
    unittest.main()
