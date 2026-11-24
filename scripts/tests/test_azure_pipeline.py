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

    def test_apiscan_uses_workload_identity_service_connection(self):
        pipeline = (ROOT / 'azure-pipelines' / 'devdiv-pre-release.yml').read_text(encoding='utf-8')
        apiscan_start = pipeline.index('- task: APIScan@2')
        tsa_start = pipeline.index('- task: TSAUpload@2', apiscan_start)
        apiscan = pipeline[apiscan_start:tsa_start]

        self.assertIn('azureSubscription: VSEng-APIScanSC', apiscan)
        self.assertRegex(
            apiscan,
            r'AzureServicesAuthConnectionString: RunAs=App;AppId=[^;]+;'
            r'TenantId=[^;]+;ServiceConnectionId=[^;]+;',
        )
        self.assertIn('SYSTEM_ACCESSTOKEN: $(System.AccessToken)', apiscan)


if __name__ == '__main__':
    unittest.main()
