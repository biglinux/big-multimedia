#!/usr/bin/env python3
"""Regression tests of the STATIC checker. They do not execute Rust or GTK."""
from pathlib import Path
import unittest
from pending_tasks_checks import PATHS,check
ROOT=Path(__file__).resolve().parents[2]

class ContractCheckerTests(unittest.TestCase):
    def test_current_tree_satisfies_nine_contracts(self):
        result=check(ROOT)
        self.assertEqual(result['failed'],0,result)
        self.assertEqual(result['passed'],9)
        self.assertFalse(result['native_executed'])

    def mutated(self,key,old,new,gate):
        original=(ROOT/PATHS[key]).read_text()
        self.assertIn(old,original,('mutation no longer matches source',key,old))
        mutated=original.replace(old,new)
        result=check(ROOT,{PATHS[key]:mutated})
        group=next(item for item in result['results'] if item['name']==gate)
        self.assertEqual(group['status'],'FAIL',group)

CASES=[
 ('generation','settings','inner.revision != revision','false','Async saves and acknowledgement'),
 ('blocking_queue','background','queue.try_send','queue.send','Async saves and acknowledgement'),
 ('secret_stdin','keyring','builder.stdin(value.to_vec())','builder.arg("secret")','Secret Service migration and recovery'),
 ('vault_readback','accounts','let readback=vault.lookup','let readback=unverified_value','Secret Service migration and recovery'),
 ('vault_durability','accounts','storage::sync_parent(&review.path)?;','/* not synced */','Secret Service migration and recovery'),
 ('sliding_header_timeout','remote','now.duration_since(client.accepted) >= HEADER_DEADLINE','now.duration_since(Instant::now()) >= HEADER_DEADLINE','Bounded LAN server lifecycle'),
 ('socket_bound','remote','MAX_CLIENTS: usize = 16','MAX_CLIENTS: usize = 100000','Bounded LAN server lifecycle'),
 ('resume_hash','receipts','hash_file(&self.target,cancel)?!=receipt.sha256','false','Durable completion receipts'),
 ('prepared_journal','receipts','self.persist(&record)?;','/* no prepared receipt */','Durable completion receipts'),
 ('review_recheck','unavailable','current.as_ref()!=Some(&item.track)','false','Unavailable library review'),
 ('media_unlink','unavailable','stop(cancel)?;tx.commit()?;', 'std::fs::remove_file("media");stop(cancel)?;tx.commit()?;', 'Unavailable library review'),
 ('walk_cancel','scan_data','cancelled.load','ignored.load','Cancellable library scanning'),
 ('sql_error_to_empty','playlist','queries::get_playlist_tracks(conn, id),','queries::get_playlist_tracks(conn, id).unwrap_or_default(),','Playlist document and SQL feedback'),
 ('parallel_restart','queue','running.compare_exchange','running.swap','Owned jobs and shutdown'),
 ('silent_unsaved_close','saving','set_default_response(Some("keep"))','set_default_response(Some("discard"))','Owned jobs and shutdown'),
 ('catalog_disconnected','messages','gettext_noop(', 'unextracted_literal(', 'Translation coverage of new states'),
]

def make_test(key,old,new,group):
    def test(self): self.mutated(key,old,new,group)
    return test
for name,key,old,new,group in CASES:
    setattr(ContractCheckerTests,'test_rejects_'+name,make_test(key,old,new,group))
if __name__=='__main__':unittest.main(verbosity=2)
