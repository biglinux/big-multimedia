#!/usr/bin/env python3
"""Display-free review checks. These are NOT Rust compilation or GTK/GPU tests.

Run from any directory. JSON always identifies native validation as not run.
SQL checks execute SQL extracted from production Rust against Python's SQLite;
they verify the SQL contract, not Rust transaction/drop behavior.
"""
from __future__ import annotations
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import sqlite3
import subprocess
import sys
import tomllib
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
RESULTS: list[dict[str, object]] = []

def check(name, action, category="static"):
    try:
        detail = action()
        RESULTS.append({"name": name, "category": category, "status": "PASS", "detail": detail})
    except Exception as error:
        RESULTS.append({"name": name, "category": category, "status": "FAIL", "detail": str(error)})

def require(condition, message):
    if not condition:
        raise AssertionError(message)

def text(path):
    return (ROOT / path).read_text(encoding="utf-8")

def manifests():
    paths = sorted(ROOT.glob("**/Cargo.toml"))
    paths = [p for p in paths if "target" not in p.parts and ".git" not in p.parts]
    for path in paths:
        tomllib.loads(path.read_text())
    tomllib.loads(text("Cargo.lock"))
    return {"manifests": len(paths), "lockfile": "parsed; dependency resolution not run"}

def xml_files():
    paths = sorted(p for p in (ROOT / "crates").rglob("*") if p.suffix in (".ui", ".xml") and p.is_file())
    for path in paths:
        ET.parse(path)
    return {"files": len(paths), "scope": "XML syntax only, not GTK builder property validation"}

def catalog(path):
    entries = {}
    for block in text(path).split("\n\n"):
        values = {}; key = None
        for line in block.splitlines():
            m = re.match(r'(msgctxt|msgid_plural|msgid|msgstr(?:\[\d+\])?)\s+(".*")$', line)
            if m:
                key = m[1]; values[key] = ast.literal_eval(m[2])
            elif line.startswith('"') and key:
                values[key] += ast.literal_eval(line)
            elif line and not line.startswith("#"):
                key = None
        if "msgid" not in values:
            continue
        identity = (values.get("msgctxt", ""), values["msgid"])
        require(identity not in entries, f"duplicate msgid: {identity!r} in {path}")
        entries[identity] = values
    return entries

def catalogs():
    pot = catalog("crates/media-runtime/po/big-media-runtime.pot")
    pt = catalog("crates/media-runtime/po/pt_BR.po")
    examples = ["Light", "Detailed", "Compatibility",
        "Press Alt+Down to adjust. Press Escape to close.",
        "The playlist could not be changed. Your previous playlist was kept."]
    for value in examples:
        require(("", value) in pot, f"POT missing {value}")
        require(bool(pt.get(("", value), {}).get("msgstr")), f"pt_BR missing {value}")
    return {"pot_entries": len(pot), "pt_BR_entries": len(pt), "note": "msgfmt/xgettext still required natively"}

def extraction_paths():
    paths = [line.strip() for line in text("crates/media-runtime/po/POTFILES.in").splitlines() if line.strip() and not line.startswith("#")]
    require(len(paths) == len(set(paths)), "duplicate extraction path")
    for path in paths:
        require((ROOT / path).is_file(), f"missing extraction source: {path}")
    return {"paths": len(paths)}

def storage_contract():
    source = text("crates/foundation/big-os-kit/src/storage.rs")
    for needle in ["create_new(true)", "options.mode(0o600)", "file.sync_all()?", "cleanup.0 = None", "fs::rename(&temporary, path)?"]:
        require(needle in source, f"storage guard missing: {needle}")
    require(source.index("create_sibling_file(&temporary, private)?") < source.index("let mut cleanup ="), "cleanup armed before exclusive creation")
    return "exclusive creation, owner-only private mode, sync, cleanup guards found; filesystem/Rust tests not run"

def frame_contract():
    sw = text("crates/big-video-player/src/video/ui/video_shell/render.rs")
    pool = text("crates/media-runtime/src/player/mpv_backend/software_frames.rs")
    require("glib::Bytes::from_owned(frame)" in sw, "immutable owned bytes not used")
    require("Duration::from_millis(16)" not in sw, "permanent 16 ms software poll reintroduced")
    for needle in ["const SLOT_COUNT: usize = 3", "#[repr(C, align(64))]", "try_reserve_exact", "Weak<Mutex<PoolState>>"]:
        require(needle in pool, f"software pool guard missing: {needle}")
    return "bounded/aligned lease and event-driven presentation guards found; not a GPU/memory safety proof"

def gpu_contract():
    source = text("crates/media-runtime/src/player/mpv_backend/offload_renderer/mod.rs")
    pool = text("crates/media-runtime/src/player/mpv_backend/offload_renderer/texture_pool.rs")
    egl = text("crates/media-runtime/src/player/mpv_backend/egl_dmabuf.rs")
    for needle in ["set_sync(", "Box::from_raw(payload)", "CurrentContext::enter", "move || drop(lease)"]:
        require(needle in source, f"GPU contract missing: {needle}")
    require("compare_exchange(false, true" in pool, "busy lease guard missing")
    require("modifiers.as_mut_ptr()" in egl, "EGL modifiers array missing")
    return "ownership/fence/failure-cleanup guards found; GL driver behavior untested"

