#!/usr/bin/env python3
"""Static linkage/order contracts for the pending-work implementation.

Not a Rust parser, ABI check, runtime test or formal concurrency proof. Negative
mutation tests ensure the SPECIFIED guards cannot be removed unnoticed. Native
regressions are present in Rust source and have to run on a prepared target.
"""
from __future__ import annotations
import argparse,ast,json,re
from pathlib import Path

PATHS={
 'settings':'crates/foundation/big-media-data/src/settings/store.rs',
 'deferred':'crates/foundation/big-os-kit/src/deferred.rs',
 'background':'crates/foundation/big-os-kit/src/background.rs',
 'saving':'crates/media-runtime/src/settings/saving.rs',
 'keyring':'crates/foundation/big-os-kit/src/keyring.rs',
 'accounts':'crates/foundation/big-media-data/src/library/accounts.rs',
 'editor':'crates/big-audio-player/src/audio/account_editor.rs',
 'remote':'crates/app/big-app-kit-core/src/remote_control.rs',
 'scope':'crates/ui/media/big-player-ui/src/smartphone/dialog.rs',
 'receipts':'crates/media-runtime/src/converter/queue/receipts.rs',
 'processing':'crates/media-runtime/src/converter/queue/processing.rs',
 'unavailable':'crates/foundation/big-media-data/src/db/unavailable.rs',
 'review_ui':'crates/big-audio-player/src/audio/ui/unavailable_dialog.rs',
 'scanner':'crates/big-audio-player/src/audio/library_scan_engine.rs',
 'scan_ui':'crates/big-audio-player/src/audio/ui/scan_progress.rs',
 'scan_data':'crates/foundation/big-media-data/src/library/scanner.rs',
 'metadata':'crates/foundation/big-media-data/src/library/metadata.rs',
 'playlist':'crates/big-audio-player/src/audio/ui/audio_shell/playlist_library_dialog_actions.rs',
 'playlist_parser':'crates/big-audio-player/src/audio/playlist_import.rs',
 'queue':'crates/media-runtime/src/converter/queue/mod.rs',
 'messages':'crates/media-runtime/src/settings/messages.rs',
}
def ordered(text,*needles):
 cursor=0
 for needle in needles:
  found=text.find(needle,cursor)
  if found<0:return False
  cursor=found+len(needle)
 return True

def after(text,marker):
 if marker not in text:raise AssertionError('missing section '+marker)
 return text.split(marker,1)[1]

def catalogue(text):
 values={};key=None;value=None;field=None
 for line in text.splitlines():
  if line.startswith('msgid '):
   if key is not None:
    if key in values:raise AssertionError('duplicate msgid '+repr(key))
    values[key]=value or ''
   key=ast.literal_eval(line[6:]);value='';field='key'
  elif line.startswith('msgstr '):value=ast.literal_eval(line[7:]);field='value'
  elif line.startswith('"') and key is not None:
   part=ast.literal_eval(line)
   if field=='key':key+=part
   elif field=='value':value+=part
 if key is not None:
  if key in values:raise AssertionError('duplicate msgid '+repr(key))
  values[key]=value or ''
 return values

