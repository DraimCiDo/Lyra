import hashlib
import io
import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import urllib.error
import install_debian as d

class Installer(unittest.TestCase):
    def test_lock_has_pinned_complete_sources(self):
        records=d.load(Path(__file__).with_name('debian.lock.json'))
        self.assertEqual(len(records),190)
        self.assertEqual(len({p['file'] for p in records}),190)
    def test_unsafe_paths_and_origins_are_rejected(self):
        lock=json.loads(Path(__file__).with_name('debian.lock.json').read_text())
        with tempfile.TemporaryDirectory() as folder:
            path=Path(folder)/'lock.json'
            lock['packages'][0]['file']='../unsafe.deb';path.write_text(json.dumps(lock))
            with self.assertRaises(ValueError):d.load(path)
            lock['packages'][0]['file']='valid.deb';lock['packages'][0]['urls']=['https://localhost/unsafe'];path.write_text(json.dumps(lock))
            with self.assertRaises(ValueError):d.load(path)
    def test_verified_fallback_and_bad_hash_never_install_payload(self):
        content=b'trusted package fixture'
        package={'package':'fixture','version':'1','file':'fixture.deb','bytes':len(content),'sha256':hashlib.sha256(content).hexdigest(),'urls':['https://deb.debian.org/fake','https://snapshot.debian.org/fake']}
        with tempfile.TemporaryDirectory() as folder:
            with patch.object(d.urllib.request,'urlopen',side_effect=[urllib.error.HTTPError(package['urls'][0],404,'missing',{},None),io.BytesIO(content)]):d.download(package,folder)
            self.assertEqual((Path(folder)/'fixture.deb').read_bytes(),content)
            (Path(folder)/'fixture.deb').unlink()
            with patch.object(d.urllib.request,'urlopen',side_effect=lambda *a,**k:io.BytesIO(b'corrupt')):
                with self.assertRaises(RuntimeError):d.download(package,folder)
            self.assertFalse((Path(folder)/'fixture.deb').exists())
if __name__=='__main__':unittest.main()
