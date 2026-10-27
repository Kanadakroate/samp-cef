# Fixed local UI audio candidate

`cef.ui_audio(action, value)` is available only in the six shipped versioned LR
main documents (house, garage, progression, toast, objective, diagnostic).
Browser-side origin, renderer source, main frame, external-browser and generation
checks remain enforced. JS supplies no path or audio bytes.

Actions: `prepare,0` caches `lr_shared/ui-click.ogg`; `click,0` plays a short bounded
voice; `volume,0..100` sets the shared UI gain. The existing notification API and
its document/sequence ownership remain unchanged. Decode limits and worker error
isolation are reused. Clicks have their own mixer map and cannot replace a toast.
Clicks are rate limited to45ms, discarded if queued for200ms or paused, and cleared
on pause/teardown. The UI gain defaults to1 and multiplies the existing GTA gain
and0.35 voice factor, leaving non-UI browser/world audio untouched.

No installation, production release, listening quality, or native CEF/GTA visual
acceptance is claimed by compilation or the isolated mixer tests.

The passive objective browser62405 sorts below interactive overlays; keyboard
focus still draws last. Actual GTA-native dialog layering requires ingame QA.
