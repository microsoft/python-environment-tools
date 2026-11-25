# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import re
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

    def test_windows_notice_includes_unharvested_build_dependency(self):
        pipeline = (ROOT / 'azure-pipelines' / 'devdiv-pre-release.yml').read_text(encoding='utf-8')
        notice = pipeline.index('- task: notice@0')
        symbols = pipeline.index('- task: PublishSymbols@2', notice)
        notice_steps = pipeline[notice:symbols]
        self.assertIn(
            'additionaldata: $(Build.SourcesDirectory)/azure-pipelines/windows-additional-notices.txt',
            notice_steps,
        )
        self.assertIn('Verify supplemental Windows NOTICE', notice_steps)

        lockfile = (ROOT / 'Cargo.lock').read_text(encoding='utf-8')
        package = re.search(r'\[\[package\]\]\nname = "winresource"\nversion = "([^"]+)"', lockfile)
        self.assertIsNotNone(package)
        additional = (ROOT / 'azure-pipelines' / 'windows-additional-notices.txt').read_text(encoding='utf-8')
        self.assertIn(f'winresource {package.group(1)} - MIT', additional)
        self.assertIn('Copyright 2016 Max Resch', additional)
        self.assertIn('Permission is hereby granted', additional)

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

    def test_macos_artifacts_are_signed_and_verified_together(self):
        pipeline = (ROOT / 'azure-pipelines' / 'devdiv-pre-release.yml').read_text(encoding='utf-8')
        signing = pipeline[pipeline.index('- job: sign_macos'):pipeline.index('- job: APIScan')]
        verification = pipeline[pipeline.index('- job: verify_macos'):pipeline.index('- job: APIScan')]
        sign_project = (ROOT / 'azure-pipelines' / 'signing' / 'sign.proj').read_text(encoding='utf-8')

        self.assertIn('dependsOn:\n              - build_aarch64_apple_darwin\n              - build_x86_64_apple_darwin', signing)
        self.assertIn('/p:AdditionalFilesToSign="$(Build.ArtifactStagingDirectory)/x86_64-apple-darwin/pet.zip"', signing)
        self.assertIn('artifactName: bin-aarch64-apple-darwin', signing)
        self.assertIn('artifactName: bin-x86_64-apple-darwin', signing)
        self.assertIn('<FilesToSign Include="$(AdditionalFilesToSign)"', sign_project)
        self.assertIn('lipo -archs "$pet"', verification)
        self.assertIn('codesign --verify --strict --verbose=2 "$pet"', verification)
        self.assertIn('ThirdPartyNotices.txt', verification)


if __name__ == '__main__':
    unittest.main()
