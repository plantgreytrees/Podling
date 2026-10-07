"""Design a voice clip from a description, for an episode's `[[cast]]`.

Qwen3-TTS VoiceDesign speaks `--text` in a voice made from `--description`.
The clip is written with its transcript and a provenance file beside it, so
Podling accepts it under the generated licence and the voice can be made
again. Offline: the model must already be downloaded (see README.md), and
this tool is never run by the pipeline.

    uv run python design.py --description "A warm, low voice, unhurried" \\
        --text "The forest was flattened for miles." --seed 7 --out voices/host.wav
"""

from __future__ import annotations

import argparse
import gc
import json
import sys
from dataclasses import asdict, dataclass
from pathlib import Path

MODEL = "Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign"
TOOL_VERSION = "0.1.0"
# Podling's `GENERATED_VOICE_LICENCE` (crates/podling-types/src/episode.rs).
LICENCE = "LicenseRef-Podling-Generated"
# Qwen3-TTS 1.7B Base peaked at 5,824 MiB in the bake-off; VoiceDesign is the
# same size. Leave some headroom.
NEEDS_MIB = 6_200
LANGUAGE = "English"


@dataclass(frozen=True)
class Provenance:
    """Exactly the fields of Podling's `VoiceProvenance`."""

    model: str
    weights_commit: str
    design_prompt: str
    seed: int
    tool_version: str
    # Ties the provenance to this clip: Podling refuses it beside another.
    clip_blake3: str


def clip_hash(clip: Path) -> str:
    """The blake3 hash of the clip file's bytes, as lowercase hex, which
    Podling compares with the clip it loads."""
    import blake3

    return blake3.blake3(clip.read_bytes()).hexdigest()


def provenance_path(clip: Path) -> Path:
    """`voices/host.wav` -> `voices/host.wav.provenance.json`: appended to the
    whole file name, as Podling's `provenance_path` does."""
    return clip.with_name(clip.name + ".provenance.json")


def transcript_path(clip: Path) -> Path:
    """`voices/host.wav` -> `voices/host.txt`."""
    return clip.with_suffix(".txt")


def voice_line(clip: Path, text: str) -> str:
    """The `voice = {...}` line for the episode's `[[cast]]` entry."""
    reference = json.dumps(clip.as_posix())
    return (
        f"voice = {{ reference = {reference}, transcript = {json.dumps(text)}, "
        f"licence = {json.dumps(LICENCE)} }}"
    )


def nonempty(value: str) -> str:
    value = value.strip()
    if not value:
        raise argparse.ArgumentTypeError("must not be empty")
    return value


def seed(value: str) -> int:
    try:
        n = int(value)
    except ValueError as err:
        raise argparse.ArgumentTypeError(f"{value!r} is not a whole number") from err
    if not 0 <= n < 2**32:
        raise argparse.ArgumentTypeError("must be between 0 and 2**32 - 1")
    return n


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument(
        "--description",
        type=nonempty,
        required=True,
        help="the voice, in words: age, pitch, pace, manner",
    )
    parser.add_argument(
        "--text",
        type=nonempty,
        required=True,
        help="what the clip says; becomes its transcript",
    )
    parser.add_argument(
        "--seed",
        type=seed,
        required=True,
        help="sampling seed, recorded so the voice can be made again",
    )
    parser.add_argument(
        "--out", type=Path, required=True, help="the clip to write (.wav)"
    )
    parser.add_argument(
        "--force", action="store_true", help="replace an existing clip and its files"
    )
    args = parser.parse_args(argv)
    if args.out.suffix.lower() != ".wav":
        parser.error(f"--out must be a .wav file, not {args.out}")
    if not args.out.parent.is_dir():
        parser.error(f"--out's directory {args.out.parent} does not exist")
    existing = [p for p in outputs(args.out) if p.exists()]
    if existing and not args.force:
        parser.error(f"{existing[0]} exists; pass --force to replace it")
    return args


def outputs(clip: Path) -> list[Path]:
    return [clip, transcript_path(clip), provenance_path(clip)]


def snapshot(model: str) -> Path:
    """The local snapshot of `model`; never downloads (README.md says how)."""
    from huggingface_hub import snapshot_download
    from huggingface_hub.errors import LocalEntryNotFoundError

    try:
        return Path(snapshot_download(model, local_files_only=True))
    except LocalEntryNotFoundError:
        sys.exit(f"{model} is not downloaded; run `uv run hf download {model}`")


def free_mib() -> int:
    import torch

    if not torch.cuda.is_available():
        sys.exit("no CUDA GPU: VoiceDesign needs one")
    free, _ = torch.cuda.mem_get_info()
    return free // 2**20


def generate(model_dir: Path, args: argparse.Namespace):
    """The clip's samples and rate. One GPU model, freed before returning."""
    import numpy as np
    import torch
    from qwen_tts import Qwen3TTSModel

    model = Qwen3TTSModel.from_pretrained(
        str(model_dir),
        device_map="cuda:0",
        dtype=torch.bfloat16,
        attn_implementation="sdpa",
    )
    try:
        torch.manual_seed(args.seed)
        torch.cuda.manual_seed_all(args.seed)
        wavs, rate = model.generate_voice_design(
            text=args.text, instruct=args.description, language=LANGUAGE
        )
        return np.asarray(wavs[0], dtype=np.float32), int(rate)
    finally:
        # One GPU model at a time: nothing else may load until this is gone.
        del model
        gc.collect()
        torch.cuda.empty_cache()


def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    model_dir = snapshot(MODEL)
    free = free_mib()
    if free < NEEDS_MIB:
        sys.exit(
            f"only {free} MiB of GPU memory is free and VoiceDesign needs about "
            f"{NEEDS_MIB}; stop other models first (e.g. `ollama stop <model>`)"
        )
    samples, rate = generate(model_dir, args)

    import soundfile as sf

    sf.write(args.out, samples, rate)
    transcript_path(args.out).write_text(args.text + "\n", encoding="utf-8")
    provenance = Provenance(
        model=MODEL,
        # The snapshot directory is named after the weights' commit.
        weights_commit=model_dir.name,
        design_prompt=args.description,
        seed=args.seed,
        tool_version=TOOL_VERSION,
        # Hashed after writing, so it is the hash of the bytes on disk.
        clip_blake3=clip_hash(args.out),
    )
    provenance_path(args.out).write_text(
        json.dumps(asdict(provenance), indent=2) + "\n", encoding="utf-8"
    )
    print(
        f"wrote {args.out} ({len(samples) / rate:.1f} s), its transcript and provenance"
    )
    print(
        "add to the speaker's [[cast]] entry, with the path relative to the episode file:"
    )
    print(voice_line(args.out, args.text))


if __name__ == "__main__":
    main()
