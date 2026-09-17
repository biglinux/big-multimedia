#!/usr/bin/env python3
"""Broad syntax/lexical/source contracts. No Rust parsing, compilation or native safety proof."""
from __future__ import annotations
import argparse,ast,json,re,subprocess,tomllib,xml.etree.ElementTree as ET
from pathlib import Path

from source_text import Source

def check(root: Path) -> dict:
    results=[]
    def record(name,func,scope='static/source'):
        try:detail=func();status='PASS'
        except ImportError as e:detail=str(e);status='BLOCKED'
        except Exception as e:detail=str(e);status='FAIL'
        results.append({'name':name,'status':status,'scope':scope,'detail':detail})
    def text(name):
        raw=(root/name).read_text()
        # Rust sources are rustfmt-formatted, so contracts match wrapping-independently.
        return Source(raw) if name.endswith('.rs') else raw
    def ordered(source,*needles):
        # Missing tokens are an explicit failing contract, not an uncaught
        # ValueError that prevents the remaining independent gates from running.
        positions=[source.find(needle) for needle in needles]
        return all(position>=0 for position in positions) and positions==sorted(positions)
    def must(condition,message):
        if not condition:raise AssertionError(message)
    def manifests():
        paths=[p for p in root.rglob('*.toml') if '.git' not in p.parts and 'target' not in p.parts]
        for p in paths:tomllib.loads(p.read_text())
        tomllib.loads(text('Cargo.lock'));return {'toml_files':len(paths),'lock':'syntax only; no dependency resolution'}
    def xml():
        paths=[p for p in (root/'crates').rglob('*') if p.suffix in ['.xml','.ui','.svg']]
        for p in paths:ET.parse(p)
        return {'files':len(paths),'limit':'Not GtkBuilder type/property validation'}
    def rust_lexical():
        from pygments.lexers import RustLexer
        from pygments.token import Error,Comment,String
        paths=list((root/'crates').rglob('*.rs'));failures=[]
        for path in paths:
            stack=[]
            for off,tok,value in RustLexer().get_tokens_unprocessed(path.read_text()):
                if tok in Error:failures.append((str(path.relative_to(root)),off,'lexer-error'))
                if tok in Comment or tok in String:continue
                for index,char in enumerate(value):
                    if char in '([{':stack.append(char)
                    elif char in ')]}':
                        if not stack or stack.pop()!={')':'(',']':'[','}':'{'}[char]:failures.append((str(path.relative_to(root)),off+index,'delimiter'))
            if stack:failures.append((str(path.relative_to(root)),len(path.read_text()),'unclosed'))
        must(not failures,repr(failures[:15]));return {'files':len(paths),'limit':'Pygments tokens/delimiters only, NOT Rust parser/type checker'}
    def internal_paths():
        configs=list(root.rglob('.cargo/config.toml'))+list(root.rglob('.cargo/config'))
        for p in configs:must('/mnt/data/' not in p.read_text(),str(p)+' pins executor path')
        for p in [root/'Cargo.toml',*root.glob('crates/**/Cargo.toml')]:must('/mnt/data/' not in p.read_text(),str(p))
        return 'No executor-specific cargo configuration or dependency path'
    def contract(name,checks):
        def action():
            for condition,message in checks:must(condition,message)
            return {'conditions':len(checks),'limit':'Source tokens/order only; no semantic proof'}
        record(name,action)
    record('TOML/lock parsing',manifests)
    record('XML/UI/SVG parsing',xml)
    record('Rust lexical delimiters',rust_lexical,'lexical only')
    record('Portable cargo source',internal_paths)
    proc=text('crates/foundation/big-os-kit/src/subprocess/linux_exec.rs');reaper=text('crates/foundation/big-os-kit/src/subprocess/reaper.rs')
    contract('Complete bounded Linux subprocess ownership',[
        ('WNOWAIT' in proc,'leader not retained'),('OutputLimit' in proc,'silent truncation'),
        ('.wait(' not in proc and '.join(' not in proc,'blocking cleanup'),
        ('scope.spawn' not in proc and 'Builder::new' not in proc,'per-stream threads'),
        (proc.index('Permit::reserve')<proc.index('command.spawn'),'permit reserved too late'),
        ('const MAX_CHILDREN: usize = 128' in reaper,'collector unbounded'),
        ('stdout.is_none() && stderr.is_none()' in proc,'completion ignores inherited pipes')])
    storage=text('crates/foundation/big-os-kit/src/storage/transaction.rs');settings=text('crates/foundation/big-media-data/src/settings/store.rs')
    contract('Publication and settings receipts',[
        ('PublishedNotSynced' in storage,'durability receipt missing'),
        ('fs::hard_link(&self.path, target)' in storage,'no-replace publication missing'),
        ('fs::remove_file(target)' not in storage,'deletes public target'),
        (ordered(settings[settings.index('pub fn try_flush'):], 'WriteLock::try_acquire', 'read_regular_limited', '!= expected', 'atomic_write_private_receipt'), 'review comparison/publication not in one transaction'),
        ('durability_pending' in settings,'receipt retry lost'),('load_error' in settings,'bad read not blocked')])
    shots=text('crates/media-runtime/src/player/mpv_backend/screenshots.rs');client=text('crates/media-runtime/src/player/mpv_backend/backend/client.rs');backend=text('crates/media-runtime/src/player/mpv_backend/backend/mod.rs')
    withdrawal=shots.split('fn withdraw_expired',1)[1].split('pub(super) fn complete',1)[0]
    contract('Screenshot terminal ownership',[
        ('requests.remove' not in withdrawal,'timeout deletes live writer'),
        ('request.stage.publish' in shots,'no staged publication'),
        ('clear_after_termination' in shots and 'clear_after_termination' in backend,'termination cleanup absent'),
        (backend.rfind('.terminate_destroy()') < backend.rfind('.screenshots.clear_after_termination()'),'staging cleaned before native termination'),
        ('MPV_EVENT_COMMAND_REPLY' in client,'command replies lost')])
    props=text('crates/app/big-app-kit/src/mpris/properties.rs');changing=props.split('CHANGING:',1)[1].split('];',1)[0]
    contract('MPRIS authoritative state',[
        ('"Position"' not in changing,'normal progress wrongly signals PropertiesChanged'),
        ('"CanControl"' not in changing,'intrinsic capability changes'),
        ('params.get::<(ObjectPath, i64)>()' in props,'TrackId loses its type'),
        ('state.track_id.as_deref() == Some(id)' in props,'stale TrackId applied'),
        ('PlayerEvent::Seeked' in text('crates/big-audio-player/src/audio/ui/audio_shell/engine.rs'),'native seek not routed')])
    process=text('crates/media-runtime/src/converter/queue/processing.rs');job=text('crates/media-runtime/src/converter/queue/job.rs');validate=text('crates/media-runtime/src/converter/queue/validation.rs')
    receipts=text('crates/media-runtime/src/converter/queue/receipts.rs')
    contract('Export validation and ownership',[
        ('std::fs::remove_file' not in process,'processing unlinks a final/original path'),
        ('if delete_source' in process and 'Originals are kept' in process,'implicit original deletion'),
        ('validate_output_cancellable' in process and 'receipts[index].publish' in process and 'stage.publish' in receipts,'publication without validation and receipt'),
        ('attached_pic' in validate,'cover can satisfy a real-video plan'),
        ('selected.len() != usize::from' in validate,'expected stream count not enforced'),
        ('run_cancellable_with_stderr' in job,'FFmpeg bypasses managed process ownership'),
        ('"--"' in process,'concat final path not separated')])
    http=text('crates/foundation/big-os-kit/src/http_client.rs');transport=text('crates/foundation/big-os-kit/src/http_client/transport.rs')
    contract('Private HTTP terminal receipts',[
        ('"--disable", "--silent"' in http,'curlrc not disabled first'),
        ('"--config", "-"' in http and '"data-raw"' in http,'private text data not literal stdin'),
        ('reader: Box::new(reader)' in http,'prefetched bytes discarded'),
        ('status.success()' in transport and 'try_wait()' in transport,'EOF not confirmed'),
        ('.wait(' not in transport,'HTTP error cleanup blocks')])
    remote=text('crates/app/big-app-kit-core/src/remote_control.rs')
    contract('Remote security boundaries',[
        ('format!("/{token}/")' in remote,'token prefix collision'),
        ('request.method != "POST"' in remote and 'x-big-media-remote' in remote,'mutating GET'),
        ('fallback' not in remote.split('fn make_token',1)[1].split('fn hex',1)[0],'predictable token fallback'),
        ('finite(status_snapshot.position_s)' in remote,'NaN JSON'),
        ('stream.set_nonblocking(true)' in remote and 'HEADER_DEADLINE' in remote and 'WRITE_DEADLINE' in remote,'unbounded accepted client')])
    return {'scope':'Static/lexical checks only; no Rust build, native UI or driver execution', 'checks':results,
            'passed':sum(r['status']=='PASS' for r in results),'failed':sum(r['status']=='FAIL' for r in results),
            'blocked':sum(r['status']=='BLOCKED' for r in results)}

def main():
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('source',type=Path);p.add_argument('--json',type=Path);a=p.parse_args()
    result=check(a.source.resolve());data=json.dumps(result,ensure_ascii=False,indent=2)+'\n'
    if a.json:a.json.write_text(data)
    print(data);return 1 if result['failed'] else 77 if result['blocked'] else 0
if __name__=='__main__':raise SystemExit(main())
