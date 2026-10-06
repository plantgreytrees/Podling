"""Word test: does respelling change how Qwen3-TTS Base says a word?

Speaks each sentence in both bake-off voices with one fixed seed, writes the
clips to an output directory, then frees the GPU and has Whisper base.en (CPU)
transcribe every clip, so a listener and the speech check can compare the
original spelling with its respellings.

    uv run --extra qwen python word_test.py --out ~/podling-listening/word-test

Step 0 of docs/ideas/natural-episode-speech.md.
"""

from __future__ import annotations

import argparse
import gc
import json
from pathlib import Path

import numpy as np
import soundfile as sf
import torch

HERE = Path(__file__).parent
QWEN = "Qwen/Qwen3-TTS-12Hz-1.7B-Base"
WHISPER = "openai/whisper-base.en"
SEED = 1234

# (name, sentence). Each word is tried as written and respelt.
TESTS = [
    (
        "01-kilometres-british",
        "The village stood about sixty kilometres from the centre of the blast.",
    ),
    (
        "02-kilometers-american",
        "The village stood about sixty kilometers from the centre of the blast.",
    ),
    (
        "03-kilometres-respelt",
        "The village stood about sixty kill-oh-meeters from the centre of the blast.",
    ),
    (
        "04-km-abbreviation",
        "The village stood about 60 km from the centre of the blast.",
    ),
    ("05-kulik", "Leonid Kulik reached the site in 1927."),
    ("06-kulik-respelt", "Leonid Koolick reached the site in 1927."),
    ("07-tunguska", "Today we look at the Tunguska explosion of 1908."),
    ("08-tunguska-respelt", "Today we look at the Toon-goose-kah explosion of 1908."),
]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    args.out.mkdir(parents=True, exist_ok=True)

    voices = json.loads((HERE / "voices" / "voices.json").read_text())
    clips = synthesise(voices, args.out)
    transcribe(clips, args.out)


def local(repo: str) -> str:
    """The cached snapshot directory. A local path also stops transformers from
    asking the hub about the repo, which fails offline."""
    from huggingface_hub import snapshot_download

    return snapshot_download(repo, local_files_only=True)


def synthesise(voices: list[dict], out: Path) -> list[dict]:
    from qwen_tts import Qwen3TTSModel

    model = Qwen3TTSModel.from_pretrained(
        local(QWEN),
        device_map="cuda:0",
        dtype=torch.bfloat16,
        attn_implementation="sdpa",
    )
    clips = []
    for i, voice in enumerate(voices):
        speaker = ("host", "guest")[i]
        prompt = model.create_voice_clone_prompt(
            ref_audio=str(HERE / "voices" / voice["wav"]),
            ref_text=voice["transcript"],
            x_vector_only_mode=False,
        )
        for name, text in TESTS:
            torch.manual_seed(SEED)
            torch.cuda.manual_seed_all(SEED)
            wavs, rate = model.generate_voice_clone(
                text=text, language="English", voice_clone_prompt=prompt
            )
            path = out / f"{name}-{speaker}.wav"
            sf.write(path, np.asarray(wavs[0], dtype=np.float32), rate)
            clips.append({"clip": path.name, "speaker": speaker, "said": text})
            print("wrote", path, flush=True)
    print("peak GPU MiB", torch.cuda.max_memory_allocated() // 2**20, flush=True)
    # One GPU model at a time: free the TTS before anything else loads.
    del model
    gc.collect()
    torch.cuda.empty_cache()
    return clips


def transcribe(clips: list[dict], out: Path) -> None:
    from transformers import pipeline

    asr = pipeline("automatic-speech-recognition", model=local(WHISPER), device="cpu")
    for clip in clips:
        clip["whisper_heard"] = asr(str(out / clip["clip"]))["text"].strip()
        print(f"{clip['clip']:38} | {clip['whisper_heard']}", flush=True)
    (out / "whisper.json").write_text(json.dumps(clips, indent=1))


if __name__ == "__main__":
    main()
