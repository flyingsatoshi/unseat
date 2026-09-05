"""Generate tiny original alert WAVs. Output is CC0 (public domain)."""

from __future__ import annotations

import math
import struct
import wave
from pathlib import Path

RATE = 22050
AMP = 0.32


def write_wav(path: Path, samples: list[float]) -> None:
    clipped = [max(-1.0, min(1.0, s)) for s in samples]
    frames = b"".join(struct.pack("<h", int(s * 32767)) for s in clipped)
    with wave.open(str(path), "w") as wav:
        wav.setnchannels(1)
        wav.setsampwidth(2)
        wav.setframerate(RATE)
        wav.writeframes(frames)


def env(i: int, n: int, attack: float = 0.01) -> float:
    t = i / RATE
    dur = n / RATE
    a = min(1.0, t / attack) if attack > 0 else 1.0
    rel = max(0.0, 1.0 - t / dur)
    return a * (rel**1.6)


def tone(freq: float, seconds: float, volume: float = 1.0, attack: float = 0.008) -> list[float]:
    n = int(RATE * seconds)
    out = []
    for i in range(n):
        t = i / RATE
        s = math.sin(2 * math.pi * freq * t)
        out.append(s * volume * AMP * env(i, n, attack))
    return out


def mix(*parts: list[float]) -> list[float]:
    n = max(len(p) for p in parts)
    out = [0.0] * n
    for part in parts:
        for i, s in enumerate(part):
            out[i] += s
    return out


def silence(seconds: float) -> list[float]:
    return [0.0] * int(RATE * seconds)


def chime() -> list[float]:
    return mix(tone(880.0, 0.28, 1.0), [0.0] * int(RATE * 0.07) + tone(1320.0, 0.32, 0.75))


def bell() -> list[float]:
    return mix(
        tone(523.25, 0.7, 0.85, 0.004),
        tone(1046.5, 0.55, 0.28, 0.004),
        tone(1568.0, 0.35, 0.12, 0.004),
    )


def pulse() -> list[float]:
    beat = tone(440.0, 0.09, 1.0, 0.003)
    gap = silence(0.07)
    return beat + gap + beat + gap + beat


def glass() -> list[float]:
    return mix(tone(2093.0, 0.38, 0.7, 0.002), tone(3136.0, 0.22, 0.22, 0.002))


def soft() -> list[float]:
    n = int(RATE * 0.45)
    out = []
    for i in range(n):
        t = i / RATE
        tri = abs((t * 392.0 * 2.0) % 2.0 - 1.0) * 2.0 - 1.0
        out.append(tri * 0.7 * AMP * env(i, n, 0.02))
    return out


def main() -> None:
    root = Path(__file__).resolve().parents[1] / "assets" / "sounds"
    root.mkdir(parents=True, exist_ok=True)
    write_wav(root / "chime.wav", chime())
    write_wav(root / "bell.wav", bell())
    write_wav(root / "pulse.wav", pulse())
    write_wav(root / "glass.wav", glass())
    write_wav(root / "soft.wav", soft())
    (root / "LICENSE.txt").write_text(
        "Unseat alert sounds\n"
        "===================\n\n"
        "The WAV files in this directory (chime.wav, bell.wav, pulse.wav,\n"
        "glass.wav, soft.wav) were generated for Unseat as original works.\n"
        "They contain no third-party samples.\n\n"
        "License: CC0 1.0 Universal (public domain dedication).\n"
        "https://creativecommons.org/publicdomain/zero/1.0/\n\n"
        "You may copy, modify, and redistribute them without attribution.\n"
        "Attribution is still appreciated: \"Alert sounds from Unseat\".\n",
        encoding="utf-8",
    )
    print(f"wrote sounds in {root}")


if __name__ == "__main__":
    main()
