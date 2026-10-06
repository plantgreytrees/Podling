"""Phase 5 TTS bake-off: synthesise a Podling script with one backend, then score it.

One backend per process, so the GPU holds one model at a time:

    uv run --extra dia2 python bakeoff.py --backend dia2 --script script.json
    uv run --extra moss python bakeoff.py --backend moss --script script.json
    uv run --extra qwen python bakeoff.py --backend qwen --script script.json
    uv run --extra qwen python bakeoff.py --report          # table over all backends

Synthesis runs on the GPU and records peak VRAM and the real-time factor per
chunk. The model is then freed and scoring runs on the CPU: Whisper base.en and
small.en transcribe every chunk (word error rate, verbatim quote hits, CPU
real-time factor), and an ECAPA speaker encoder compares each turn with its
speaker's reference clip.

Voices: `--fetch-voices` downloads two LibriTTS-R clips (CC-BY-4.0) and writes
`voices/voices.json`; any other clips work if voices.json names a licence.
"""

from __future__ import annotations

import argparse
import difflib
import gc
import io
import json
import math
import os
import threading
import time
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Any

import numpy as np
import soundfile as sf
from scipy.signal import resample_poly

HERE = Path(__file__).resolve().parent
WORDS_PER_MINUTE = 155
TURN_GAP_SECS = 0.3
OLLAMA_PS = "http://127.0.0.1:11434/api/ps"
WHISPER_SIZES = ("base.en", "small.en")

BACKENDS = {
    "dia2": {
        "model": "nari-labs/Dia2-1B",
        "licence": "Apache-2.0",
        "max_chunk_secs": 90,
    },
    "moss": {
        "model": "OpenMOSS-Team/MOSS-TTSD-v1.0",
        "licence": "Apache-2.0",
        "max_chunk_secs": 120,
    },
    "qwen": {
        "model": "Qwen/Qwen3-TTS-12Hz-1.7B-Base",
        "licence": "Apache-2.0",
        "max_chunk_secs": 120,
    },
}

# LibriTTS-R (CC-BY-4.0, https://www.openslr.org/141/) test-clean speakers: a
# female (4446) and a male (1089) voice, one 6-11 s clip each.
VOICE_SPEAKERS = ["4446", "1089"]
LIBRITTS_R = "mythicinfinity/libritts_r"


# --------------------------------------------------------------------------
# Inputs


@dataclass
class Turn:
    index: int
    speaker: str
    text: str
    quotes: list[str]


@dataclass
class Voice:
    speaker: str
    wav: Path
    transcript: str
    licence: str
    source: str


def load_script(path: Path) -> tuple[list[str], list[Turn]]:
    """Reads a Podling `script.json` artifact (or a bare script body)."""
    doc = json.loads(path.read_text())
    body = doc.get("body", doc)
    cast = [s["id"] for s in body["cast"]]
    turns = [
        Turn(i, t["speaker"], t["text"], [q["text"] for q in t.get("quotes", [])])
        for i, t in enumerate(body["turns"])
    ]
    return cast, turns


def load_voices(path: Path, cast: list[str]) -> list[Voice]:
    """Voices in cast order: the first clip speaks for the first cast member."""
    entries = json.loads(path.read_text())
    if len(entries) < len(cast):
        raise SystemExit(
            f"{path} has {len(entries)} voices but the cast has {len(cast)}"
        )
    voices = []
    for speaker, entry in zip(cast, entries, strict=False):
        if not entry.get("licence"):
            raise SystemExit(f"voice {entry.get('wav')} has no licence")
        voices.append(
            Voice(
                speaker,
                path.parent / entry["wav"],
                entry["transcript"],
                entry["licence"],
                entry["source"],
            )
        )
    return voices


def estimate_secs(text: str) -> float:
    return len(text.split()) / WORDS_PER_MINUTE * 60


