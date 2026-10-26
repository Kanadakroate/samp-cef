"""Run a bounded isolated native CEF/renderer bridge test, never GTA or a server."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--cef-runtime', required=True, type=Path)
parser.add_argument('--sound', required=True, type=Path)
parser.add_argument('--output', required=True, type=Path)
args = parser.parse_args()
repo = Path(__file__).resolve().parents[2]
output = args.output.resolve()
output.mkdir(exist_ok=False)
runtime = output/'runtime'
runtime.mkdir()
names = ['libcef.dll', 'chrome_elf.dll', 'icudtl.dat', 'v8_context_snapshot.bin',
         'chrome_100_percent.pak', 'chrome_200_percent.pak', 'resources.pak',
         'd3dcompiler_47.dll', 'libEGL.dll', 'libGLESv2.dll']
for name in names:
    shutil.copy2(args.cef_runtime/name, runtime/name)
shutil.copytree(args.cef_runtime/'locales', runtime/'locales')
release = repo/'target/i686-pc-windows-msvc/release'
shutil.copy2(release/'cef-renderer.exe', runtime/'cef-renderer.exe')
shutil.copy2(release/'examples/notification_bridge_probe.exe', runtime/'probe.exe')
bundle = 'a'*64  # Private fixture identity, not a production manifest.
assets = runtime/'assets/liberty-revival'/bundle/'lr_progression_toast'
assets.mkdir(parents=True)
shutil.copy2(args.sound, assets/'achievement-complete.ogg')
shutil.copy2(repo/'integration/notification-audio.js', assets/'notification-audio.js')
(assets/'index.html').write_text('''<!doctype html><meta charset="utf-8"><title>Private native audio audit</title>
<script src="notification-audio.js"></script><script>
const player=LRNotificationAudio.create(window);
if(!player)throw Error('Native API missing');
player.load(); let index=0;
player.addEventListener('ended',()=>cef.emit('probe_result',JSON.stringify({clip:++index,ended:true})));
player.addEventListener('error',()=>cef.emit('probe_result',JSON.stringify({error:true})));
for(const seconds of [2,7,12,17])setTimeout(()=>player.play().catch(()=>{}),seconds*1000);
</script>''', encoding='utf-8')
hashes = {name: hashlib.sha256((runtime/name).read_bytes()).hexdigest()
          for name in [*names, 'cef-renderer.exe', 'probe.exe']}
(output/'input-hashes.json').write_text(json.dumps(hashes,indent=2))
with (output/'process.log').open('wb') as log:
    result = subprocess.run([str(runtime/'probe.exe'),str(runtime),
        f'sampcef://assets/liberty-revival/{bundle}/lr_progression_toast/index.html',str(output/'measurement')],
        cwd=runtime, stdout=log,stderr=subprocess.STDOUT,creationflags=subprocess.CREATE_NO_WINDOW,timeout=40)
assert result.returncode == 0, (result.returncode, 'inspect process.log')
rows=[json.loads(line) for line in (output/'measurement/results.jsonl').read_text().splitlines()]
assert rows == [{'clip':i,'ended':True} for i in range(1,5)], rows
print(json.dumps({'nativeRendererBridge':True,'completions':rows,'gtaLoaded':False,'installed':False}))
