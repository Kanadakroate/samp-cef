# Native notification sound candidate

This is a scoped alternative to rebuilding Chromium's capture implementation.
It fixes the missing-capture-prefix dependency for the achievement/daily sound;
it does not modify libcef.dll or claim to fix all browser media playback.

The shipped original OGG is decoded with pinned `lewton 0.10.2` once per toast
document, off the browser and mixer threads. A finite immutable PCM clip is
rendered by the existing plugin mixer. Its cursor starts at sample zero, retains
the final partial block, and cannot overflow the streaming 250 ms input queue:
the finite player never enters that queue. All existing streamed/spatial audio
continues on its old path.

## Integration and lifecycle

- `cef.notification_sound(action, token)` is exposed only in the exact versioned
  local toast main document. The host independently rechecks frame, source,
  document URL, non-external browser and nonzero native generation.
- The document URL determines the 64-character lowercase bundle hash. There is
  no script-supplied URL, filesystem path or volume. Only that bundle's
  `lr_progression_toast/achievement-complete.ogg` may be read, with canonical
  containment under the asset root, a 1 MiB input limit and ten-second PCM limit.
- Prepare has a bounded five-second UI deadline. Repeated polls use the same
  decode job/cache. Only the first document token is admitted until navigation
  resets the session. Play sequences must increase; duplicate play is idempotent.
- Only one sound per browser is admitted. Native status is polled locally every
  50 ms while preparing/playing, not through the gameserver. Queue failure,
  missing/invalid sound and start timeout fail without a second playback engine.
- Native playback uses fixed 0.35 clip gain multiplied by the existing GTA SFX
  gain. Pause/Alt-Tab follow the mixer's existing pause detector; paused finite
  voices retain their exact cursor. Hide/mute, navigation, browser destruction
  and explicit UI cancellation cancel the owned voice.
- `ended` follows rendering of the final sample plus 2048 silent drain frames,
  beyond the mixer's four queued 256-frame blocks. It is not a hardware/DAC ACK.
- `integration/notification-audio.js` adapts these states to the existing toast
  lifecycle. No browser, focus, layout or reward behavior is redesigned. Older
  clients retain their existing HTMLAudio compatibility path. Once native mode
  is selected, failure never falls back and accidentally plays twice.

The native fork and the UI bundle must both be delivered for this route to be
used. The server protocol is unchanged. The UI bundle contains one new adapter
file (55 total reviewed files); its generated Pawn URLs have a new content hash.
Changing only the client while keeping an old UI does not activate this fix.

## Validation performed

- Win32 release client and renderer build with Rust 1.93 and pinned CEF 150.
  Six pre-existing dead-code warnings remain in the CEF wrappers.
- Ten production mixer/finite-player unit tests, including prefix/tail, repeated
  play, pause, GTA gain, cancellation, stale sequences and invalid input.
- Two native URL policy tests; six adapter lifecycle tests.
- Original OGG decoded to 73,728 stereo frames at 48 kHz. Four renders at each
  tested output rate (44.1/48 kHz) retain the complete source; at 48 kHz the
  samples equal decoded source times gain exactly, including the first sample.
- Isolated real CEF/renderer IPC run with starts at 2/7/12/17 seconds: four
  completions, no CEF capture stream, and all four 44.1 kHz PCM outputs identical
  to the finite-player reference (`7edd7a74bb2c3e2044b492bf80295fa3fedb77f8f20075a2c562b20411fa06f1`).
  This executes the production decoder, session, voice, renderer and JS adapter;
  the harness supplies a non-GTA mixer clock/output sink.
- UI audio suite: 67 passes, including actual isolated Chromium HTMLAudio and
  two adapter-to-real-toast integration cases. This is compatibility/lifecycle
  coverage, not native playback proof.
- Asset package: 20 passes and three Windows symlink-privilege skips.
- Local Pawn build succeeded with the regenerated asset header; no migration.

`cargo test --lib -p cef-client` remains unavailable because the existing
`game_command` test configuration excludes functions used by `app.rs`. The
`notification_mixer_tests` example imports the actual production modules and
executes their tests without the GTA entry point instead of weakening that guard.

Private generated evidence remains under `target/notification-pcm-audit` and
`target/notification-bridge-audit`. The run driver takes explicit runtime/sound
paths and never copies the candidate into GTA. No audio asset is committed here.

## Remaining acceptance

No GTA installation, production deployment or human listening acceptance is
claimed. First sound after join, a three-sound sequence, an idle gap, mute,
pause/Alt-Tab, reconnect and game exit must still be checked with the matched
client/UI candidate. The existing output-device recovery behavior is unchanged;
this work does not claim a new hot-swap implementation or rule out downstream
hardware/driver glitches.
