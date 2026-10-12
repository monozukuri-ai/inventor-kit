"""Version drift and failed updates must not leave a partially bumped release."""
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'scripts'))
from bump_version import MANAGED_FILES, bump_version, update_manifest
from versioning import check_versions, validate_version


class Versioning(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.files = {
            'Cargo.toml': '[workspace]\nmembers = ["crates/inventor-core", "crates/inventor-py"]\n'
                          '[workspace.package]\nversion = "1.2.3" # keep comment\n',
            'crates/inventor-core/Cargo.toml': '[package]\nname = "inventor-core"\nversion.workspace = true\n',
            'crates/inventor-py/Cargo.toml': '[package]\nname = "inventor-py"\nversion.workspace = true\n',
            'fuzz/Cargo.toml': '[package]\nname = "inventor-fuzz"\nversion = "0.0.0"\n',
            'pyproject.toml': '[project]\nname = "inventor-kit"\ndynamic = ["version"]\n',
            'viewer/package.json': json.dumps({'private': True}),
            'viewer/package-lock.json': json.dumps({'packages': {'': {}}}),
            'Cargo.lock': '[[package]]\nname = "inventor-core"\nversion = "1.2.3"\n'
                          '[[package]]\nname = "inventor-py"\nversion = "1.2.3"\n'
                          '[[package]]\nname = "external"\nversion = "2.0.0"\nsource = "registry+test"\n',
            'fuzz/Cargo.lock': '[[package]]\nname = "inventor-core"\nversion = "1.2.3"\n'
                               '[[package]]\nname = "inventor-fuzz"\nversion = "0.0.0"\n',
            'uv.lock': '[[package]]\nname = "inventor-kit"\nsource = {editable = "."}\n'
                       '[[package]]\nname = "external"\nversion = "2.0.0"\n',
        }
        for name, text in self.files.items():
            p = self.root/name
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding='utf-8')

    def snapshot(self):
        return {name: (self.root/name).read_bytes() for name in self.files}

    def test_current_sources_and_tag(self):
        self.assertEqual(check_versions(self.root, 'v1.2.3'), '1.2.3')
        with self.assertRaisesRegex(ValueError, 'Release tag'):
            check_versions(self.root, 'v1.2.4')

    def test_invalid_versions_before_mutation(self):
        before = self.snapshot()
        for version in ('', 'v1.2.3', '01.2.3', '1.2', '1.2.3\n', '1.2.3-rc.1', '1.2.3+local'):
            with self.subTest(version=version), self.assertRaises(ValueError):
                bump_version(self.root, version)
        self.assertEqual(validate_version('0.8.0'), '0.8.0')
        self.assertEqual(before, self.snapshot())

    def test_stale_missing_and_duplicate_cargo_locks(self):
        for name in ('Cargo.lock', 'fuzz/Cargo.lock'):
            original = self.files[name]
            entry = '[[package]]\nname = "inventor-core"\nversion = "1.2.3"\n'
            for text in (original.replace('1.2.3', '1.2.2'), original.replace(entry, ''),
                         original + entry, original.replace(entry, entry + 'source = "registry+test"\n')):
                (self.root/name).write_text(text)
                with self.subTest(name=name, text=text), self.assertRaises(ValueError):
                    check_versions(self.root)
            (self.root/name).write_text(original)

    def test_dynamic_uv_version_may_be_omitted_but_not_stale(self):
        original = self.files['uv.lock']
        for field in ('', 'version = "1.2.3"\n', 'version = "1.2.2"\n'):
            (self.root/'uv.lock').write_text(original.replace('name = "inventor-kit"\n', 'name = "inventor-kit"\n' + field))
            if '1.2.2' in field:
                with self.assertRaises(ValueError):
                    check_versions(self.root)
            else:
                check_versions(self.root)
        for text in (original.replace('editable = "."', 'registry = "https://pypi.org/simple"'),
                     original + '[[package]]\nname = "inventor-kit"\nsource = {editable = "."}\n'):
            (self.root/'uv.lock').write_text(text)
            with self.assertRaises(ValueError):
                check_versions(self.root)

    def test_duplicate_version_definitions_are_rejected(self):
        changes = {
            'crates/inventor-py/Cargo.toml': self.files['crates/inventor-py/Cargo.toml'].replace('version.workspace = true', 'version = "1.2.3"'),
            'pyproject.toml': self.files['pyproject.toml'] + 'version = "1.2.3"\n',
            'viewer/package.json': '{"private": true, "version": "1.2.3"}',
            'viewer/package-lock.json': '{"packages": {"": {"version": "1.2.3"}}}',
        }
        for name, text in changes.items():
            (self.root/name).write_text(text)
            with self.subTest(name=name), self.assertRaises(ValueError):
                check_versions(self.root)
            (self.root/name).write_text(self.files[name])

    def test_manifest_comments_line_endings_and_other_versions_survive(self):
        text = self.files['Cargo.toml'].replace('\n', '\r\n') + '[workspace.dependencies]\r\nother = "1.2.3"\r\n'
        changed = update_manifest(text, '2.0.0')
        self.assertEqual(changed, text.replace('version = "1.2.3"', 'version = "2.0.0"'))
        with self.assertRaises(ValueError):
            update_manifest('[workspace.package]\nlicense = "MIT"\n', '2.0.0')

    def finish_locks(self, command, **kwargs):
        for name in ('Cargo.lock', 'fuzz/Cargo.lock'):
            p = self.root/name
            p.write_bytes(p.read_bytes().replace(b'1.2.3', b'2.0.0'))

    def test_success_preserves_dependencies_and_unrelated_files(self):
        before = self.snapshot()
        with patch('bump_version.subprocess.run', side_effect=self.finish_locks) as run:
            self.assertEqual(bump_version(self.root, '2.0.0', offline=True), '2.0.0')
        self.assertEqual(check_versions(self.root), '2.0.0')
        self.assertTrue(all('--offline' in call.args[0] for call in run.call_args_list))
        for name, content in before.items():
            if name not in MANAGED_FILES:
                self.assertEqual((self.root/name).read_bytes(), content)

    def test_omitted_version_synchronizes_manually_edited_workspace(self):
        p = self.root/'Cargo.toml'
        p.write_text(update_manifest(p.read_text(), '2.0.0'))
        with patch('bump_version.subprocess.run', side_effect=self.finish_locks):
            self.assertEqual(bump_version(self.root), '2.0.0')

    def test_failure_at_each_command_restores_exact_starting_bytes(self):
        for failure_index in range(3):
            before = self.snapshot()
            count = 0
            def fail(command, **kwargs):
                nonlocal count
                self.finish_locks(command)
                if count == failure_index:
                    (self.root/'uv.lock').write_bytes(b'partial lock')
                    raise subprocess.CalledProcessError(1, command)
                count += 1
            with patch('bump_version.subprocess.run', side_effect=fail), self.assertRaises(subprocess.CalledProcessError):
                bump_version(self.root, '2.0.0')
            self.assertEqual(before, self.snapshot())

    def test_unexpected_dependency_update_is_rolled_back(self):
        before = self.snapshot()
        def drift(command, **kwargs):
            self.finish_locks(command)
            p = self.root/'uv.lock'
            p.write_text(p.read_text().replace('2.0.0', '2.0.1'))
        with patch('bump_version.subprocess.run', side_effect=drift), self.assertRaisesRegex(ValueError, 'dependency changes'):
            bump_version(self.root, '2.0.0')
        self.assertEqual(before, self.snapshot())

    def test_failed_postcheck_and_interrupt_are_rolled_back(self):
        before = self.snapshot()
        with patch('bump_version.subprocess.run'), self.assertRaises(ValueError):
            bump_version(self.root, '2.0.0')
        self.assertEqual(before, self.snapshot())
        with patch('bump_version.subprocess.run', side_effect=KeyboardInterrupt), self.assertRaises(KeyboardInterrupt):
            bump_version(self.root, '2.0.0')
        self.assertEqual(before, self.snapshot())


if __name__ == '__main__':
    unittest.main()
