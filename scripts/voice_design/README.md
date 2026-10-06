# Voice design

An offline tool that makes a reference clip for an episode's `[[cast]]` from
a written description, with Qwen3-TTS VoiceDesign. Use it when no recorded
voice with a commercial-safe licence fits a speaker.

It is not part of the pipeline and not a sidecar: Podling never runs it, and
`sidecars.toml` never names it. Podling only reads what it writes.

## Run

```bash
cd scripts/voice_design
uv sync
# Download the model once (about 4.5 GB); the tool itself never downloads.
uv run hf download Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign
# One GPU model at a time: free the GPU first (the tool refuses below ~6.2 GB free).
docker exec infra_docker_compose-ollama-1 ollama stop llama3.1:8b
uv run python design.py \
  --description "A woman in her fifties, warm and low, unhurried, a slight rasp" \
  --text "The forest was flattened for two thousand square kilometres." \
  --seed 7 \
  --out ../../examples/tunguska/voices/designed-host.wav
```

It writes three files beside each other, and refuses to replace any of them
without `--force`:

| File | Holds |
|---|---|
| `designed-host.wav` | the clip |
| `designed-host.txt` | its transcript: exactly `--text` |
| `designed-host.wav.provenance.json` | `model`, `weights_commit`, `design_prompt`, `seed`, `tool_version` |

It then prints the `voice = { ... }` line for the speaker's `[[cast]]` entry,
with `licence = "LicenseRef-Podling-Generated"`. Make the `reference` path
relative to the episode file.

Podling refuses a voice with that licence unless a valid
`<clip>.provenance.json` is beside the clip, so a designed voice can always be
told from a recorded one and made again from its prompt and seed.

## Licence policy

Whether a designed clip may carry an open licence (for example CC0-1.0, with
the user as author) is **not decided**. A planned voice marketplace will settle
it. Until then, designed clips are marked `LicenseRef-Podling-Generated`, stay
on the machine that made them, and are not committed.

## Dependencies

Every direct dependency, and its licence:

| Package | Licence |
|---|---|
| `qwen-tts` 0.1.1 | Apache-2.0 |
| `torch`, from the PyTorch cu128 index | BSD-3-Clause |
| `torchaudio`, from the PyTorch cu128 index | BSD-2-Clause |
| `numpy` | BSD-3-Clause |
| `soundfile` | BSD-3-Clause |
| `huggingface-hub` | Apache-2.0 |
| `pytest` (dev only) | MIT |

The model, `Qwen/Qwen3-TTS-12Hz-1.7B-VoiceDesign`, is Apache-2.0. No locked
package is AGPL, non-commercial or revenue-capped. The CUDA runtime wheels that
`torch` pulls in are NVIDIA's redistributable proprietary licence.

## Tests

```bash
uv run pytest -q
```

The tests need no GPU and no model. They check the arguments, the file names,
the printed `voice` line, and that the provenance fields are exactly those of
Podling's `VoiceProvenance`.
