#!/usr/bin/env python3
"""Run static and auxiliary gates. Never installs Rust or starts the native players.

--mechanisms uses synthetic local files/loopback curl and FFmpeg if already installed.
--browser tests actual web assets with mock transport, not the Rust HTTP server.
"""
from __future__ import annotations
import argparse,json,shutil,sys
from pathlib import Path
from runner import run


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',type=Path,required=True)
    parser.add_argument('--mechanisms',action='store_true');parser.add_argument('--browser',action='store_true')
    args=parser.parse_args();root=Path(__file__).resolve().parents[2]
    output=args.output.resolve();output.mkdir(parents=True,exist_ok=True)
    tasks=[('runner-unit-tests',[sys.executable,'scripts/audit-final/test_runner.py']),
           ('archive-unit-tests',[sys.executable,'scripts/audit-final/test_artifact_checks.py']),
           ('inherited-review-gates',[sys.executable,'scripts/review-check.py']),
           ('source-contracts',[sys.executable,'scripts/audit-final/source_checks.py',str(root),'--json',str(output/'source-checks.json')]),
           ('pending-task-contracts',[sys.executable,'scripts/audit-final/pending_tasks_checks.py',str(root),'--json',str(output/'pending-tasks.json')]),
           ('pending-task-checker-regressions',[sys.executable,'scripts/audit-final/test_pending_tasks_checks.py']),
           ('javascript-syntax',['node','--check','crates/app/big-app-kit-core/src/remote_control.js'])]
    for path in sorted((root/'scripts/audit-final').glob('*.sh')):tasks.append(('syntax-'+path.stem,['bash','-n',str(path)]))
    if args.mechanisms:tasks.append(('local-mechanisms',[sys.executable,'scripts/audit-final/mechanism_tests.py']))
    if args.browser:tasks.append(('mock-transport-browser',[sys.executable,'scripts/audit-final/browser_tests.py',str(root),str(output/'browser')]))
    results=[]
    for name,cmd in tasks:
        results.append(run(name,cmd,root,output/(name+'.log'),45))
        # Persist each actual exit status. If the review tool is interrupted,
        # completed commands remain evidence without inventing an all-PASS run.
        checkpoint=output/'gate-progress.json'
        temporary=output/'gate-progress.json.tmp'
        temporary.write_text(json.dumps({'complete':False,'commands':results},ensure_ascii=False,indent=2)+'\n')
        temporary.replace(checkpoint)

    native=['Rust compilation','cargo test','rustfmt','Clippy','msgfmt','GTK/mpv playback','D-Bus MPRIS session','GPU/Wayland/X11','Orca/AT-SPI','player benchmarks']
    report={'commands':results,'native_not_executed':native,'rust_installed':False,
            'availability':{tool:shutil.which(tool) for tool in ['rustc','cargo','rustfmt','msgfmt','node','curl','ffmpeg','ffprobe','chromium']},
            'scope':'Status-based results; mock/mechanism tests do not test native Rust paths',
            'passed':sum(r['status']=='PASS' for r in results),'failed':sum(r['status'] in ['FAIL','TIMEOUT'] for r in results),'blocked':sum(r['status']=='BLOCKED' for r in results)}
    (output/'gate-summary.json').write_text(json.dumps(report,ensure_ascii=False,indent=2)+'\n')
    print(json.dumps(report,ensure_ascii=False,indent=2));return 1 if report['failed'] else 77 if report['blocked'] else 0
if __name__=='__main__':raise SystemExit(main())
