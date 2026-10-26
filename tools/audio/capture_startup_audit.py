"""Additional native control for CEF's audible-dependent capture lifecycle.

Uses the previously built isolated CEF harness, never GTA or a server. It
prewarms capture once, stops the pilot, then plays four copies of the original
OGG. Prediction: first clip remains warm, later clips after idle restart capture.
The pilot belongs only to the bounded diagnostic document, not product code.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--experiment-root', type=Path, required=True)
parser.add_argument('--run-name', required=True)
args = parser.parse_args()
root = args.experiment_root.resolve()
assert args.run_name and all(c.isalnum() or c in '-_' for c in args.run_name)
runtime = root / 'runtime'
exe = runtime / 'capture_lifetime_probe.exe'
expected = json.loads((root / 'input-hashes.json').read_text())
for name, sha in expected.items():
    assert hashlib.sha256((runtime/name).read_bytes()).hexdigest() == sha, name
out = root / args.run_name
assert not out.exists(), 'Preserve earlier measurements; choose a new run name.'
out.mkdir()
baseline = (runtime / 'assets/baseline.html').read_text(encoding='utf-8')
assert baseline.count('const mode="baseline";') == 1
assert baseline.count("if(mode==='warm')") == 1
assert baseline.count('window.pilot={context,oscillator,gain};') == 1
prime = baseline.replace('const mode="baseline";', 'const mode="prime";').replace(
    "if(mode==='warm')", "if(mode==='prime')").replace(
    'window.pilot={context,oscillator,gain};',
    'window.pilot={context,oscillator,gain};\n  setTimeout(()=>{oscillator.stop();context.close();},1000);')
fixture = runtime / 'assets/prime.html'
fixture.write_text(prime, encoding='utf-8')
with (out/'process.log').open('wb') as log:
    completed = subprocess.run([str(exe), str(runtime), 'sampcef://assets/prime.html', str(out)],
                               cwd=runtime, stdout=log, stderr=subprocess.STDOUT,
                               creationflags=subprocess.CREATE_NO_WINDOW, timeout=40)
assert completed.returncode == 0, f'Harness exit code {completed.returncode}; inspect process.log'
streams = [json.loads(s) for s in (out/'streams.jsonl').read_text().splitlines()]
receipt = {'mode':'single-prewarm-then-idle', 'pilotStopSeconds':1,
           'clipStartSeconds':[2,7,12,17], 'streams':streams,
           'exeSHA256':hashlib.sha256(exe.read_bytes()).hexdigest(),
           'fixtureSHA256':hashlib.sha256(fixture.read_bytes()).hexdigest(),
           'soundSHA256':hashlib.sha256((runtime/'assets/achievement-complete.ogg').read_bytes()).hexdigest(),
           'capturesOnlyOwnDocument':True, 'gameClientChanged':False, 'serverChanged':False}
(out/'receipt.json').write_text(json.dumps(receipt,indent=2)+'\n')
print(json.dumps(receipt,indent=2))