def pack_chunks(turns: list[Turn], max_secs: float) -> list[list[Turn]]:
    """Greedy packing of whole turns into chunks of at most `max_secs` (estimated)."""
    chunks: list[list[Turn]] = []
    current: list[Turn] = []
    length = 0.0
    for turn in turns:
        secs = estimate_secs(turn.text) + TURN_GAP_SECS
        if current and length + secs > max_secs:
            chunks.append(current)
            current, length = [], 0.0
        current.append(turn)
        length += secs
    if current:
        chunks.append(current)
    return chunks


def derive_seed(chunk_index: int, attempt: int = 0) -> int:
    return (chunk_index * 1_000_003 + attempt) % (2**31)


# --------------------------------------------------------------------------
# GPU memory


class VramMonitor:
    """Polls NVML for the device's used memory; `peak_mib - baseline_mib` is ours."""

    def __init__(self, interval: float = 0.2):
        import pynvml

        pynvml.nvmlInit()
        self._nvml = pynvml
        self._handle = pynvml.nvmlDeviceGetHandleByIndex(0)
        self.name = pynvml.nvmlDeviceGetName(self._handle)
        self.total_mib = self._used(total=True)
        self.baseline_mib = self._used()
        self.peak_mib = self.baseline_mib
        self._interval = interval
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._run, daemon=True)
        self._thread.start()

    def _used(self, total: bool = False) -> float:
        info = self._nvml.nvmlDeviceGetMemoryInfo(self._handle)
        return (info.total if total else info.used) / 2**20

    def _run(self) -> None:
        while not self._stop.is_set():
            self.peak_mib = max(self.peak_mib, self._used())
            time.sleep(self._interval)

    def reset_peak(self) -> None:
        self.peak_mib = self._used()

    def stop(self) -> None:
        self._stop.set()
        self._thread.join()


def ollama_state() -> dict[str, Any]:
    """What Ollama has loaded; the bake-off refuses to run with a model on the GPU."""
    try:
        with urllib.request.urlopen(OLLAMA_PS, timeout=3) as response:
            models = json.load(response).get("models", [])
    except OSError as err:
        return {"reachable": False, "error": str(err), "models": []}
    return {
        "reachable": True,
        "models": [
            {"name": m["name"], "size_vram": m.get("size_vram", 0)} for m in models
        ],
    }


# --------------------------------------------------------------------------
# Audio helpers


def to_mono(audio: np.ndarray) -> np.ndarray:
    audio = np.asarray(audio, dtype=np.float32)
    return audio.mean(axis=1) if audio.ndim == 2 else audio


