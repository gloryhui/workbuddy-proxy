import os,sys
from pathlib import Path
root=Path('/Users/shile/Downloads/git/workbuddy-proxy-ui-redesign')
ev=Path(__file__).resolve().parent
cap=Path('/Users/shile/Library/Caches/workbuddy-proxy-ui-redesign/01a153a0-15c3-7b61-a9e4-834a1ea9702e/tmp/d15-build-001')
assert not cap.exists()
cap.mkdir();(cap/'owner.txt').write_text('Codex D15 2026-10-20\n')
os.environ.update(TMPDIR=str(cap),TMP=str(cap),TEMP=str(cap),CARGO_TARGET_DIR=str(root/'gpui/target'),RUSTUP_TOOLCHAIN='stable')
import subprocess,hashlib
try:
 for label,args in [('fmt',['fmt','--manifest-path','gpui/Cargo.toml','--check']),('tests',['test','--manifest-path','gpui/Cargo.toml']),('build',['build','--release','--manifest-path','gpui/Cargo.toml'])]:
  with (ev/'logs'/f'{label}.txt').open('wb') as log:
   subprocess.run(['/Users/shile/.cargo/bin/cargo',*args],cwd=root,stdout=log,stderr=subprocess.STDOUT,check=True)
  print(label+' PASS',flush=True)
 (ev/'logs/binary.txt').write_text(hashlib.sha256((root/'gpui/target/release/workbuddy-proxy-gpui').read_bytes()).hexdigest()+'\n')
finally:
 subprocess.run([sys.executable,str(ev/'cleanup_owned.py'),str(cap),str(ev/'logs/cleanup-build.json')],check=True)
