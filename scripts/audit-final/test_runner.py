#!/usr/bin/env python3
import sys,tempfile,unittest
from pathlib import Path
from runner import run

class RunnerTests(unittest.TestCase):
    def check_child(self,source,timeout=3):
        with tempfile.TemporaryDirectory() as td:
            return run('test',[sys.executable,'-c',source],Path(td),Path(td)/'log',timeout)
    def test_printed_pass_with_nonzero_exit_fails(self):
        r=self.check_child("print('PASS');raise SystemExit(17)");self.assertEqual((r['status'],r['exit_code']),('FAIL',17))
    def test_zero_exit_passes(self):self.assertEqual(self.check_child("print('done')")['status'],'PASS')
    def test_missing_executable_is_blocked(self):
        with tempfile.TemporaryDirectory() as td:
            r=run('missing',['/deliberately-unavailable/audit-tool'],Path(td),Path(td)/'log')
            self.assertEqual(r['status'],'BLOCKED');self.assertIsNone(r['exit_code'])
    def test_explicit_blocked_exit_is_not_pass(self):self.assertEqual(self.check_child('raise SystemExit(77)')['status'],'BLOCKED')
    def test_deadline_is_not_pass(self):self.assertEqual(self.check_child('import time;time.sleep(30)',.1)['status'],'TIMEOUT')
    def test_child_output_is_logged(self):
        with tempfile.TemporaryDirectory() as td:
            log=Path(td)/'log';r=run('data',[sys.executable,'-c',"print('result')"],Path(td),log)
            self.assertEqual(r['status'],'PASS');self.assertIn('result',log.read_text())

if __name__=='__main__':unittest.main(verbosity=2)
