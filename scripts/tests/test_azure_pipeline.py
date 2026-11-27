# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import re
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
PIPELINES = ROOT / 'azure-pipelines'


def read(path: Path) -> str:
    return path.read_text(encoding='utf-8').replace('\r\n', '\n')


class DevDivPipelineTests(unittest.TestCase):
    def setUp(self):
        self.caller = read(PIPELINES / 'devdiv-pre-release.yml')
        self.template = read(PIPELINES / 'templates' / 'rust-package.yml')

    def test_caller_only_supplies_parameters_to_rust_package_template(self):
        self.assertIn('extends:\n  template: templates/rust-package.yml', self.caller)
        self.assertIn('buildPlatforms: ${{ parameters.buildPlatforms }}', self.caller)
        self.assertIn('windowsAdditionalNotices: $(Build.SourcesDirectory)/azure-pipelines/windows-additional-notices.txt', self.caller)
        self.assertNotIn('- job:', self.caller)
        self.assertIn('template: azure-pipelines/MicroBuild.1ES.Official.yml@MicroBuildTemplate', self.template)

    def test_notice_follows_a_registered_component_scan_after_build(self):
        build = self.template.index('- script: cargo build --release')
        scan = self.template.index('- task: ComponentGovernanceComponentDetection@0')
        notice = self.template.index('- task: notice@0')

        self.assertLess(build, scan)
        self.assertLess(scan, notice)
        scan_step = self.template[scan:notice]
        self.assertIn('scanType: Register', scan_step)
        self.assertNotIn('continueOnError: true', scan_step)
        self.assertNotIn('condition:', scan_step)

    def test_windows_notice_includes_unharvested_build_dependency(self):
        notice = self.template.index('- task: notice@0')
        symbols = self.template.index('- task: PublishSymbols@2', notice)
        notice_steps = self.template[notice:symbols]
        self.assertIn('additionaldata: ${{ parameters.windowsAdditionalNotices }}', notice_steps)
        self.assertIn('Verify supplemental Windows NOTICE', notice_steps)

        lockfile = read(ROOT / 'Cargo.lock')
        package = re.search(r'\[\[package\]\]\nname = "winresource"\nversion = "([^"]+)"', lockfile)
        self.assertIsNotNone(package)
        additional = read(PIPELINES / 'windows-additional-notices.txt')
        self.assertIn(f'winresource {package.group(1)} - MIT', additional)
        self.assertIn('Copyright 2016 Max Resch', additional)
        self.assertIn('Permission is hereby granted', additional)

    def test_apiscan_source_ref_gate(self):
        pattern = re.search(r"\$scanRef = \$env:BUILD_SOURCEBRANCH -cmatch '([^']+)'", self.template)
        self.assertIsNotNone(pattern)
        for ref, expected in (
            ('refs/heads/main', True),
            ('refs/heads/release/latest', True),
            ('refs/tags/release/2026.12', True),
            ('refs/tags/v2024.16.0', True),
            ('refs/pull/585/merge', False),
            ('refs/heads/devdiv-prerelease', False),
            ('refs/heads/feature/release/2026.12', False),
            ('refs/heads/Main', False),
            ('refs/tags/release/test', False),
            ('refs/tags/release/2026.12-preview', False),
            ('refs/tags/vnext', False),
        ):
            with self.subTest(ref=ref):
                self.assertEqual(bool(re.search(pattern[1], ref)), expected)

        stage = self.template.split('- stage: APIScan\n', 1)[1].split('jobs:', 1)[0]
        self.assertIn("condition: and(succeeded(), ne(variables['Build.Reason'], 'PullRequest'), "
                      "eq(dependencies.Package.outputs['Package_x86_64_pc_windows_msvc.classifySource.scanRef'], 'True'))", stage)

    def test_apiscan_uses_workload_identity_service_connection(self):
        apiscan_start = self.template.index('- task: APIScan@2')
        tsa_start = self.template.index('- task: TSAUpload@2', apiscan_start)
        apiscan = self.template[apiscan_start:tsa_start]

        self.assertIn('azureSubscription: ${{ parameters.apiScanServiceConnection }}', apiscan)
        self.assertIn('AzureServicesAuthConnectionString: ${{ parameters.apiScanConnectionString }}', apiscan)
        self.assertIn('SYSTEM_ACCESSTOKEN: $(System.AccessToken)', apiscan)
        self.assertIn("default: VSEng-APIScanSC", self.template)
        self.assertRegex(
            self.caller,
            r'apiScanConnectionString: RunAs=App;AppId=[^;]+;TenantId=[^;]+;ServiceConnectionId=[^;]+;',
        )

    def test_every_stage_sets_microbuild_team_name(self):
        stages = self.template.split('- stage: ')[1:]
        self.assertEqual(len(stages), 2)
        for stage in stages:
            self.assertIn('TeamName: ${{ parameters.teamName }}', stage)

    def test_macos_jobs_cover_only_selected_macos_targets(self):
        signing = self.template[self.template.index('- job: SignMacOS'):self.template.index('- job: VerifyMacOS')]
        verification = self.template[self.template.index('- job: VerifyMacOS'):self.template.index('- stage: APIScan')]
        guard = self.template[:self.template.index('- job: SignMacOS')].rsplit('- ${{ if', 1)[1]

        self.assertIn('''contains(convertToJson(parameters.buildPlatforms), '"darwin"')''', guard)
        for section in (signing, verification):
            self.assertIn("${{ each platform in parameters.buildPlatforms }}", section)
            self.assertIn("${{ if eq(platform.name, 'darwin') }}", section)
        self.assertIn('- Package_${{ replace(platform.target, \'-\', \'_\') }}', signing)
        self.assertEqual(signing.count('/macos/*/${{ parameters.binaryName }}.zip"'), 2)
        self.assertIn('artifactName: bin-${{ platform.target }}', signing)
        self.assertIn('lipo -archs "$binary"', verification)
        self.assertIn('codesign --verify --strict --verbose=2 "$binary"', verification)
        self.assertIn('ThirdPartyNotices.txt', verification)


if __name__ == '__main__':
    unittest.main()
