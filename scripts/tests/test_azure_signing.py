# Copyright (c) Microsoft Corporation.
# Licensed under the MIT License.

import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
DOTNET = shutil.which('dotnet')


@unittest.skipUnless(DOTNET, 'dotnet is required to exercise the MSBuild signing project')
class SigningProjectTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        shutil.copy(ROOT / 'azure-pipelines' / 'signing' / 'sign.proj', self.root)
        core = self.root / 'packages' / 'Microsoft.VisualStudioEng.MicroBuild.Core.1.0.0' / 'build'
        core.mkdir(parents=True)
        (core / 'Microsoft.VisualStudioEng.MicroBuild.Core.props').write_text('<Project />')
        (core / 'Microsoft.VisualStudioEng.MicroBuild.Core.targets').write_text(
            '''<Project>
  <Target Name="SignFiles"
          Condition="('$(SignType)' == 'real' OR '$(SignType)' == 'test') AND '@(FilesToSign)' != ''">
    <Message Importance="high" Text="SIGNING %(FilesToSign.Identity)|%(FilesToSign.Authenticode)|%(FilesToSign.MacAppName)" />
  </Target>
</Project>'''
        )
        self.binary = self.root / 'pet'
        self.binary.write_text('fixture')

    def run_signing(self, *properties):
        env = os.environ.copy()
        env['DOTNET_SKIP_FIRST_TIME_EXPERIENCE'] = '1'
        env['DOTNET_CLI_TELEMETRY_OPTOUT'] = '1'
        return subprocess.run(
            [
                DOTNET,
                'msbuild',
                str(self.root / 'sign.proj'),
                '-nologo',
                '-v:normal',
                '-p:SignType=test',
                f'-p:FileToSign={self.binary}',
                '-p:Authenticode=Microsoft400',
                *properties,
            ],
            capture_output=True,
            text=True,
            env=env,
            check=False,
        )

    def test_signing_target_receives_file_and_metadata(self):
        for extra, expected in (
            ((), 'Microsoft400|'),
            (('-p:MacAppName=pet', '-p:Authenticode=8020'), '8020|pet'),
        ):
            with self.subTest(extra=extra):
                result = self.run_signing(*extra)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                self.assertIn(f'SIGNING {self.binary}|{expected}', result.stdout)

    def test_invalid_inputs_fail_instead_of_skipping_signing(self):
        self.binary.unlink()
        result = self.run_signing()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FileToSign does not exist', result.stdout)

        result = self.run_signing('-p:SignType=invalid')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SignType must be 'real' or 'test'", result.stdout)

    def test_wildcard_signs_every_match_in_one_operation(self):
        zips = []
        for target in ('aarch64-apple-darwin', 'x86_64-apple-darwin'):
            directory = self.root / 'macos' / target
            directory.mkdir(parents=True)
            zips.append(directory / 'pet.zip')
            zips[-1].write_text('fixture')

        result = self.run_signing(
            f'-p:FileToSign={self.root / "macos" / "*" / "pet.zip"}',
            f'-p:OutDir={self.root}{os.sep}',
            '-p:Authenticode=MacDeveloperHarden',
        )
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        for path in zips:
            self.assertIn(f'SIGNING {path}|MacDeveloperHarden|', result.stdout)

        result = self.run_signing(f'-p:FileToSign={self.root / "missing" / "*" / "pet.zip"}')
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('FileToSign does not exist', result.stdout)


if __name__ == '__main__':
    unittest.main()
