"""Verify the audited runtime seams against hashes from the downloaded archive."""
import argparse
import hashlib
import json
from pathlib import Path
from . import HERMES_REVISION
from .journal import BridgeError


def verify_source(source=Path('/opt/hermes')):
    """Do not accept a version marker in place of actual source-file evidence."""
    source = Path(source).resolve()
    lock_path = Path(__file__).resolve().parent.parent / 'hermes-source-lock.json'
    if lock_path.stat().st_size > 16384:
        raise BridgeError('invalid_source_lock')
    lock = json.loads(lock_path.read_text())
    if lock['revision'] != HERMES_REVISION or (source / '.env').exists() or (source / 'auth.json').exists():
        raise BridgeError('image_source_mismatch', 503)
    for name, digest in lock['source_files'].items():
        path = source / name
        if path.is_symlink() or not path.resolve().is_relative_to(source) or path.stat().st_size > 8 * 1024 * 1024:
            raise BridgeError('image_source_mismatch', 503)
        with path.open('rb') as file:
            actual = hashlib.file_digest(file, 'sha256').hexdigest()
        if actual != digest:
            raise BridgeError('image_source_mismatch', 503)
    return lock


def main():
    """Build-only source verification before stamping the informational marker."""
    parser = argparse.ArgumentParser()
    parser.add_argument('--source', default='/opt/hermes')
    parser.add_argument('--write-marker', action='store_true')
    args = parser.parse_args()
    lock = verify_source(Path(args.source))
    if args.write_marker:
        (Path(args.source) / 'ORTAK_SOURCE_REVISION').write_text(lock['revision'] + '\n')
    print(json.dumps({'source_revision': lock['revision'], 'verified_source_files': len(lock['source_files'])}))

if __name__ == '__main__':
    main()
