#!/usr/bin/env python3
"""Dump reference Whisper transcripts for the candle parity test.

Development only: the pipeline never runs Python. This transcribes a few WAV
files with Hugging Face transformers, decoding the way the TTS bake-off did
(sequential 30 s windows, temperature fallback, no previous-text prompt), and
writes what it heard to
crates/podling-core/tests/fixtures/whisper/reference.json.

The clips are the bake-off's Qwen3-TTS chunks (scripts/tts_bakeoff/out/qwen,
gitignored: they are cloned from CC-BY-4.0 LibriTTS-R voices, so they are not
committed). Run the bake-off first, then, from the repository root:

    uv run --extra qwen --project scripts/tts_bakeoff python scripts/whisper_reference.py \
        <whisper-base.en snapshot dir> scripts/tts_bakeoff/out/qwen/chunk_000.wav ...
"""

import hashlib
import json
import math
import sys
from pathlib import Path

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly
from transformers import pipeline

# The same settings as scripts/tts_bakeoff/bakeoff.py (LONG_FORM).
LONG_FORM = {
    "temperature": (0.0, 0.2, 0.4, 0.6, 0.8, 1.0),
    "compression_ratio_threshold": 1.35,
    "logprob_threshold": -1.0,
    "no_speech_threshold": 0.6,
    "condition_on_prev_tokens": False,
}

OUT = Path("crates/podling-core/tests/fixtures/whisper/reference.json")


def load_16k(path: Path) -> np.ndarray:
    audio, rate = sf.read(path, dtype="float32", always_2d=True)
    audio = audio.mean(axis=1)
    g = math.gcd(rate, 16_000)
    return resample_poly(audio, 16_000 // g, rate // g).astype(np.float32)


def main() -> None:
    model_dir = sys.argv[1]
    asr = pipeline("automatic-speech-recognition", model=model_dir, device="cpu")
    clips = []
    for name in sys.argv[2:]:
        path = Path(name)
        out = asr(
            {"raw": load_16k(path), "sampling_rate": 16_000},
            return_timestamps=True,
            generate_kwargs=LONG_FORM,
        )
        clips.append(
            {
                "path": path.as_posix(),
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                "text": out["text"].strip(),
                "segments": [
                    {
                        "start": c["timestamp"][0],
                        "end": c["timestamp"][1],
                        "text": c["text"].strip(),
                    }
                    for c in out["chunks"]
                ],
            }
        )
        print(f"{path}: {out['text'][:80]}…", file=sys.stderr)
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(
        json.dumps({"snapshot": Path(model_dir).name, "clips": clips}, indent=2) + "\n"
    )


if __name__ == "__main__":
    main()
