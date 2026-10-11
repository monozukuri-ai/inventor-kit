"""Check public documentation links and reject internal files in the Git index."""
import argparse
from pathlib import Path
import re
import subprocess
from urllib.parse import unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
PRIVATE_ROOTS = {'internal', '.internal', '.local'}
PRIVATE_SUFFIXES = {'.ipt', '.iam', '.idw', '.ipn', '.step', '.stp', '.sab', '.sat', '.log'}


def check_public_path(name):
    path = Path(name)
    if (path.parts and path.parts[0] in PRIVATE_ROOTS) or path.suffix.lower() in PRIVATE_SUFFIXES:
        raise ValueError(f'Private data must not be tracked publicly: {name}')
    if path.name in {'error.png', 'error.log'}:
        raise ValueError(f'Private capture must not be tracked publicly: {name}')


def check_links(root):
    root = root.resolve()
    documents = list(root.glob('*.md'))
    for directory in ('docs', 'scripts', 'benchmarks', 'fixtures/oracles'):
        documents.extend((root / directory).rglob('*.md'))
    for path in documents:
        text = path.read_text(encoding='utf-8')
        for match in re.finditer(r'\]\(([^)]+)\)', text):
            target = match.group(1).strip().strip('<>')
            parsed = urlsplit(target)
            if parsed.scheme or parsed.netloc or not parsed.path:
                continue
            destination = (path.parent / unquote(parsed.path)).resolve()
            if not destination.is_relative_to(root):
                raise ValueError(f'Public link leaves the repository: {path.name}: {target}')
            if any(destination.is_relative_to(root / name) for name in PRIVATE_ROOTS):
                raise ValueError(f'Public link depends on internal files: {path.name}: {target}')
            if not destination.exists():
                raise ValueError(f'Broken public link: {path.name}: {target}')
    return len(documents)


def check_index(root):
    result = subprocess.run(['git', 'ls-files', '-z'], cwd=root,
                            check=True, capture_output=True)
    for name in result.stdout.decode().split('\0'):
        if name:
            check_public_path(name)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=ROOT)
    args = parser.parse_args()
    if (args.root / '.git').exists():
        check_index(args.root)
    count = check_links(args.root)
    print(f'Public layout passed: {count} documentation files; no internal link dependency')


if __name__ == '__main__':
    main()
