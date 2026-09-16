#!/usr/bin/env python3
"""Regression: an empty compressed TAR is never an implementation delivery."""
import io,tarfile,tempfile,unittest,hashlib
from pathlib import Path
from artifact_checks import inspect_source

class ArchiveTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.addCleanup(self.tmp.cleanup);self.path=Path(self.tmp.name)/'source.tar.gz'
    def archive(self,files):
        with tarfile.open(self.path,'w:gz') as out:
            for name,body in files.items():
                member=tarfile.TarInfo(name);member.mode=0o644;member.size=len(body);out.addfile(member,io.BytesIO(body))
    def test_empty_tar_rejected(self):
        self.archive({})
        with self.assertRaisesRegex(ValueError,'Empty TAR'):inspect_source(self.path)
    def test_readme_only_is_not_source(self):
        self.archive({'source/README.md':b'all done'})
        with self.assertRaises(ValueError):inspect_source(self.path,min_files=1)
    def test_unsafe_member_rejected(self):
        self.archive({'../escape':b'no'})
        with self.assertRaisesRegex(ValueError,'Unsafe'):inspect_source(self.path,min_files=1)
    def test_manifest_and_modes_checked(self):
        files={'source/Cargo.toml':b'[workspace]\n','source/Cargo.lock':b'version=4\n','source/crates/a/src/lib.rs':b'pub fn a() {}\n'}
        self.archive(files)
        manifest={name.removeprefix('source/'):{'bytes':len(body),'sha256':hashlib.sha256(body).hexdigest(),'mode':0o644} for name,body in files.items()}
        self.assertEqual(inspect_source(self.path,min_files=3,manifest=manifest)['files'],3)
        manifest['Cargo.toml']['mode']=0o755
        with self.assertRaisesRegex(ValueError,'Manifest mismatch'):inspect_source(self.path,min_files=3,manifest=manifest)
    def test_fonts_are_not_included(self):
        self.archive({'source/private.ttf':b'not a distributable font'})
        with self.assertRaisesRegex(ValueError,'Font'):inspect_source(self.path,min_files=1)

if __name__=='__main__':unittest.main(verbosity=2)
