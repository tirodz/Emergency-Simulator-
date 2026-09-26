#!/usr/bin/env python3
"""Generate the bundled emergency test tone.

The tone is the standard Emergency Alert System attention signal: 853 Hz and 960 Hz sounded
together. That pairing is what makes an alert recognisable *as an alert* by ear.

The device's own alarm ringtone was used first and is wrong for this tool. On a Samsung build the
default alarm/notification tone is a pleasant chime that sounds like a text message, so the operator
cannot tell from across the room whether the alert path ran or they received an SMS. A distinct,
deliberately attention-demanding tone is part of the instrument, not decoration.

Generated rather than committed as an opaque binary so the tone is reproducible and its
characteristics are reviewable. Deterministic: identical bytes on every run.

Regenerate with: python3 tools/make_tone.py
"""

import math
import struct
import sys
import wave
from pathlib import Path

SAMPLE_RATE = 22050
DURATION_SECONDS = 4.0
TONES_HZ = (853.0, 960.0)
AMPLITUDE = 0.85
# Long enough to remove the discontinuity click at the loop point, short enough to be inaudible.
FADE_SECONDS = 0.005

OUTPUT = (
    Path(__file__).resolve().parent.parent
    / "android"
    / "local-simulator"
    / "app"
    / "src"
    / "main"
    / "res"
    / "raw"
    / "emergency_tone.wav"
)


def build_frames() -> bytes:
    total = int(SAMPLE_RATE * DURATION_SECONDS)
    fade = max(1, int(SAMPLE_RATE * FADE_SECONDS))
    frames = bytearray()

    for index in range(total):
        seconds = index / SAMPLE_RATE
        sample = sum(math.sin(2.0 * math.pi * hz * seconds) for hz in TONES_HZ) / len(TONES_HZ)

        if index < fade:
            sample *= index / fade
        elif index >= total - fade:
            sample *= (total - 1 - index) / fade

        clamped = max(-1.0, min(1.0, sample * AMPLITUDE))
        frames += struct.pack("<h", int(clamped * 32767))

    return bytes(frames)


def main() -> int:
    frames = build_frames()
    OUTPUT.parent.mkdir(parents=True, exist_ok=True)

    with wave.open(str(OUTPUT), "wb") as handle:
        handle.setnchannels(1)
        handle.setsampwidth(2)
        handle.setframerate(SAMPLE_RATE)
        handle.writeframes(frames)

    print(
        f"wrote {OUTPUT} ({OUTPUT.stat().st_size} bytes, "
        f"{DURATION_SECONDS}s, {SAMPLE_RATE} Hz, tones={'/'.join(str(t) for t in TONES_HZ)} Hz)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
