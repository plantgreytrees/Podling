"""Protocol v1 between Podling and a TTS worker: request types and their validation.

Every request body is checked field by field: a wrong type, a missing field or an
unknown field is a `ProtocolError`, which the server answers with HTTP 400 and the
reason. Paths are resolved (symlinks included) and must stay inside the run
directory the worker was started with, so a request can neither read a file
Podling did not put there nor write outside it.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from pathlib import Path
from typing import Any

PROTOCOL = 1
PREFIX = "/v1/podling"

# The audio a turn can carry beside its words. `at` says where: before or after the
# words in the same voice, or `over` the turn on a second track.
NONVERBAL_KINDS = ("laugh", "chuckle", "sigh", "backchannel")
NONVERBAL_AT = ("before", "after", "over")


class ProtocolError(ValueError):
    """A request that does not follow protocol v1; the message says why."""


@dataclass(frozen=True)
class Nonverbal:
    kind: str
    by: str
    at: str
    text: str | None = None


@dataclass(frozen=True)
class Turn:
    speaker: str
    text: str
    emotion: str | None = None
    nonverbal: tuple[Nonverbal, ...] = ()


@dataclass(frozen=True)
class Voice:
    reference: Path
    transcript: str


@dataclass(frozen=True)
class Context:
    """Conditioning only, never part of the output: the previous beat and callback clips."""

    turns: tuple[Turn, ...] = ()
    audio: Path | None = None
    callbacks: tuple[Path, ...] = ()


@dataclass(frozen=True)
class SynthesizeRequest:
    turns: tuple[Turn, ...]
    voices: dict[str, Voice]
    seed: int
    out_path: Path
    context: Context | None = None


@dataclass
class SynthesizeResult:
    """What a backend made: `turn_spans` are sample ranges in the main output."""

    sample_rate: int
    samples: int
    turn_spans: list[tuple[int, int]] | None
    # Requested features the backend cannot express, e.g. {"turn": 0, "kind": "laugh"}.
    dropped: list[dict[str, Any]] = field(default_factory=list)
    # Backchannels rendered as their own clips: {"turn", "index", "by", "path", "samples"}.
    clips: list[dict[str, Any]] = field(default_factory=list)

    def to_json(self) -> dict[str, Any]:
        return {
            "sample_rate": self.sample_rate,
            "samples": self.samples,
            "turn_spans": (
                None if self.turn_spans is None else [list(s) for s in self.turn_spans]
            ),
            "dropped": self.dropped,
            "clips": self.clips,
        }


# --------------------------------------------------------------------------
# Field checks


def _object(value: Any, where: str, required: set[str], optional: set[str]) -> dict:
    if not isinstance(value, dict):
        raise ProtocolError(f"{where} must be an object")
    unknown = sorted(set(value) - required - optional)
    if unknown:
        raise ProtocolError(f"{where} has unknown field(s): {', '.join(unknown)}")
    missing = sorted(required - set(value))
    if missing:
        raise ProtocolError(f"{where} is missing field(s): {', '.join(missing)}")
    return value


def _text(value: Any, where: str, *, allow_empty: bool = False) -> str:
    if not isinstance(value, str):
        raise ProtocolError(f"{where} must be a string")
    if not allow_empty and not value.strip():
        raise ProtocolError(f"{where} must not be empty")
    return value


def _list(value: Any, where: str) -> list:
    if not isinstance(value, list):
        raise ProtocolError(f"{where} must be a list")
    return value


def _choice(value: Any, where: str, choices: tuple[str, ...]) -> str:
    text = _text(value, where)
    if text not in choices:
        raise ProtocolError(f"{where} must be one of {', '.join(choices)}")
    return text


def _seed(value: Any) -> int:
    # bool is an int subclass in Python; a seed of `true` is a client bug.
    if isinstance(value, bool) or not isinstance(value, int):
        raise ProtocolError("seed must be an integer")
    if not 0 <= value < 2**64:
        raise ProtocolError("seed must fit in an unsigned 64-bit integer")
    return value


def confine(value: Any, where: str, run_dir: Path, *, must_exist: bool) -> Path:
    """Resolves a path (following symlinks) and refuses it unless it is inside `run_dir`.

    `run_dir` must already be resolved. A path that does not exist yet (an output)
    is resolved through its parent, so a symlinked parent cannot smuggle it out.
    """
    raw = Path(_text(value, where))
    if not raw.is_absolute():
        raise ProtocolError(f"{where} must be an absolute path")
    resolved = raw.resolve(strict=False)
    if not resolved.is_relative_to(run_dir) or resolved == run_dir:
        raise ProtocolError(f"{where} is outside the run directory")
    if must_exist and not resolved.is_file():
        raise ProtocolError(f"{where} is not a file")
    return resolved


# --------------------------------------------------------------------------
# Request parsing


def _nonverbal(value: Any, where: str, speakers: set[str]) -> Nonverbal:
    obj = _object(value, where, {"kind", "by", "at"}, {"text"})
    kind = _choice(obj["kind"], f"{where}.kind", NONVERBAL_KINDS)
    by = _text(obj["by"], f"{where}.by")
    if by not in speakers:
        raise ProtocolError(f"{where}.by names {by!r}, which has no voice")
    at = _choice(obj["at"], f"{where}.at", NONVERBAL_AT)
    text = obj.get("text")
    if kind == "backchannel":
        if text is None:
            raise ProtocolError(f"{where}.text is required for a backchannel")
        text = _text(text, f"{where}.text")
    elif text is not None:
        raise ProtocolError(f"{where}.text is only allowed on a backchannel")
    return Nonverbal(kind, by, at, text)


def _turn(value: Any, where: str, speakers: set[str]) -> Turn:
    obj = _object(value, where, {"speaker", "text"}, {"emotion", "nonverbal"})
    speaker = _text(obj["speaker"], f"{where}.speaker")
    if speaker not in speakers:
        raise ProtocolError(f"{where}.speaker names {speaker!r}, which has no voice")
    emotion = obj.get("emotion")
    if emotion is not None:
        emotion = _text(emotion, f"{where}.emotion")
    nonverbal = tuple(
        _nonverbal(n, f"{where}.nonverbal[{i}]", speakers)
        for i, n in enumerate(_list(obj.get("nonverbal", []), f"{where}.nonverbal"))
    )
    return Turn(speaker, _text(obj["text"], f"{where}.text"), emotion, nonverbal)


def _voice(value: Any, where: str, run_dir: Path) -> Voice:
    obj = _object(value, where, {"reference", "transcript"}, set())
    return Voice(
        confine(obj["reference"], f"{where}.reference", run_dir, must_exist=True),
        _text(obj["transcript"], f"{where}.transcript"),
    )


def _context(value: Any, run_dir: Path, speakers: set[str]) -> Context | None:
    if value is None:
        return None
    obj = _object(value, "context", set(), {"turns", "audio", "callbacks"})
    turns = tuple(
        _turn(t, f"context.turns[{i}]", speakers)
        for i, t in enumerate(_list(obj.get("turns", []), "context.turns"))
    )
    audio = obj.get("audio")
    if audio is not None:
        audio = confine(audio, "context.audio", run_dir, must_exist=True)
    callbacks = tuple(
        confine(c, f"context.callbacks[{i}]", run_dir, must_exist=True)
        for i, c in enumerate(_list(obj.get("callbacks", []), "context.callbacks"))
    )
    return Context(turns, audio, callbacks)


def parse_synthesize(body: Any, run_dir: Path) -> SynthesizeRequest:
    """Validates a `/synthesize` body against protocol v1."""
    obj = _object(body, "request", {"turns", "voices", "seed", "out_path"}, {"context"})
    voices_obj = obj["voices"]
    if not isinstance(voices_obj, dict) or not voices_obj:
        raise ProtocolError("voices must be a non-empty object keyed by speaker")
    voices = {
        _text(speaker, "voices key"): _voice(v, f"voices.{speaker}", run_dir)
        for speaker, v in voices_obj.items()
    }
    speakers = set(voices)
    turns = tuple(
        _turn(t, f"turns[{i}]", speakers)
        for i, t in enumerate(_list(obj["turns"], "turns"))
    )
    if not turns:
        raise ProtocolError("turns must not be empty")
    out_path = confine(obj["out_path"], "out_path", run_dir, must_exist=False)
    if out_path.suffix != ".wav":
        raise ProtocolError("out_path must end in .wav")
    if not out_path.parent.is_dir():
        raise ProtocolError("out_path's directory does not exist")
    return SynthesizeRequest(
        turns,
        voices,
        _seed(obj["seed"]),
        out_path,
        _context(obj.get("context"), run_dir, speakers),
    )
