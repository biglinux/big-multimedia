#!/usr/bin/env python3
"""Exit-status based gate runner. Never interpret a 'PASS' printed by a child as success."""
from __future__ import annotations
import os,signal,subprocess,time
from pathlib import Path
from typing import Sequence

def run(name: str, argv: Sequence[str], cwd: Path, log: Path, timeout: float = 45) -> dict:
    log.parent.mkdir(parents=True, exist_ok=True)
    start=time.monotonic();code=None;status='FAIL'
    with log.open('wb') as out:
        try:
            process=subprocess.Popen(list(argv),cwd=cwd,stdout=out,stderr=subprocess.STDOUT,start_new_session=True)
        except FileNotFoundError as error:
            out.write(str(error).encode());status='BLOCKED'
        else:
            try:
                code=process.wait(timeout=timeout)
                status='PASS' if code==0 else ('BLOCKED' if code==77 else 'FAIL')
            except subprocess.TimeoutExpired:
                # Polling wait did not collect a live child; its leader owns PGID.
                try:os.killpg(process.pid,signal.SIGKILL)
                except ProcessLookupError:pass
                try:code=process.wait(timeout=3)
                except subprocess.TimeoutExpired:code=None
                status='TIMEOUT'
    return {'name':name,'argv':list(argv),'cwd':str(cwd),'status':status,'exit_code':code,
            'elapsed_seconds':round(time.monotonic()-start,3),'log':str(log)}
