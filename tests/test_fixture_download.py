"""Exercise cold fixture downloads and their integrity checks without network access."""
import hashlib
import io
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import zipfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'scripts'))
import fetch_assembly_samples
import fetch_public_samples
from fixture_download import download


def digest(data):
    return hashlib.sha256(data).hexdigest()


class FixtureDownloads(unittest.TestCase):
    def response(self, payloads):
        def open_request(request, *, timeout):
            # Reproduce an origin that rejects urllib's default User-Agent.
            url = request.full_url if hasattr(request, 'full_url') else request
            agent = request.get_header('User-agent') if hasattr(request, 'get_header') else None
            if not agent or not agent.startswith('inventor-kit-fixtures/'):
                raise urllib.error.HTTPError(url, 403, 'Forbidden', None, None)
            self.assertIn('https://github.com/monozukuri-ai/inventor-kit', agent)
            self.assertGreater(timeout, 0)
            return io.BytesIO(payloads[url])
        return open_request

    def test_cold_public_download_checks_archive_and_reuses_cache(self):
        archive_url = 'https://samples.example/archive.zip'
        direct_url = 'https://samples.example/direct.ipt'
        payload = b'fixed fixture'
        archive_bytes = io.BytesIO()
        with zipfile.ZipFile(archive_bytes, 'w') as archive:
            archive.writestr('parts/one.ipt', payload)
            archive.writestr('parts/two.ipt', payload)
        zipped = archive_bytes.getvalue()
        rows = [dict(file=f'{name}.ipt', url=archive_url, bytes=len(payload),
                     sha256=digest(payload), archive_sha256=digest(zipped),
                     archive_member=f'parts/{name}.ipt') for name in ('one', 'two')]
        rows.append(dict(file='direct.ipt', url=direct_url, bytes=len(payload), sha256=digest(payload)))
        with tempfile.TemporaryDirectory() as directory:
            dest, cache = Path(directory) / 'files', Path(directory) / 'archives'
            args = ['fetch_public_samples.py', '--dest', str(dest), '--cache', str(cache)]
            with patch.object(sys, 'argv', args), patch.object(fetch_public_samples, 'load_manifest', return_value=rows):
                with patch('urllib.request.urlopen', side_effect=self.response({archive_url: zipped, direct_url: payload})) as opener:
                    fetch_public_samples.main()
                    self.assertEqual(opener.call_count, 2)
                for row in rows:
                    self.assertEqual((dest / row['file']).read_bytes(), payload)
                self.assertEqual((cache / f'{digest(zipped)}.zip').read_bytes(), zipped)
                with patch('urllib.request.urlopen', side_effect=AssertionError('Unexpected network access')):
                    fetch_public_samples.main()
                    # Re-extract an absent fixture from the verified archive cache.
                    (dest / 'one.ipt').unlink()
                    fetch_public_samples.main()
                    self.assertEqual((dest / 'one.ipt').read_bytes(), payload)

    def test_public_download_rejects_changed_archive(self):
        url = 'https://samples.example/archive.zip'
        row = dict(file='one.ipt', url=url, bytes=7, sha256=digest(b'fixture'),
                   archive_sha256=digest(b'expected archive'), archive_member='one.ipt')
        with tempfile.TemporaryDirectory() as directory:
            dest, cache = Path(directory) / 'files', Path(directory) / 'archives'
            args = ['fetch_public_samples.py', '--dest', str(dest), '--cache', str(cache)]
            with patch.object(sys, 'argv', args), patch.object(fetch_public_samples, 'load_manifest', return_value=[row]), \
                    patch('urllib.request.urlopen', side_effect=self.response({url: b'changed archive'})):
                with self.assertRaisesRegex(ValueError, 'Archive SHA-256 mismatch'):
                    fetch_public_samples.main()
            self.assertFalse((dest / 'one.ipt').exists())
            self.assertEqual(list(cache.iterdir()), [])

    def test_direct_download_rejects_wrong_hash_and_size(self):
        url = 'https://samples.example/one.ipt'
        row = dict(file='one.ipt', url=url, bytes=7, sha256=digest(b'fixture'))
        for payload in (b'changed', b'fixture extra'):
            with self.subTest(payload=payload), tempfile.TemporaryDirectory() as directory:
                dest, cache = Path(directory) / 'files', Path(directory) / 'archives'
                args = ['fetch_public_samples.py', '--dest', str(dest), '--cache', str(cache)]
                with patch.object(sys, 'argv', args), patch.object(fetch_public_samples, 'load_manifest', return_value=[row]), \
                        patch('urllib.request.urlopen', side_effect=self.response({url: payload})):
                    with self.assertRaisesRegex(ValueError, 'Fixture mismatch'):
                        fetch_public_samples.main()
                self.assertFalse((dest / row['file']).exists())

    def test_assembly_download_uses_identified_requests_and_checks_hash(self):
        url, payload = 'https://samples.example/part.ipt', b'fixture'
        row = dict(file='assembly/part.ipt', url=url, bytes=len(payload), sha256=digest(payload))
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'fixtures').mkdir()
            (root / 'fixtures/assembly-manifest.json').write_text(json.dumps([row]))
            args = ['fetch_assembly_samples.py', '--dest', str(root / 'download')]
            with patch.object(sys, 'argv', args), patch.object(fetch_assembly_samples, 'ROOT', root):
                with patch('urllib.request.urlopen', side_effect=self.response({url: b'changed'})):
                    with self.assertRaisesRegex(ValueError, 'Fixture mismatch'):
                        fetch_assembly_samples.main()
                self.assertFalse((root / 'download' / row['file']).exists())
                with patch('urllib.request.urlopen', side_effect=self.response({url: payload})):
                    fetch_assembly_samples.main()
                self.assertEqual((root / 'download' / row['file']).read_bytes(), payload)

    def test_http_error_is_not_silenced(self):
        error = urllib.error.HTTPError('https://samples.example/file', 403, 'Forbidden', None, None)
        with patch('urllib.request.urlopen', side_effect=error):
            with self.assertRaises(urllib.error.HTTPError) as raised:
                download(error.url)
        self.assertIs(raised.exception, error)


if __name__ == '__main__':
    unittest.main()
