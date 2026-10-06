"""A deterministic, model-free backend for tests: one sine tone per word.

Each speaker gets a pitch from a hash of its id, each word lasts 0.25 s with a short
fade so word boundaries are audible, and turns follow each other with no gap (the
assembler adds gaps). It behaves like a per-turn model: exact spans, backchannels
as their own clips, and laughs/sighs/emotion reported as dropped.
"""

from __future__ import annotations

import hashlib
import math

from podling_tts.backends import Backend, clip_path, ignored_context, in_line, write_wav
from podling_tts.protocol import SynthesizeRequest, SynthesizeResult

RATE = 24_000
WORD_SECS = 0.25
FADE = 240  # samples


def _pitch(speaker: str) -> float:
    digest = hashlib.blake2b(speaker.encode(), digest_size=2).digest()
    return 140.0 + int.from_bytes(digest, "little") % 160


def tone(speaker: str, text: str) -> list[float]:
    freq = _pitch(speaker)
    per_word = int(WORD_SECS * RATE)
    samples: list[float] = []
    for _ in text.split():
        for n in range(per_word):
            envelope = min(1.0, n / FADE, (per_word - n) / FADE)
            samples.append(0.3 * envelope * math.sin(2 * math.pi * freq * n / RATE))
    return samples


class FakeBackend(Backend):
    name = "fake"
    model = "sine"
    multi_speaker = False
    native_sample_rate = RATE

    def weights(self) -> str:
        return "none"

    @property
    def loaded(self) -> bool:
        return True

    def synthesize(self, request: SynthesizeRequest) -> SynthesizeResult:
        samples: list[float] = []
        spans: list[tuple[int, int]] = []
        dropped: list[dict] = ignored_context(request)
        clips: list[dict] = []
        for t, turn in enumerate(request.turns):
            if turn.emotion is not None:
                dropped.append({"turn": t, "kind": "emotion"})
            text, spoken = in_line(turn)
            for k, event in enumerate(turn.nonverbal):
                if k in spoken:
                    continue
                if event.kind != "backchannel":
                    dropped.append({"turn": t, "kind": event.kind})
                    continue
                clip = tone(event.by, event.text or "")
                path = clip_path(request.out_path, t, k)
                write_wav(path, clip, RATE)
                clips.append(
                    {
                        "turn": t,
                        "index": k,
                        "by": event.by,
                        "at": event.at,
                        "path": str(path),
                        "samples": len(clip),
                    }
                )
            start = len(samples)
            samples.extend(tone(turn.speaker, text))
            spans.append((start, len(samples)))
        write_wav(request.out_path, samples, RATE)
        return SynthesizeResult(RATE, len(samples), spans, dropped, clips)
