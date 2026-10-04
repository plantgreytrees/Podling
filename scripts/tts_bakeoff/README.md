# TTS bake-off (Phase 5, unit 1)

A throwaway measuring script, not part of Podling. It synthesises a Podling
`script.json` with one candidate text-to-speech model at a time on an 8 GB GPU
and records what the Phase 5 plan needs to pick a default backend: peak VRAM,
real-time factor, speaker similarity to the pinned reference clip, and word
error rate and verbatim quote hits under Whisper `base.en` and `small.en` on the
CPU (with the CPU real-time factor of each).

## Candidates

| Backend | Model | Kind | How it fits 8 GB |
|---|---|---|---|
| `dia2` | `nari-labs/Dia2-1B` | dialogue, 2 speakers, ≤ 2 min per call incl. voice prefixes | bf16 |
| `moss` | `OpenMOSS-Team/MOSS-TTSD-v1.0` (8B) | dialogue, 1–5 speakers, long context | Qwen3 backbone in NF4 (bitsandbytes); audio heads bf16 |
| `qwen` | `Qwen/Qwen3-TTS-12Hz-1.7B-Base` | one speaker per call | bf16 |

## Run

```bash
cd scripts/tts_bakeoff
# Dia2's packaging omits its subpackages, so it is installed editable from the
# reviewed commit unpacked here (vendor/ is gitignored).
mkdir -p vendor/dia2 && curl -sL https://github.com/nari-labs/dia2/archive/8687268f4ed3ed20704638fd353b51491de3b476.tar.gz \
  | tar -xz --strip-components=1 -C vendor/dia2
uv sync --extra dia2                       # one backend's environment at a time
uv run --extra dia2 python bakeoff.py --fetch-voices
# Free the GPU first: the script refuses to start while Ollama has a model on it.
docker exec infra_docker_compose-ollama-1 ollama stop llama3.1:8b
uv run --extra dia2 python bakeoff.py --backend dia2 --script fixtures/tunguska-10min.json
uv sync --extra moss && uv run --extra moss python bakeoff.py --backend moss --script fixtures/tunguska-10min.json
uv sync --extra qwen && uv run --extra qwen python bakeoff.py --backend qwen --script fixtures/tunguska-10min.json
uv run --extra qwen python bakeoff.py --report   # markdown table over every backend run
```

`fixtures/tunguska-10min.json` is a hand-written two-host Tunguska dialogue
(51 turns, ~1,360 words, ~9 minutes, 8 verbatim quotes from
`examples/tunguska/sources`) in the shape of Podling's `script.json`. A real
Podling run cannot produce it yet: the example sources total 96 words, so a
grounded script from them is under a minute long. Any Podling `script.json`
(the artifact envelope or a bare body) works as `--script`.

Outputs land in `out/<backend>/`: `chunk_NNN.wav` (native rate, float) and
`results.json` (per-chunk timings, VRAM, transcripts, WER, quote misses, turn
spans and per-turn speaker similarity; the Ollama state before and after).
`--score-only` rescores existing WAVs; `--chunks N` caps the work (default 6).

Turns are packed greedily into chunks at 155 words per minute (Dia2 ≤ 90 s,
because its 2-minute context also holds both voice prefixes; others ≤ 120 s).
Seeds are derived from the chunk index, so a rerun reproduces every take.

## Licences

Every direct dependency, model weight and voice clip is commercial-safe. Dia2's
own `pyproject.toml` depends on `whisper-timestamped` (**AGPL-3.0**) to time the
words of its voice-prefix clips; `pyproject.toml` overrides it out of every
environment and `bakeoff.py` gives Dia2 those timings from Whisper via
transformers instead.

### Python dependencies (direct)

| Package | Licence | Extra |
|---|---|---|
| torch | BSD-3-Clause | all |
| torchaudio | BSD-2-Clause | all |
| numpy | BSD-3-Clause | all |
| soundfile | BSD-3-Clause | all |
| scipy | BSD-3-Clause | all |
| huggingface-hub | Apache-2.0 | all |
| speechbrain | Apache-2.0 | all |
| nvidia-ml-py | BSD-3-Clause | all |
| dia2 (vendored source, commit `8687268`) | Apache-2.0 | dia2 |
| transformers | Apache-2.0 | dia2, moss (qwen via qwen-tts) |
| accelerate | Apache-2.0 | moss |
| bitsandbytes | MIT | moss |
| einops | MIT | moss |
| librosa | ISC | moss |
| tiktoken | MIT | moss |
| torchcodec | BSD-3-Clause | moss |
| qwen-tts 0.1.1 | Apache-2.0 | qwen |

Transitive dependencies were scanned per environment from installed package
metadata: none is GPL, AGPL or non-commercial. The one weak-copyleft package is
`soxr` (LGPL-2.1-or-later, pulled in by librosa in the `moss` and `qwen`
environments), used unmodified as a separately installed library, which LGPL
permits in commercial work. The only non-open entries are
NVIDIA's CUDA runtime wheels (`nvidia-*-cu12`, proprietary but redistributable,
as with any CUDA build of PyTorch). `sphn` (Kyutai, used by Dia2) ships no
licence metadata; its repository is MIT/Apache-2.0.

### Model weights

| Weights | Licence | Used by |
|---|---|---|
| `nari-labs/Dia2-1B` | Apache-2.0 | dia2 |
| `kyutai/mimi` (audio codec) | CC-BY-4.0 (attribution) | dia2 |
| `OpenMOSS-Team/MOSS-TTSD-v1.0` | Apache-2.0 | moss |
| `OpenMOSS-Team/MOSS-Audio-Tokenizer` | Apache-2.0 | moss |
| `Qwen/Qwen3-TTS-12Hz-1.7B-Base` | Apache-2.0 | qwen |
| `Qwen/Qwen3-TTS-Tokenizer-12Hz` | Apache-2.0 | qwen |
| `speechbrain/spkrec-ecapa-voxceleb` | Apache-2.0 | scoring |
| `openai/whisper-base.en`, `openai/whisper-small.en` | Apache-2.0 (Hugging Face card; code MIT) | scoring, Dia2 prefix timing |

### Voice clips

`--fetch-voices` downloads one 6–11 s clip each for LibriTTS-R test-clean
speakers 4446 and 1089 (LibriTTS-R, CC-BY-4.0, https://www.openslr.org/141/);
`voices/voices.json` records each clip's transcript, licence and source.
