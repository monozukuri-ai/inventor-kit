"""Identify opt-in fixture downloads to public source servers."""
import urllib.request

USER_AGENT = 'inventor-kit-fixtures/0.1 (+https://github.com/monozukuri-ai/inventor-kit)'


def download(url, *, max_bytes=None, timeout=120):
    # NIST rejects urllib's default Python-urllib User-Agent with HTTP 403.
    request = urllib.request.Request(url, headers={'User-Agent': USER_AGENT})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read() if max_bytes is None else response.read(max_bytes + 1)
