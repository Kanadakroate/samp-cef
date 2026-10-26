# Audio startup diagnosis — 26 October 2026

Status: a reproducible upstream capture-start loss is confirmed. No production
fix, installation or deployment is claimed by this report.

## Mechanism

The pinned CEF 150.0.11+gb887805 Alloy host starts its audio capturer in
`OnAudioStateChanged(true)`, after playback has already become audible. After
two seconds of non-audibility it destroys that capturer. The next sound must
establish capture again. Keeping a browser and an HTMLAudio element alive, or
preloading the file, does not keep this independent capture path alive.

Pinned source:
https://github.com/chromiumembedded/cef/blob/b887805/libcef/browser/alloy/alloy_browser_host_impl.cc

Relevant lines: 61 (two-second timeout), 1175–1199 (start and teardown),
1247 onward (capturer creation).

Related upstream report: https://github.com/chromiumembedded/cef/issues/3319

## Measurements

An isolated Win32 harness used the installed CEF binary and the LR asset scheme,
with the unchanged achievement OGG. It captured only its own document's PCM at
the CEF callback. GTA, the plugin mixer, Windows output and a microphone were
not involved. Four clip starts were scheduled at 2, 7, 12 and 17 seconds.

- Cold baseline, 24 October: four distinct capture streams; prefix loss before
  usable audio in each stream. The captured stream runs for roughly the clip's
  1.536 seconds plus the two-second idle timeout.
- Continuous diagnostic pilot, 24 October: one capture stream; all four clip
  prefixes retained. This is an experiment, not a proposed production pilot.
- Single prewarm control, 26 October: a pilot stopped at one second. The first
  real clip arrived before capture expired and retained its prefix. Each of the
  next three clips restarted capture and lost its prefix again, although the
  same browser, audio element, cached asset and sound file were reused.

The latest control was aligned to the decoded sound using a 200–400 ms anchor:

| Clip | Capture | Anchor correlation | Source-prefix observation |
| --- | --- | --- | --- |
| 1 | Already active | 0.999552 | First 20 ms present; fitted gain 0.3366, approaching configured 0.35 |
| 2 | Restarted | 1.000000 | First 100 ms absent; normal gain by 120–140 ms |
| 3 | Restarted | 0.999615 | First 100 ms absent; normal gain by 120–140 ms |
| 4 | Restarted | 0.987807 | First 80 ms absent; transition follows |

These are 20 ms analysis windows, not exact sub-millisecond loss boundaries.
The reference decoder/resampler differs from the live CEF media pipeline;
correlation and fitted gain are evidence of retained/missing content, not a
claim of bit-identical decoding. Negative aligned offsets mean that the source
has already advanced before the capture stream begins.

Earlier GTA two-stage sampling also found that the retained CEF samples reached
the plugin mixer unchanged except for the expected gain and buffering delay.
The separate timing run recorded no queue overflow, underflow or stale-packet
drop. Those bounded runs do not rule out an additional fault after the mixer.

## Provenance

- libcef.dll SHA-256:
  `d48c1cd51ecb955552e560f91cb41aa38f72cdc8b661574c3f3b0bef3ad14c18`
- Original OGG SHA-256:
  `ac28f73647a88b5a852222e6198b76ee7b7f77b707f07421f343494d50144a98`
- Native harness SHA-256:
  `3f0dee1d7c1cc5f4247624fce38bf4c407ca40b79ffa1816edc1630d57de52af`
- Single-prewarm fixture SHA-256:
  `1a6f23171779438cc8c93822473f08c1984f3f38612631b80172a1a2053574c1`
- Evidence folders in the private game workspace:
  `output/cef-capture-lifetime-20261024/{baseline-valid,warm-valid,prime-control-20261026}`.
  Analysis: `signal-comparison.json` and `prime-signal-comparison.json`.
- The additional control driver is `tools/audio/capture_startup_audit.py`.

## Consequence for the LR fork

This failure occurs before the plugin receives the samples. Increasing its
queue or preloading the same file again cannot recover that missing prefix.
Changing file format or HTTP caching is not supported as the solution to this
reproduction: the sound was local and unchanged throughout the controls.

A candidate engine change must establish capture before the real sound, expose
reliable readiness, and retain it across idle gaps for the opted-in toast
browser. Removing only the idle timeout leaves the first cold start unfixed.
The CEF library is a separate build dependency: changing the Rust plugin alone
does not change the Alloy implementation above. Any alternative output route
must preserve GTA volume, pause/focus and mute behavior.

The diagnosis does not prove that every subjective stutter has only this cause.
First playback, repeated playback, idle gaps, navigation, focus/pause, shutdown
and output-device behavior still require validation of the eventual fix.
