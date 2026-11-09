"""Fetch the exact signed-snapshot dependency closure, checking every payload.

The committed lock was resolved by apt against GPG-verified snapshot metadata
and the pinned base image. Mirrors may supply identical bytes; never substitute
package versions or disable transport/hash verification.
"""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

BASE = 'python@sha256:ae14d4ce8771606477a81446e7ef429edd6762305fc50c84507c1041fa8285d1'
HOSTS = {'deb.debian.org', 'security.debian.org', 'snapshot.debian.org'}


def load(path):
    path=Path(path)
    if path.stat().st_size>1024*1024:raise ValueError('Debian lock exceeds 1 MiB')
    lock=json.loads(path.read_text())
    if lock.get('schema_version')!=1 or lock.get('base_image')!=BASE or lock.get('platform')!='linux/amd64':
        raise ValueError('Unsupported dependency lock/base/platform')
    packages=lock.get('packages',[]);seen=set();total=0
    if not 1<=len(packages)<=512:raise ValueError('Invalid package count')
    for package in packages:
        name=package['file'];total+=package['bytes']
        if not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9+._%~:-]{0,240}\.deb',name) or name in seen:
            raise ValueError('Unsafe or duplicate package filename')
        seen.add(name)
        if not re.fullmatch(r'[a-f0-9]{64}',package['sha256']) or not 0<package['bytes']<=256*1024*1024 or total>1024*1024*1024:
            raise ValueError('Invalid package size/hash')
        if not 1<=len(package['urls'])<=3:raise ValueError('Invalid package sources')
        for source in package['urls']:
            url=urllib.parse.urlsplit(source)
            if url.scheme!='https' or url.hostname not in HOSTS or url.port not in (None,443) or url.username or url.password or url.query or url.fragment:
                raise ValueError('Untrusted package source')
    return packages


def download(package, directory):
    destination=Path(directory)/package['file'];partial=destination.with_suffix('.partial')
    if destination.is_file() and destination.stat().st_size==package['bytes'] and hashlib.sha256(destination.read_bytes()).hexdigest()==package['sha256']:
        return
    errors=[]
    for url in package['urls']:
        for attempt in range(3):
            try:
                digest=hashlib.sha256();size=0
                with urllib.request.urlopen(url,timeout=45) as response,partial.open('wb') as target:
                    while True:
                        chunk=response.read(65536)
                        if not chunk:break
                        size+=len(chunk)
                        if size>package['bytes']:raise ValueError('Package exceeds pinned size')
                        digest.update(chunk);target.write(chunk)
                if size!=package['bytes'] or digest.hexdigest()!=package['sha256']:raise ValueError('Package checksum/size mismatch')
                partial.replace(destination)
                print(package['package']+' '+package['version']+': verified',flush=True)
                return
            except (OSError,ValueError) as error:
                partial.unlink(missing_ok=True);errors.append(str(error))
                if isinstance(error,ValueError) or isinstance(error,urllib.error.HTTPError) and error.code not in (408,429,500,502,503,504):break
                if attempt<2:time.sleep(2**attempt)
    raise RuntimeError(package['file']+': no verified download; '+ '; '.join(errors)[-1500:])


def main():
    packages=load(sys.argv[1]);destination=Path(sys.argv[2]);destination.mkdir(parents=True,exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=4) as pool:
        list(pool.map(lambda package:download(package,destination),packages))
if __name__=='__main__':main()