def resample(audio: np.ndarray, src: int, dst: int) -> np.ndarray:
    if src == dst:
        return audio.astype(np.float32)
    g = math.gcd(src, dst)
    return resample_poly(audio, dst // g, src // g).astype(np.float32)


def read_mono(path: Path, rate: int) -> np.ndarray:
    audio, sr = sf.read(path, dtype="float32")
    return resample(to_mono(audio), sr, rate)


# --------------------------------------------------------------------------
# Whisper (transformers, CPU). Used for scoring and for Dia2's prefix timings.

_WHISPER: dict[str, Any] = {}


def whisper(size: str):
    if size not in _WHISPER:
        from transformers import pipeline

        # No chunk_length_s: the chunked pipeline (marked experimental) fell into
        # repetition loops ("The size of the graphs." x5) that scored good audio
        # at WER 0.3-0.9. Sequential long-form decoding instead, as Whisper does.
        _WHISPER[size] = pipeline(
            "automatic-speech-recognition",
            model=f"openai/whisper-{size}",
            device="cpu",
        )
    return _WHISPER[size]


# Whisper's own guard against loops: a 30 s window whose output is too
# repetitive (compression ratio) or too unlikely (mean log-prob) is decoded
# again at a higher temperature; earlier text is not fed back as a prompt.
LONG_FORM = {
    "temperature": (0.0, 0.2, 0.4, 0.6, 0.8, 1.0),
    "compression_ratio_threshold": 1.35,
    "logprob_threshold": -1.0,
    "no_speech_threshold": 0.6,
    "condition_on_prev_tokens": False,
}


def transcribe(
    size: str, audio16k: np.ndarray
) -> tuple[str, list[tuple[str, float, float]]]:
    out = whisper(size)(
        {"raw": audio16k, "sampling_rate": 16_000},
        return_timestamps="word",
        generate_kwargs=LONG_FORM,
    )
    words = [
        (
            c["text"].strip(),
            float(c["timestamp"][0] or 0.0),
            float(c["timestamp"][1] or c["timestamp"][0] or 0.0),
        )
        for c in out.get("chunks", [])
        if c["text"].strip()
    ]
    return out["text"].strip(), words


def normalise(size: str, text: str) -> list[str]:
    """Whisper's English normaliser: case, punctuation, spelled-out numbers."""
    return whisper(size).tokenizer.normalize(text).split()


def word_error_rate(reference: list[str], hypothesis: list[str]) -> float:
    if not reference:
        return 0.0 if not hypothesis else 1.0
    previous = list(range(len(hypothesis) + 1))
    for i, ref_word in enumerate(reference, 1):
        current = [i] + [0] * len(hypothesis)
        for j, hyp_word in enumerate(hypothesis, 1):
            current[j] = min(
                previous[j] + 1,
                current[j - 1] + 1,
                previous[j - 1] + (ref_word != hyp_word),
            )
        previous = current
    return previous[-1] / len(reference)


def contains(haystack: list[str], needle: list[str]) -> bool:
    n = len(needle)
    return n > 0 and any(
        haystack[i : i + n] == needle for i in range(len(haystack) - n + 1)
    )


# --------------------------------------------------------------------------
# Backends. Each returns (mono float32 audio, sample rate, turn spans in seconds or None).


class Backend:
    multi_speaker = True

    def load(self, voices: list[Voice]) -> None: ...

    def synth(self, turns: list[Turn], voices: dict[str, Voice], seed: int):
        raise NotImplementedError


class Dia2Backend(Backend):
    """Dia2: dialogue model, two speakers ([S1], [S2]), voices from prefix clips."""

    def load(self, voices: list[Voice]) -> None:
        import torch
        from dia2 import Dia2, engine
        from dia2.runtime import voice_clone

        if len(voices) > 2:
            raise SystemExit("Dia2 supports two speakers")
        # Dia2 times the prefix clip's words with whisper-timestamped (AGPL-3.0),
        # which is excluded from the environment. Whisper on the CPU does it here,
        # once per clip at load: Dia2 asks again on every chunk, and a sidecar
        # would store a voice's timings rather than recompute them per call.
        self.prefix_words = {str(v.wav): self._time_words(v.wav) for v in voices}
        voice_clone.transcribe_words = self._prefix_words
        self.torch = torch
        self.model = Dia2.from_repo(
            BACKENDS["dia2"]["model"], device="cuda", dtype="bfloat16"
        )
        # Dia2 decodes a whole chunk's Mimi frames in one GPU pass while the
        # generation cache is still resident, which overflows 8 GB on a
        # 90-second chunk. A CPU copy of the codec decodes instead; the GPU copy
        # still encodes the voice prefixes.
        engine.decode_audio = self._decode_on_cpu
        self.cpu_mimi = None

    @staticmethod
    def _time_words(wav: Path):
        from dia2.runtime.voice_clone import WhisperWord

        _, words = transcribe("small.en", read_mono(wav, 16_000))
        return [WhisperWord(text=w, start=s, end=e) for w, s, e in words]

    def _prefix_words(self, audio_path: str, device, language=None):
        return self.prefix_words[audio_path]

    def _decode_on_cpu(self, runtime, tokens):
        from dia2.audio.codec import DEFAULT_MIMI_MODEL_ID, MimiCodec

        if tokens.shape[-1] == 0:
            return self.torch.zeros(0)
        if self.cpu_mimi is None:
            self.cpu_mimi = MimiCodec.from_pretrained(
                DEFAULT_MIMI_MODEL_ID, device=self.torch.device("cpu")
            )
        self.torch.cuda.empty_cache()
        return self.cpu_mimi.decode(tokens.cpu())[0, 0]

    def synth(self, turns, voices, seed):
        from dia2 import GenerationConfig, SamplingConfig

        order = list(voices)
        text = " ".join(f"[S{order.index(t.speaker) + 1}] {t.text}" for t in turns)
        self.torch.manual_seed(seed)
        self.torch.cuda.manual_seed_all(seed)
        config = GenerationConfig(
            cfg_scale=2.0,
            audio=SamplingConfig(temperature=0.8, top_k=50),
            use_cuda_graph=True,
        )
        result = self.model.generate(
            text,
            config=config,
            prefix_speaker_1=str(voices[order[0]].wav),
            prefix_speaker_2=str(voices[order[1]].wav) if len(order) > 1 else None,
        )
        audio = result.waveform.detach().float().cpu().numpy().reshape(-1)
        return audio, int(result.sample_rate), None


class MossBackend(Backend):
    """MOSS-TTSD v1.0 (8B): the Qwen3 backbone in 4-bit NF4, audio heads in bf16."""

    def load(self, voices: list[Voice]) -> None:
        # transformers 5 materialises checkpoint tensors on the GPU from a thread
        # pool, so bf16 tensors pile up faster than they are quantised and the
        # load overflows 8 GB. Sequential loading quantises each in turn.
        os.environ.setdefault("HF_DEACTIVATE_ASYNC_LOAD", "1")
        import torch
        from transformers import AutoModel, AutoProcessor, BitsAndBytesConfig

        torch.backends.cuda.enable_cudnn_sdp(False)
        repo = BACKENDS["moss"]["model"]
        self.torch = torch
        self.processor = AutoProcessor.from_pretrained(repo, trust_remote_code=True)
        # The audio tokenizer is 1.77B parameters in fp32 (~7 GB), which cannot
        # share 8 GB with the backbone, so it encodes and decodes on the CPU.
        quant = BitsAndBytesConfig(
            load_in_4bit=True,
            bnb_4bit_quant_type="nf4",
            bnb_4bit_compute_dtype=torch.bfloat16,
            # Only the 16 small audio heads and embeddings stay bf16. The text
            # head (lm_heads.0, 0.64B parameters) is quantised too: left in bf16
            # beside the unquantisable 0.64B text embedding, it pushes the model
            # past 8 GB.
            llm_int8_skip_modules=["emb_ext", *(f"lm_heads.{i}" for i in range(1, 17))],
        )
        self.model = AutoModel.from_pretrained(
            repo,
            trust_remote_code=True,
            quantization_config=quant,
            attn_implementation="sdpa",
            torch_dtype=torch.bfloat16,
            device_map="cuda",
        ).eval()
        self.rate = int(self.processor.model_config.sampling_rate)
        wavs = [
            torch.from_numpy(read_mono(v.wav, self.rate)).unsqueeze(0) for v in voices
        ]
        self.reference_codes = self.processor.encode_audios_from_wav(
            wavs, sampling_rate=self.rate
        )
        self.prompt_codes = self.processor.encode_audios_from_wav(
            [torch.cat(wavs, dim=-1)], sampling_rate=self.rate
        )[0]
        self.prompt_text = " ".join(
            f"[S{i + 1}] {v.transcript}" for i, v in enumerate(voices)
        )
        self.order = [v.speaker for v in voices]

    def synth(self, turns, voices, seed):
        torch = self.torch
        text = " ".join(f"[S{self.order.index(t.speaker) + 1}] {t.text}" for t in turns)
        conversation = [
            [
                self.processor.build_user_message(
                    text=f"{self.prompt_text} {text}", reference=self.reference_codes
                ),
                self.processor.build_assistant_message(
                    audio_codes_list=[self.prompt_codes]
                ),
            ]
        ]
        batch = self.processor(conversation, mode="continuation")
        est = sum(estimate_secs(t.text) for t in turns)
        torch.manual_seed(seed)
        torch.cuda.manual_seed_all(seed)
        with torch.no_grad():
            outputs = self.model.generate(
                input_ids=batch["input_ids"].to("cuda"),
                attention_mask=batch["attention_mask"].to("cuda"),
                max_new_tokens=int(est * 12.5 * 1.6) + 200,
                audio_temperature=1.1,
                audio_top_p=0.9,
                audio_top_k=50,
                audio_repetition_penalty=1.1,
            )
        message = self.processor.decode(outputs)[0]
        if message is None:
            raise RuntimeError("MOSS-TTSD returned no audio")
        audio = message.audio_codes_list[0].float().cpu().numpy().reshape(-1)
        return audio, self.rate, None


class QwenBackend(Backend):
    """Qwen3-TTS 1.7B Base: one speaker per call, so one turn per call."""

    multi_speaker = False

    def load(self, voices: list[Voice]) -> None:
        import torch
        from qwen_tts import Qwen3TTSModel

        self.torch = torch
        self.model = Qwen3TTSModel.from_pretrained(
            BACKENDS["qwen"]["model"],
            device_map="cuda:0",
            dtype=torch.bfloat16,
            attn_implementation="sdpa",
        )
        self.prompts = {
            v.speaker: self.model.create_voice_clone_prompt(
                ref_audio=str(v.wav), ref_text=v.transcript, x_vector_only_mode=False
            )
            for v in voices
        }

    def synth(self, turns, voices, seed):
        pieces, spans, rate, cursor = [], [], 24_000, 0.0
        for i, turn in enumerate(turns):
            self.torch.manual_seed(seed + i)
            self.torch.cuda.manual_seed_all(seed + i)
            wavs, rate = self.model.generate_voice_clone(
                text=turn.text,
                language="English",
                voice_clone_prompt=self.prompts[turn.speaker],
            )
            audio = to_mono(wavs[0])
            if pieces:
                gap = np.zeros(int(TURN_GAP_SECS * rate), dtype=np.float32)
                pieces.append(gap)
                cursor += len(gap) / rate
            spans.append((cursor, cursor + len(audio) / rate))
            pieces.append(audio)
            cursor += len(audio) / rate
        return np.concatenate(pieces), int(rate), spans


BACKEND_CLASSES = {"dia2": Dia2Backend, "moss": MossBackend, "qwen": QwenBackend}


def model_revision(repo: str) -> str | None:
    try:
        from huggingface_hub import model_info

        return model_info(repo).sha
    # Offline or rate-limited (HTTP errors are OSErrors): the revision is informational.
    except OSError:
        return None


# --------------------------------------------------------------------------
# Synthesis


def synthesise(
    args, cast: list[str], turns: list[Turn], voices: list[Voice], out: Path
) -> dict[str, Any]:
    import torch

    info = BACKENDS[args.backend]
    max_secs = args.max_chunk_secs or info["max_chunk_secs"]
    backend = BACKEND_CLASSES[args.backend]()
    ollama = ollama_state()
    on_gpu = [m for m in ollama["models"] if m["size_vram"] > 0]
    if on_gpu and not args.allow_ollama:
        raise SystemExit(
            f"Ollama has {on_gpu} on the GPU; run `ollama stop <model>` first"
        )

    monitor = VramMonitor()
    started = time.perf_counter()
    backend.load(voices)
    load_secs = time.perf_counter() - started
    load_peak = monitor.peak_mib

    by_speaker = {v.speaker: v for v in voices}
    chunks = pack_chunks(turns, max_secs)[: args.chunks]
    records = []
    for index, chunk in enumerate(chunks):
        seed = derive_seed(index)
        torch.cuda.reset_peak_memory_stats()
        monitor.reset_peak()
        t0 = time.perf_counter()
        audio, rate, spans = backend.synth(chunk, by_speaker, seed)
        synth_secs = time.perf_counter() - t0
        wav = out / f"chunk_{index:03d}.wav"
        sf.write(wav, audio, rate, subtype="FLOAT")
        audio_secs = len(audio) / rate
        record = {
            "index": index,
            "wav": wav.name,
            "turns": [t.index for t in chunk],
            "est_secs": round(sum(estimate_secs(t.text) for t in chunk), 1),
            "audio_secs": round(audio_secs, 2),
            "synth_secs": round(synth_secs, 2),
            "rtf": round(synth_secs / max(audio_secs, 1e-6), 3),
            "seed": seed,
            "sample_rate": rate,
            "turn_spans": spans,
            "peak_nvml_mib": round(monitor.peak_mib),
            "peak_torch_allocated_mib": round(
                torch.cuda.max_memory_allocated() / 2**20
            ),
            "peak_torch_reserved_mib": round(torch.cuda.max_memory_reserved() / 2**20),
        }
        records.append(record)
        print(
            f"chunk {index}: {audio_secs:.1f}s audio in {synth_secs:.1f}s, peak {monitor.peak_mib:.0f} MiB"
        )

    peak = max([load_peak] + [r["peak_nvml_mib"] for r in records])
    result = {
        "backend": args.backend,
        "model": info["model"],
        "revision": model_revision(info["model"]),
        "licence": info["licence"],
        "gpu": monitor.name,
        "gpu_total_mib": round(monitor.total_mib),
        "baseline_mib": round(monitor.baseline_mib),
        "ollama_at_start": ollama,
        "loadavg_at_start": os.getloadavg(),
        "ollama_at_end": ollama_state(),
        "max_chunk_secs": max_secs,
        "load_secs": round(load_secs, 1),
        "peak_device_mib": round(peak),
        "peak_ours_mib": round(peak - monitor.baseline_mib),
        "voices": [
            {
                "speaker": v.speaker,
                "wav": str(v.wav.name),
                "licence": v.licence,
                "source": v.source,
            }
            for v in voices
        ],
        "chunks": records,
    }
    monitor.stop()
    del backend
    gc.collect()
    torch.cuda.empty_cache()
    return result


# --------------------------------------------------------------------------
# Scoring (CPU)


def align_turns(
    script_words: list[tuple[str, int]],
    heard: list[tuple[str, float, float]],
    size: str,
):
    """Maps each turn to the time range of the transcript words aligned with it."""
    reference = [w for w, _ in script_words]
    hypothesis = [normalise(size, w) for w, _, _ in heard]
    flat, owner = [], []
    for k, words in enumerate(hypothesis):
        for w in words:
            flat.append(w)
            owner.append(k)
    matcher = difflib.SequenceMatcher(a=reference, b=flat, autojunk=False)
    ranges: dict[int, list[float]] = {}
    for block in matcher.get_matching_blocks():
        for offset in range(block.size):
            turn = script_words[block.a + offset][1]
            _, start, end = heard[owner[block.b + offset]]
            r = ranges.setdefault(turn, [start, end])
            r[0], r[1] = min(r[0], start), max(r[1], end)
    return {turn: (s, e) for turn, (s, e) in ranges.items()}


def score(
    result: dict[str, Any], turns: list[Turn], voices: list[Voice], out: Path
) -> None:
    import torch
    from speechbrain.inference.speaker import EncoderClassifier

    encoder = EncoderClassifier.from_hparams(
        source="speechbrain/spkrec-ecapa-voxceleb",
        savedir=str(HERE / ".models" / "spkrec-ecapa-voxceleb"),
        run_opts={"device": "cpu"},
    )

    def embed(audio16k: np.ndarray) -> np.ndarray:
        with torch.no_grad():
            e = (
                encoder.encode_batch(torch.from_numpy(audio16k).unsqueeze(0))
                .reshape(-1)
                .numpy()
            )
        return e / (np.linalg.norm(e) + 1e-9)

    references = {v.speaker: embed(read_mono(v.wav, 16_000)) for v in voices}
    by_index = {t.index: t for t in turns}

    for record in result["chunks"]:
        audio16k = read_mono(out / record["wav"], 16_000)
        chunk_turns = [by_index[i] for i in record["turns"]]
        record["asr"] = {}
        for size in WHISPER_SIZES:
            t0 = time.perf_counter()
            text, heard = transcribe(size, audio16k)
            cpu_secs = time.perf_counter() - t0
            reference = normalise(size, " ".join(t.text for t in chunk_turns))
            hypothesis = normalise(size, text)
            quotes = [q for t in chunk_turns for q in t.quotes]
            hits = [q for q in quotes if contains(hypothesis, normalise(size, q))]
            record["asr"][size] = {
                "wer": round(word_error_rate(reference, hypothesis), 4),
                "quotes": len(quotes),
                "quote_hits": len(hits),
                "quote_misses": [q for q in quotes if q not in hits],
                "cpu_secs": round(cpu_secs, 2),
                "cpu_rtf": round(cpu_secs / max(record["audio_secs"], 1e-6), 3),
                "transcript": text,
            }
            if size == "small.en" and record["turn_spans"] is None:
                script_words = [
                    (w, t.index) for t in chunk_turns for w in normalise(size, t.text)
                ]
                spans = align_turns(script_words, heard, size)
                record["asr_turn_spans"] = {
                    str(k): [round(s, 2), round(e, 2)] for k, (s, e) in spans.items()
                }

        spans = (
            {
                t.index: tuple(s)
                for t, s in zip(chunk_turns, record["turn_spans"], strict=True)
            }
            if record["turn_spans"] is not None
            else {int(k): tuple(v) for k, v in record.get("asr_turn_spans", {}).items()}
        )
        similarity = {}
        for turn in chunk_turns:
            if turn.index not in spans:
                continue
            s, e = spans[turn.index]
            segment = audio16k[int(s * 16_000) : int(e * 16_000)]
            if len(segment) < 16_000:  # under a second says little about the voice
                continue
            similarity[str(turn.index)] = round(
                float(embed(segment) @ references[turn.speaker]), 4
            )
        record["speaker_similarity"] = similarity
        print(
            f"chunk {record['index']}: {json.dumps({k: v['wer'] for k, v in record['asr'].items()})}"
        )

    # CPU timings mean little on a busy machine (e.g. an LLM running on the CPU).
    result["loadavg_after_score"] = os.getloadavg()
    result["summary"] = summarise(result)


def summarise(result: dict[str, Any]) -> dict[str, Any]:
    chunks = result["chunks"]
    audio = sum(c["audio_secs"] for c in chunks)
    synth = sum(c["synth_secs"] for c in chunks)
    sims = [s for c in chunks for s in c.get("speaker_similarity", {}).values()]
    summary: dict[str, Any] = {
        "chunks": len(chunks),
        "audio_secs": round(audio, 1),
        "rtf": round(synth / max(audio, 1e-6), 3),
        "peak_device_mib": result["peak_device_mib"],
        "peak_ours_mib": result["peak_ours_mib"],
        "speaker_similarity_mean": round(float(np.mean(sims)), 4) if sims else None,
        "speaker_similarity_min": round(float(np.min(sims)), 4) if sims else None,
    }
    for size in WHISPER_SIZES:
        rows = [c["asr"][size] for c in chunks if "asr" in c]
        if not rows:
            continue
        cpu = sum(r["cpu_secs"] for r in rows)
        summary[size] = {
            "wer_mean": round(float(np.mean([r["wer"] for r in rows])), 4),
            "wer_max": round(float(np.max([r["wer"] for r in rows])), 4),
            "quote_hits": f"{sum(r['quote_hits'] for r in rows)}/{sum(r['quotes'] for r in rows)}",
            "cpu_rtf": round(cpu / max(audio, 1e-6), 3),
        }
    return summary


# --------------------------------------------------------------------------
# Voices and report


def libritts_rows(page: int = 100):
    """Pages through LibriTTS-R test-clean rows (transcripts plus signed audio URLs)."""
    dataset = urllib.parse.quote(LIBRITTS_R, safe="")
    offset = 0
    while True:
        url = (
            f"https://datasets-server.huggingface.co/rows?dataset={dataset}"
            f"&config=clean&split=test.clean&offset={offset}&length={page}"
        )
        with urllib.request.urlopen(url, timeout=120) as response:
            rows = json.load(response)["rows"]
        if not rows:
            return
        yield from (r["row"] for r in rows)
        offset += len(rows)


def fetch_voices(target: Path) -> None:
    """Downloads one 6-11 s LibriTTS-R clip for each speaker in VOICE_SPEAKERS."""
    target.mkdir(parents=True, exist_ok=True)
    found: dict[str, dict[str, str]] = {}
    for row in libritts_rows():
        speaker = row["speaker_id"]
        # 15-28 words is roughly 6-11 s of read speech; check the real length below.
        if (
            speaker not in VOICE_SPEAKERS
            or speaker in found
            or not 15 <= len(row["text_original"].split()) <= 28
        ):
            continue
        with urllib.request.urlopen(row["audio"][0]["src"], timeout=120) as response:
            audio, rate = sf.read(io.BytesIO(response.read()), dtype="float32")
        secs = len(audio) / rate
        if not 6.0 <= secs <= 11.0:
            continue
        number = VOICE_SPEAKERS.index(speaker) + 1
        name = f"voice_{number}_{row['id']}.wav"
        sf.write(target / name, to_mono(audio), rate)
        found[speaker] = {
            "wav": name,
            "transcript": row["text_original"],
            "licence": "CC-BY-4.0",
            "source": f"LibriTTS-R test-clean {row['id']} (speaker {speaker}), https://www.openslr.org/141/",
        }
        print(f"voice {number}: {name} ({secs:.1f}s) {row['text_original']}")
        if len(found) == len(VOICE_SPEAKERS):
            break
    missing = [s for s in VOICE_SPEAKERS if s not in found]
    if missing:
        raise SystemExit(f"no 6-11 s clip found for speakers {missing}")
    entries = [found[s] for s in VOICE_SPEAKERS]
    (target / "voices.json").write_text(json.dumps(entries, indent=2) + "\n")


def report(out: Path) -> None:
    rows = []
    for path in sorted(out.glob("*/results.json")):
        r = json.loads(path.read_text())
        s = r.get("summary") or summarise(r)
        rows.append((r, s))
    header = (
        "| backend | peak VRAM (device / ours, MiB) | RTF | chunks | audio s "
        "| WER base.en | WER small.en | quotes base / small | spk sim mean / min "
        "| CPU RTF base / small |"
    )
    print(header)
    print("|---" * (header.count("|") - 1) + "|")
    for r, s in rows:
        b, m = s.get("base.en", {}), s.get("small.en", {})
        print(
            f"| {r['backend']} | {s['peak_device_mib']} / {s['peak_ours_mib']} | {s['rtf']} | {s['chunks']} "
            f"| {s['audio_secs']} | {b.get('wer_mean')} | {m.get('wer_mean')} "
            f"| {b.get('quote_hits')} / {m.get('quote_hits')} "
            f"| {s['speaker_similarity_mean']} / {s['speaker_similarity_min']} "
            f"| {b.get('cpu_rtf')} / {m.get('cpu_rtf')} |"
        )


def main() -> None:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--backend", choices=sorted(BACKENDS))
    parser.add_argument("--script", type=Path, help="a Podling script.json")
    parser.add_argument("--voices", type=Path, default=HERE / "voices" / "voices.json")
    parser.add_argument("--out", type=Path, default=HERE / "out")
    parser.add_argument(
        "--chunks", type=int, default=6, help="synthesise at most this many chunks"
    )
    parser.add_argument("--max-chunk-secs", type=float)
    parser.add_argument("--skip-score", action="store_true")
    parser.add_argument(
        "--score-only", action="store_true", help="rescore existing WAVs"
    )
    parser.add_argument(
        "--allow-ollama",
        action="store_true",
        help="run even with an Ollama model on the GPU",
    )
    parser.add_argument("--fetch-voices", action="store_true")
    parser.add_argument("--report", action="store_true")
    args = parser.parse_args()

    if args.fetch_voices:
        fetch_voices(args.voices.parent)
        return
    if args.report:
        report(args.out)
        return
    if not args.backend or not args.script:
        parser.error("--backend and --script are required")

    cast, turns = load_script(args.script)
    voices = load_voices(args.voices, cast)
    out = args.out / args.backend
    out.mkdir(parents=True, exist_ok=True)
    results_path = out / "results.json"
    if args.score_only:
        result = json.loads(results_path.read_text())
    else:
        result = synthesise(args, cast, turns, voices, out)
        results_path.write_text(json.dumps(result, indent=2) + "\n")
    if not args.skip_score:
        score(result, turns, voices, out)
        results_path.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result.get("summary", {}), indent=2))


if __name__ == "__main__":
    main()
