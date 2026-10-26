# Liberty Revival CEF fork

This repository is the dedicated development home for Liberty Revival's
customizations of [zottce/samp-cef](https://github.com/zottce/samp-cef).
The upstream GPL-3.0 license and attribution remain in place.

## Baseline

- Fork: https://github.com/Kanadakroate/samp-cef
- Upstream commit: `f14fe4fa7f0d036edf445ba02b4bddd4cccac377`
- Development branch: `codex/liberty-revival`
- Imported baseline: the existing reviewed LR protocol-1 patch, not a new audio fix.
- The import manifest is `docs/liberty-revival/baseline-provenance.json`.
  Its 42 normalized source hashes were checked against the imported files.
- CEF: `150.0.11+gb887805+chromium-150.0.7871.115`, Win32, API 15000.
- Rust: 1.93; retain the pinned dependency revisions and lockfile.

`origin` points to the LR fork; `upstream` points to the original project.
Future upstream changes should be fetched and reviewed before merging. Native
client, renderer and server artifacts must come from compatible source pins.

## Audio work

The new scoped native notification candidate and its exact validation boundary
are documented in [native-notification-player.md](docs/liberty-revival/native-notification-player.md).
It bypasses capture for the one local completion sound through the existing
GTA-aware mixer; it does not patch the CEF engine. The following describes the
imported baseline diagnosis, which remains relevant to other browser media.

The achievement audio issue remains unresolved in this baseline. An isolated
CEF 150 experiment reproduced missing initial samples when CEF's capture starts
on demand. A pilot signal kept capture alive, but is a diagnostic experiment,
not a production solution. See [CEF issue 3319](https://github.com/chromiumembedded/cef/issues/3319).

The plugin and the embedded Chromium/CEF engine are separate components.
Changing the plugin does not change `libcef.dll`. A durable capture-lifetime
change inside CEF requires a separately built and tested engine. Simply
disabling the plugin's audio handler would bypass GTA gain/pause behavior and
is not an equivalent fix.

Acceptance must distinguish source/build checks from native sample and listening
tests: first playback, repeated playback, idle gaps, mute, pause/Alt-Tab,
reconnect, teardown, output-device changes and unchanged spatial audio.

## Release boundaries

This local import is not a deployment or a newly verified client build.
Do not copy game credentials, certificates, account data, logs, caches or game
assets into this repository. Review LR-specific source before publishing it to
the public fork. Package only reviewed sources and artifacts with hashes;
installation and production release remain explicit separate steps.
