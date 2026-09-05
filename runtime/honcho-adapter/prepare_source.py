"""Verify the public archive, extract regular files, then apply pinned seams."""

import hashlib
import json
import shutil
import sys
import tarfile
from pathlib import Path, PurePosixPath

from apply_source_patch import patch


def prepare(archive, destination):
    lock = json.loads(Path(__file__).with_name("honcho-source-lock.json").read_text())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != lock["archive_sha256"]:
        raise ValueError("Honcho archive does not match reviewed SHA256")
    destination.mkdir(parents=True, exist_ok=False)
    with tarfile.open(archive) as source:
        for member in source:
            path = PurePosixPath(member.name)
            if (
                path.is_absolute()
                or ".." in path.parts
                or path.parts[0] != "honcho-" + lock["commit"]
            ):
                raise ValueError("unsafe or unexpected source member")
            target = destination.joinpath(*path.parts[1:])
            if member.isdir():
                target.mkdir(parents=True, exist_ok=True)
            elif member.isfile():
                target.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(member) as data, target.open("wb") as output:
                    shutil.copyfileobj(data, output)
            elif (
                member.issym()
                and str(PurePosixPath(*path.parts[1:]))
                in {".agents/skills", ".claude/skills"}
                and member.linkname == "../skills"
            ):
                continue  # Pinned editor-only links are not part of the build.
            else:
                raise ValueError("non-regular source member")
    patch(destination)


if __name__ == "__main__":
    prepare(Path(sys.argv[1]), Path(sys.argv[2]))
