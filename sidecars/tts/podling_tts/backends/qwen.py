"""Qwen3-TTS 1.7B Base: a voice-cloning model that speaks one speaker per call.

Each turn is its own call, so turn spans are exact and voices cannot swap. The
voice-clone path takes no style instruction: emotion and laughs/chuckles/sighs are
reported as dropped, and a backchannel is rendered as its own short call (the
assembler places it). Needs the `qwen` extra and a CUDA GPU.
"""

from __future__ import annotations

import hashlib
from pathlib import Path
from typing import Any

from podling_tts.backends import (
    Backend,
    BackendError,
    clip_path,
    ignored_context,
    in_line,
    turn_seed,
    write_wav,
)
from podling_tts.protocol import SynthesizeRequest, SynthesizeResult, Voice

MODEL = "Qwen/Qwen3-TTS-12Hz-1.7B-Base"
# Measured in the bake-off: 5,824 MiB peak for the process; leave some headroom.
NEEDS_MIB = 6_200


def snapshot(model: str) -> Path:
    """The local snapshot of `model`; never downloads (the README says how)."""
    from huggingface_hub import snapshot_download
    from huggingface_hub.errors import LocalEntryNotFoundError

    try:
        return Path(snapshot_download(model, local_files_only=True))
    except LocalEntryNotFoundError as err:
        raise BackendError(
            f"{model} is not downloaded; run `uv run --extra qwen hf download {model}`"
        ) from err


def digest(model_dir: Path) -> str:
    """`sha256:<hex>` over a local model's weight and config files, names included.

    A `--model-dir` has no commit to name it, and its directory name stays the same
    when the weights inside change, so the files themselves are hashed (once).
    """
    files = sorted(
        p
        for pattern in ("*.safetensors", "*.json")
        for p in model_dir.rglob(pattern)
        if p.is_file()
    )
    if not files:
        raise BackendError(f"{model_dir} holds no *.safetensors or *.json files")
    h = hashlib.sha256()
    for path in files:
        name = path.relative_to(model_dir).as_posix().encode()
        h.update(len(name).to_bytes(8, "little"))
        h.update(name)
        h.update(path.stat().st_size.to_bytes(8, "little"))
        with path.open("rb") as f:
            for block in iter(lambda: f.read(1 << 20), b""):
                h.update(block)
    return f"sha256:{h.hexdigest()}"


class QwenBackend(Backend):
    name = "qwen"
    model = MODEL
    multi_speaker = False
    max_chunk_secs = 120
    native_sample_rate = 24_000

    def __init__(self, model_dir: str | None = None, language: str = "English"):
        self.path = Path(model_dir) if model_dir else snapshot(MODEL)
        self.local = model_dir is not None
        self.language = language
        self._weights: str | None = None
        self._model: Any = None
        # Voice-clone prompts are cached per reference clip and transcript.
        self._prompts: dict[tuple[Path, str], Any] = {}

    def weights(self) -> str:
        if not self.local:
            # A hub snapshot directory is named after the commit it holds.
            return self.path.name
        if self._weights is None:
            self._weights = digest(self.path)
        return self._weights

    @property
    def loaded(self) -> bool:
        return self._model is not None

    def load(self) -> None:
        if self._model is not None:
            return
        import torch

        if not torch.cuda.is_available():
            raise BackendError("Qwen3-TTS needs a CUDA GPU, and torch sees none")
        free, _total = torch.cuda.mem_get_info()
        free_mib = free // 2**20
        if free_mib < NEEDS_MIB:
            raise BackendError(
                f"only {free_mib} MiB of GPU memory is free and Qwen3-TTS needs about "
                f"{NEEDS_MIB} MiB. Unload other models first (`ollama stop <model>`); "
                "the desktop itself holds about 1.2 GB"
            )
        from qwen_tts import Qwen3TTSModel

        self._model = Qwen3TTSModel.from_pretrained(
            str(self.path),
            device_map="cuda:0",
            dtype=torch.bfloat16,
            attn_implementation="sdpa",
        )

    def _prompt(self, voice: Voice) -> Any:
        key = (voice.reference, voice.transcript)
        if key not in self._prompts:
            self._prompts[key] = self._model.create_voice_clone_prompt(
                ref_audio=str(voice.reference),
                ref_text=voice.transcript,
                x_vector_only_mode=False,
            )
        return self._prompts[key]

    def _speak(self, text: str, voice: Voice, seed: int) -> tuple[Any, int]:
        import numpy as np
        import torch

        torch.manual_seed(seed)
        torch.cuda.manual_seed_all(seed)
        wavs, rate = self._model.generate_voice_clone(
            text=text, language=self.language, voice_clone_prompt=self._prompt(voice)
        )
        audio = np.asarray(wavs[0], dtype=np.float32)
        if audio.ndim > 1:
            audio = audio.mean(axis=-1)
        if audio.size == 0 or not np.isfinite(audio).all():
            raise BackendError("Qwen3-TTS returned empty or non-finite audio")
        return audio, int(rate)

    def synthesize(self, request: SynthesizeRequest) -> SynthesizeResult:
        import numpy as np

        self.load()
        pieces: list[Any] = []
        length = 0
        spans: list[tuple[int, int]] = []
        dropped: list[dict] = ignored_context(request)
        clips: list[dict] = []
        rate = self.native_sample_rate
        for t, turn in enumerate(request.turns):
            seed = turn_seed(request.seed, t)
            if turn.emotion is not None:
                dropped.append({"turn": t, "kind": "emotion"})
            text, spoken = in_line(turn)
            for k, event in enumerate(turn.nonverbal):
                if k in spoken:
                    continue
                if event.kind != "backchannel":
                    dropped.append({"turn": t, "kind": event.kind})
                    continue
                clip, rate = self._speak(
                    event.text or "", request.voices[event.by], seed
                )
                path = clip_path(request.out_path, t, k)
                write_wav(path, clip, rate)
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
            audio, rate = self._speak(text, request.voices[turn.speaker], seed)
            pieces.append(audio)
            spans.append((length, length + len(audio)))
            length += len(audio)
        write_wav(request.out_path, np.concatenate(pieces), rate)
        return SynthesizeResult(rate, length, spans, dropped, clips)

    def unload(self) -> None:
        if self._model is None:
            return
        import gc

        import torch

        self._model = None
        self._prompts.clear()
        gc.collect()
        torch.cuda.empty_cache()
