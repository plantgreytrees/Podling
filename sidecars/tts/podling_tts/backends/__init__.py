"""TTS backends: each one turns a validated request into a WAV in the run directory.

A backend maps Podling's emotion and nonverbal events to its model's own syntax, so
the Rust side never learns model-specific tags. What a model cannot express is
listed in the result's `dropped`, never silently lost.
"""

from __future__ import annotations

import os
import struct
import sys
import tempfile
from array import array
from collections.abc import Sequence
from pathlib import Path
from typing import Any

from podling_tts.protocol import SynthesizeRequest, SynthesizeResult, Turn


class BackendError(RuntimeError):
    """The model failed or cannot run; the server answers 503 with the message."""


class Backend:
    name: str = ""
    model: str = ""
    multi_speaker: bool = False
    max_chunk_secs: int = 120
    max_speakers: int = 8
    native_sample_rate: int = 24_000
    # Whether the model listens to a request's `context`. Podling sends none, and
    # keys no chunk on it, when it does not.
    uses_context: bool = False

    def weights(self) -> str:
        """An identifier of the exact weights (a revision or hash), for the cache key."""
        return ""

    @property
    def loaded(self) -> bool:
        return False

    def capabilities(self) -> dict[str, Any]:
        return {
            "multi_speaker": self.multi_speaker,
            "max_chunk_secs": self.max_chunk_secs,
            "max_speakers": self.max_speakers,
            "native_sample_rate": self.native_sample_rate,
            "context": self.uses_context,
        }

    def load(self) -> None:
        """Loads the model now instead of on the first request (`--preload`)."""

    def synthesize(self, request: SynthesizeRequest) -> SynthesizeResult:
        raise NotImplementedError

    def unload(self) -> None:
        """Frees the model (and its GPU memory); the next request loads it again."""


def write_wav(path: Path, samples: Sequence[float] | Any, rate: int) -> None:
    """Writes mono 32-bit float PCM (WAVE_FORMAT_IEEE_FLOAT) atomically.

    The stdlib `wave` module only writes integer PCM, and the header is short, so it
    is written by hand. Temp file + rename means Podling never sees half a file, and
    a symlink planted at `path` is replaced rather than written through.
    """
    data = _f32le(samples)
    header = b"".join(
        [
            b"RIFF",
            struct.pack("<I", 4 + 26 + 12 + 8 + len(data)),
            b"WAVE",
            b"fmt ",
            # size, format 3 = float, channels, rate, byte rate, block align, bits, cbSize
            struct.pack("<IHHIIHHH", 18, 3, 1, rate, rate * 4, 4, 32, 0),
            b"fact",
            struct.pack("<II", 4, len(data) // 4),
            b"data",
            struct.pack("<I", len(data)),
        ]
    )
    fd, tmp = tempfile.mkstemp(dir=path.parent, prefix=".tmp-", suffix=".wav")
    try:
        with os.fdopen(fd, "wb") as out:
            out.write(header)
            out.write(data)
        os.replace(tmp, path)
    except BaseException:
        Path(tmp).unlink(missing_ok=True)
        raise


def _f32le(samples: Any) -> bytes:
    """Little-endian f32 bytes from a numpy array (fast path) or any float sequence."""
    if hasattr(samples, "astype"):
        return samples.astype("<f4").tobytes()
    values = array("f", samples)
    if sys.byteorder == "big":
        values.byteswap()
    return values.tobytes()


def clip_path(out_path: Path, turn: int, index: int) -> Path:
    """Where a separately rendered nonverbal clip goes: beside the main output."""
    return out_path.with_name(f"{out_path.stem}.t{turn}.n{index}.wav")


def in_line(turn: Turn) -> tuple[str, set[int]]:
    """The words to say for `turn`, with the speaker's own backchannels before or after
    them said in line ("Mm-hm. Right, so..."), and the indices of those events.

    A per-turn model has no tags for sounds, but a backchannel is words, so the
    speaker can simply say it. Anything else is left for the caller to render or drop.

    Podling checks the audio against the same words (`SpokenTurn::said` in
    crates/podling-core/src/plugin/tts.rs); change both together.
    """
    before: list[str] = []
    after: list[str] = []
    spoken: set[int] = set()
    for k, event in enumerate(turn.nonverbal):
        own = event.by == turn.speaker and event.kind == "backchannel" and event.text
        if own and event.at in ("before", "after"):
            (before if event.at == "before" else after).append(event.text)
            spoken.add(k)
    return " ".join([*before, turn.text, *after]), spoken


def ignored_context(request: SynthesizeRequest) -> list[dict[str, Any]]:
    """A `dropped` entry for a context the backend cannot listen to."""
    return [] if request.context is None else [{"kind": "context"}]


def turn_seed(seed: int, turn: int) -> int:
    """Each turn of a chunk gets its own seed, so a turn's audio doesn't depend on its neighbours."""
    return (seed + turn) % 2**64


def make_backend(name: str, **options: Any) -> Backend:
    if name == "fake":
        from podling_tts.backends.fake import FakeBackend

        return FakeBackend()
    if name == "qwen":
        from podling_tts.backends.qwen import QwenBackend

        return QwenBackend(**options)
    raise BackendError(f"unknown backend {name!r} (expected fake or qwen)")
