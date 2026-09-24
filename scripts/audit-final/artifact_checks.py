#!/usr/bin/env python3
"""Reject empty/incomplete/unsafe source TARs; optionally verify every file and mode.

Integrity and provenance only. Passing never means that the source compiles.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import tarfile
from pathlib import Path, PurePosixPath

FONTS = {".ttf", ".otf", ".woff", ".woff2", ".eot"}


def inspect_source(
    path: Path, *, min_files: int = 100, manifest: dict | None = None
) -> dict:
    records = {}
    prefixes = set()
    total = 0
    with tarfile.open(path, "r:*") as archive:
        members = archive.getmembers()
        if not members:
            raise ValueError("Empty TAR: no source files")
        for member in members:
            name = PurePosixPath(member.name)
            if name.is_absolute() or ".." in name.parts or not name.parts:
                raise ValueError(f"Unsafe archive path: {member.name}")
            prefixes.add(name.parts[0])
            if member.isdir():
                continue
            if not member.isfile():
                raise ValueError(f"Non-regular source entry: {member.name}")
            if len(name.parts) < 2:
                raise ValueError(
                    "Source archive must have a single containing directory"
                )
            relative = PurePosixPath(*name.parts[1:]).as_posix()
            if relative in records:
                raise ValueError(f"Duplicate file: {relative}")
            if name.suffix.lower() in FONTS:
                raise ValueError("Font files must not be distributed")
            if member.size < 0 or member.size > 128 * 1024 * 1024:
                raise ValueError("Oversized member")
            total += member.size
            if total > 1024 * 1024 * 1024:
                raise ValueError("Oversized source archive")
            stream = archive.extractfile(member)
            if stream is None:
                raise ValueError(f"Unreadable member: {relative}")
            digest = hashlib.sha256()
            count = 0
            while block := stream.read(1024 * 1024):
                digest.update(block)
                count += len(block)
            if count != member.size:
                raise ValueError("Truncated member")
            records[relative] = {
                "bytes": count,
                "sha256": digest.hexdigest(),
                "mode": member.mode & 0o777,
            }
    if len(prefixes) != 1:
        raise ValueError("Archive has multiple top-level directories")
    if len(records) < min_files:
        raise ValueError(f"Incomplete source: only {len(records)} files")
    for required in ["Cargo.toml", "Cargo.lock"]:
        if required not in records:
            raise ValueError(f"Missing {required}")
    rust = [p for p in records if p.startswith("crates/") and p.endswith(".rs")]
    if not rust:
        raise ValueError("No workspace Rust sources")
    if manifest is not None and records != manifest:
        added = sorted(set(records) - set(manifest))
        missing = sorted(set(manifest) - set(records))
        changed = [
            p for p in records.keys() & manifest.keys() if records[p] != manifest[p]
        ]
        raise ValueError(
            f"Manifest mismatch: added={added[:5]} missing={missing[:5]} changed={changed[:5]}"
        )
    return {
        "files": len(records),
        "rust_files": len(rust),
        "uncompressed_bytes": total,
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "manifest_matched": manifest is not None,
        "scope": "Archive content, modes and hashes; not code correctness",
    }


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("archive", type=Path)
    p.add_argument("--manifest", type=Path)
    p.add_argument("--json", type=Path)
    a = p.parse_args()
    try:
        result = inspect_source(
            a.archive,
            manifest=json.loads(a.manifest.read_text()) if a.manifest else None,
        )
        result["status"] = "PASS"
        code = 0
    except (ValueError, OSError, tarfile.TarError) as error:
        result = {"status": "FAIL", "error": str(error)}
        code = 1
    data = json.dumps(result, ensure_ascii=False, indent=2) + "\n"
    if a.json:
        a.json.write_text(data)
    print(data)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