def queue_contract():
    s = text("crates/media-runtime/src/player/mpv_backend/backend/mod.rs")
    drop = s[s.index("impl Drop for MpvBackend"):]
    require(drop.index("self.event_sender.close()") < drop.index("join()"), "join before closing bounded queue")
    return "close-before-join guard found; native backend tests cover volume and scheduling"

def library_contract():
    s = text("crates/big-audio-player/src/audio/ui/audio_shell/library_store.rs")
    update = text("crates/big-audio-player/src/audio/ui/audio_shell/update.rs")
    require(s.count("ticket.is_current()") >= 2, "missing pre/post-query cancellation")
    require("self.library_generation.is_current(generation)" in update, "missing final UI generation check")
    search = text("crates/big-audio-player/src/audio/ui/library_grouping_ui.rs")
    require(".search_delay(150)" in search and "debouncer.run" not in search, "double search debounce")
    return "generation guards and one GTK debounce found; native thread tests pending"

def rust_string(value):
    # Rust permits literal newlines and backslash-newline continuations in strings.
    value = re.sub(r"\\\n\s*", "", value)
    return json.loads(value.replace("\n", "\\n"))

def sql_function(name):
    source = text("crates/foundation/big-media-data/src/db/queries.rs")
    part = source[source.index(f"pub fn {name}("):]
    part = part.split("\npub fn ", 1)[0]
    strings = [rust_string(m.group()) for m in re.finditer(r'"(?:\\.|[^"\\])*"', part)]
    return [s for s in strings if s.lstrip().startswith(("INSERT", "DELETE"))]

def sql_checks():
    # Use the actual schema's initial SQL batch, rather than a hand-written mock.
    schema = text("crates/foundation/big-media-data/src/db/schema.rs")
    batch = re.search(r'conn\.execute_batch\(\s*("(?:\\.|[^"\\])*")', schema).group(1)
    db = sqlite3.connect(":memory:", isolation_level=None)
    db.executescript(rust_string(batch))
    db.execute("PRAGMA foreign_keys=ON")
    db.execute("INSERT INTO tracks (file_path,title) VALUES ('/one.flac','One'),('/two.flac','Two')")
    create = sql_function("create_playlist")[0]
    insert = sql_function("create_playlist_with_tracks")[0]
    deletes = sql_function("delete_playlist")
    append = sql_function("add_track_to_playlist")[0]
    db.execute("BEGIN")
    pid = db.execute(create, ("failed",)).lastrowid
    db.execute(insert, (pid, 1, 0))
    try:
        db.execute(insert, (pid, 99999, 1))
    except sqlite3.IntegrityError:
        db.execute("ROLLBACK")
    else:
        raise AssertionError("foreign-key failure not detected")
    require(db.execute("SELECT COUNT(*) FROM playlists").fetchone()[0] == 0, "partial playlist survived rollback")
    db.execute("BEGIN")
    pid = db.execute(create, ("saved",)).lastrowid
    db.execute(insert, (pid, 2, 0))
    db.execute(append, (pid, 1))
    db.execute("COMMIT")
    require(db.execute("SELECT track_id,position FROM playlist_tracks ORDER BY position").fetchall() == [(2, 0), (1, 1)], "wrong playlist order")
    db.executescript("CREATE TRIGGER refuse_delete BEFORE DELETE ON playlists BEGIN SELECT RAISE(ABORT,'test'); END;")
    db.execute("BEGIN")
    try:
        for statement in deletes:
            db.execute(statement, (pid,))
    except sqlite3.IntegrityError:
        db.execute("ROLLBACK")
    else:
        raise AssertionError("injected delete failure not detected")
    require(db.execute("SELECT COUNT(*) FROM playlist_tracks").fetchone()[0] == 2, "membership lost on delete failure")
    require(not db.execute("PRAGMA foreign_key_check").fetchall(), "foreign-key inconsistency")
    db.close()
    return {"sqlite_version": sqlite3.sqlite_version, "scenarios": ["failed batch rollback", "append/order", "failed delete rollback", "foreign keys"], "limit": "Python executes extracted SQL; Rust transaction lifetime NOT tested"}

def imports():
    result = subprocess.run(["bash", str(ROOT / "scripts/check-media-surface-imports.sh")], capture_output=True, text=True, check=False)
    require(result.returncode == 0, result.stdout + result.stderr)
    return "existing media-surface import gate passed"

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    for name, action in [("TOML/lock syntax", manifests), ("XML syntax", xml_files), ("gettext catalog structure", catalogs),
        ("gettext extraction paths", extraction_paths), ("storage source contract", storage_contract),
        ("software frame source contract", frame_contract), ("GPU lease source contract", gpu_contract),
        ("event/volume source contract", queue_contract), ("search source contract", library_contract),
        ("media-surface import gate", imports)]:
        check(name, action)
    check("extracted SQLite contracts", sql_checks, "sql-contract-execution")
    report = {"native_validation": "NOT_RUN", "scope": "static/source and extracted-SQL checks only",
        "checks": RESULTS, "passed": sum(x["status"] == "PASS" for x in RESULTS),
        "failed": sum(x["status"] == "FAIL" for x in RESULTS)}
    output = json.dumps(report, indent=2, ensure_ascii=False) + "\n"
    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(output)
    print(output)
    return 1 if report["failed"] else 0

if __name__ == "__main__":
    sys.exit(main())