def check(root:Path,overrides:dict[str,str]|None=None):
 overrides=overrides or {};results=[]
 def read(key):
  name=PATHS.get(key,key)
  return overrides[name] if name in overrides else (root/name).read_text()
 def gate(name,callback):
  try:detail=callback();status='PASS'
  except Exception as exc:status='FAIL';detail=str(exc)
  results.append({'name':name,'status':status,'scope':'textual contracts, NOT native Rust execution','detail':detail})
 def require(condition,message):
  if not condition:raise AssertionError(message)

 def persistence():
  bg=read('background');settings=read('settings');defer=read('deferred')
  require('CAPACITY: usize = 64' in bg and 'sync_channel::<Job>(8)' in bg,'disk/service lanes not bounded')
  require('queue.try_send' in bg and 'catch_unwind' in bg,'nonblocking admission/cleanup missing')
  require('PublishedNotSynced' in settings and 'inner.revision != revision' in settings,'acknowledgement loses newer state')
  require(ordered(after(settings,'pub fn try_flush'),'WriteLock::try_acquire','read_regular_limited','!= expected','atomic_write_private_receipt'),'main settings compare/publication not under one lock')
  require(ordered(after(defer,'fn transaction'),'WriteLock::try_acquire','read_regular_limited','!=expected','atomic_write_private_receipt'),'independent document compare/publication weakened')
  require('s.queued' in defer and 's.desired' in defer and 's.durable_revision' in defer,'coalesced generation owner absent')
  require('pub mod saving;' in read('crates/media-runtime/src/settings/mod.rs'),'external async helper inaccessible')
  require('save_status()' in read('saving') and 'connect_unmap' in read('saving'),'UI has no acknowledged status/lifecycle')
  return 'Bounded workers; compare under lock; generation receipt; mapped status and retry.'
 gate('Async saves and acknowledgement',persistence)

 def vault():
  k=read('keyring');a=read('accounts');save=after(a,'fn save_reviewed_with');finish=after(a,'fn finish')
  require('builder.stdin(value.to_vec())' in k and 'MAX_SECRET_BYTES: usize = 7900' in k,'stdin or helper size bound lost')
  require('result.status.success()' in k and 'result.stdout' in k,'helper result not checked')
  require(ordered(save,'WriteLock::try_acquire','current!=review.bytes','atomic_write_private_receipt(&path','vault.store','let readback=vault.lookup','readback!=payload','atomic_write_private_receipt(&review.path'),'journal/readback/publish ordering lost')
  require(ordered(finish,'reference(&current','vault.lookup','payload_hash','sync_parent','vault.clear','fs::remove_file(journal_path'),'cleanup precedes reference/readback/durability confirmation')
  require('ProtectedCleanupPending' in a and 'RecoveryRequired' in a and 'payload_hash' in a,'uncertain recovery state missing')
  require('editor_seed' in a and 'save_reviewed' in read('editor'),'retry/editor not wired to reviewed transaction')
  return 'No credential argv; immutable referenced candidate; recoverable non-secret journal; exact reviewed state.'
 gate('Secret Service migration and recovery',vault)

 def remote():
  text=read('remote');serve=after(text,'fn serve(')
  for bound in ['MAX_CLIENTS: usize = 16','MAX_PER_IP: usize = 4','MAX_HEADER: usize = 8192','MAX_HEADERS: usize = 48','Duration::from_secs(3)','Duration::from_secs(2)','MAX_RATE_PEERS: usize = 64']:
   require(bound in text,'missing remote bound '+bound)
  require('set_nonblocking(true)' in text and 'now.duration_since(client.accepted) >= HEADER_DEADLINE' in serve,'absolute header budget lost')
  require('start.elapsed() >= WRITE_DEADLINE' in serve,'response deadline lost')
  require('rate_limited_mutation(&request, token)' in serve and 'global.allow(now, 24)' in serve,'authenticated/per-client/global admission missing')
  require('stop.load(Ordering::Acquire)' in serve and 'impl Drop for ServerHandle' in text and 'wake.unpark()' in text,'revocation/lifecycle missing')
  require('start_scoped' in read('scope'),'GUI does not share revocation')
  return '16 sockets, 4/IP, absolute 3s header and 2s output deadlines; no per-client threads.'
 gate('Bounded LAN server lifecycle',remote)

 def receipts():
  text=read('receipts');processing=read('processing');publish=after(text,'fn publish(')
  require('MAX_RECEIPT:usize=4096' in text and 'deny_unknown_fields' in text,'receipt input bound/schema missing')
  require(ordered(after(text,'fn completed('),'receipt.plan!=self.plan','hash_file(&self.target,cancel)?!=receipt.sha256','validate_output_cancellable','hash_file(&self.target,cancel)?!=receipt.sha256'),'resume proof or post-probe identity missing')
  require(ordered(publish,'self.persist(&record)?','cancel.load','stage.publish(&self.target)','hash_file(&self.target,cancel)','published:true'),'prepared receipt not durable before publication')
  require('external_path' in text and 'hash_file(path,cancel)?' in text,'external subtitle fingerprint missing')
  require(processing.count('recheck_plan(')>=3 and 'reused.iter().all' in processing,'all-reused/published output path skips input-plan recheck')
  require('std::fs::remove_file' not in processing,'processing deletes source or already public output')
  return 'Full source/plan/output fingerprints, structure probe, prepared journal, no-replace publication.'
 gate('Durable completion receipts',receipts)

 def unavailable():
  text=read('unavailable');remove=after(text,'pub fn remove_reviewed');ui=read('review_ui')
  require('MAX_SELECTED:usize=100' in text and 'MAX_CHECKED:usize=2048' in text,'review is not paged/bounded')
  require(ordered(remove,'unchecked_transaction','availability(&item.track.file_path)!=Availability::Missing','current.as_ref()!=Some(&item.track)','DELETE FROM playlist_tracks','DELETE FROM tracks','stop(cancel)?;tx.commit()?'),'review revalidation/transaction lost')
  require('memberships!=item.memberships' in remove and 'queued!=item.queued' in remove,'playlist/queue relationships not rechecked')
  require('remove_file' not in text and 'remove_dir' not in text,'review can delete media files')
  require('set_default_response(Some("cancel"))' in ui and 'check.is_active()' in ui and 'check.set_active(true)' not in ui,'destructive consent is not explicit')
  return 'Bounded pages, no implicit selection, exact row/relationship recheck, DB-only deletion.'
 gate('Unavailable library review',unavailable)

 def scan():
  s=read('scanner');data=read('scan_data');metadata=read('metadata');ui=read('scan_ui')
  require('ScanAdmission' in s and 'MAX_SCANS' in s and 'cancelled.load' in s,'scan admission/cancellation absent')
  require('db.try_lock()' in s and 'read_track_metadata_cancellable' in s,'blocked DB or probes do not observe cancellation')
  require('AtomicBool' in data and ordered(after(data,'pub fn visit_audio_files_cancellable'),'for entry in','cancelled.load','visit(entry.path())'),'directory walk cancellation missing')
  require('run_cancellable' in metadata and 'Duration::from_secs(30)' in metadata,'metadata probe is not supervised')
  require('control.finished' in ui and 'control.cancelled' in ui,'progress/terminal cancel not wired')
  return 'Cooperative directory/probe/DB cancellation, bounded scans, confirmed-import progress.'
 gate('Cancellable library scanning',scan)

 def playlists():
  s=read('playlist');parser=read('playlist_parser')
  require('create_playlist_with_tracks' in s and 'read_regular_limited' in parser,'bounded parse / transactional import missing')
  require('PlaylistImported(result.map_err' in s and 'PlaylistReadFailed' in s,'result is not acknowledged')
  require('get_playlist_tracks(conn, id).unwrap_or_default' not in s and 'get_all_playlists(conn)\n                    .unwrap_or_default' not in s,'read error becomes an empty queue/list')
  require('acknowledged_task' in s and 'select_folder_optional' in s,'export/import not off main loop')
  return 'Confirmed transaction and errors kept distinct from empty playlist.'
 gate('Playlist document and SQL feedback',playlists)

 def lifecycle():
  q=read('queue');saving=read('saving');bg=read('background')
  require('running.compare_exchange' in q and 'let run = QueueRun' in q,'parallel starts can reset cancellation')
  require('track_operation()' in q and 'track_operation()' in read('scanner') and 'OperationGuard' in bg,'long durable jobs not accounted in close drain')
  require('impl Drop for ConversionQueue' in q and 'self.sender.close()' in q,'closed queue strands workers/readers')
  require('std::thread::panicking()' in q and 'self.sender.close()' in after(q,'struct QueueRun'),'worker unwind loses terminal state')
  require('after_writes' in saving and 'app.hold()' in saving,'shutdown drain missing')
  require('unsaved_changes()' in saving and 'unsaved_dialog()' in saving and 'set_default_response(Some("keep"))' in saving,'unconfirmed writes can close without review/consent')
  require('Settings::any_unsaved()' in saving and 'big_os_kit::deferred::any_unsaved()' in saving,'close omits a live store category')
  return 'Normal-close/suspend drains owned work and asks before abandoning unsaved state; forced termination not guaranteed.'
 gate('Owned jobs and shutdown',lifecycle)

 def messages():
  src=read('messages');msgs=[json.loads(x) for x in re.findall(r'gettext_noop\(("(?:[^"\\]|\\.)*")\)',src)]
  pot=catalogue(read('crates/media-runtime/po/big-media-runtime.pot'));pt=catalogue(read('crates/media-runtime/po/pt_BR.po'))
  require(msgs and len(msgs)==len(set(msgs)),'catalog anchors empty or duplicated')
  missing=[x for x in msgs if x not in pot or not pt.get(x)]
  require(not missing,'missing POT/PT-BR messages: '+repr(missing[:6]))
  files=read('crates/media-runtime/po/POTFILES.in').splitlines()
  for key in ['messages','editor','scan_ui','review_ui','saving']:
   require(PATHS[key] in files,'new UI missing from POTFILES: '+key)
  require('mod messages;' in read('crates/media-runtime/src/settings/mod.rs'),'dynamic extraction module disconnected')
  return {'anchored_messages':len(msgs),'translated':'PT-BR; manual AI translation, no human certification/msgfmt'}
 gate('Translation coverage of new states',messages)
 return {'results':results,'passed':sum(r['status']=='PASS' for r in results),'failed':sum(r['status']=='FAIL' for r in results),'native_executed':False}

def main():
 p=argparse.ArgumentParser(description=__doc__);p.add_argument('root',nargs='?',type=Path,default=Path(__file__).resolve().parents[2]);p.add_argument('--json',type=Path);args=p.parse_args()
 result=check(args.root.resolve());data=json.dumps(result,ensure_ascii=False,indent=2)+'\n';print(data)
 if args.json:args.json.write_text(data)
 return 1 if result['failed'] else 0
if __name__=='__main__':raise SystemExit(main())
