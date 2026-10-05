# podling-tts: the text-to-speech worker

Podling's Rust core has no native text-to-speech model good enough for
podcast dialogue, so speech comes from a Python worker: a small HTTP
server on `127.0.0.1` that runs one TTS model on the GPU. Podling starts
it just before synthesis and kills it right after. Killing the process
is the only reliable way to give the 8 GB card back.

The server and the `fake` backend use only the Python standard library.
Each model backend is a uv extra with its own dependencies.

| Backend | Model | Kind | Notes |
|---|---|---|---|
| `qwen` (default) | `Qwen/Qwen3-TTS-12Hz-1.7B-Base` | one speaker per call | ~4.8–5.8 GB VRAM, RTF ≈ 0.6 on an RTX 5060 |
| `fake` | sine tones, one per word | one speaker per call | for tests; no model, no GPU |

The Phase 5 plan (`docs/plans/phase5-tts-audio.md`, "Spike results") says why Qwen won.
Dia2-1B is the documented fallback, and it is built only if per-turn banter sounds wrong.

## Install

You need [uv](https://docs.astral.sh/uv/) and, for `qwen`, an NVIDIA GPU with a CUDA 12.8 driver.

```bash
cd sidecars/tts
uv sync --extra qwen                     # Python 3.11, torch cu128, qwen-tts
uv run --extra qwen hf download Qwen/Qwen3-TTS-12Hz-1.7B-Base   # ~4.5 GB, once
uv run pytest                            # protocol tests on the fake backend
```

The worker never downloads anything itself. A missing model is an error that prints the command above.

To try the worker by hand:

```bash
uv run podling-tts --port 0 --backend fake
# {"listening": "127.0.0.1:40933", "protocol": 1}
curl http://127.0.0.1:40933/v1/podling/health
```

`qwen-tts` prints warnings about SoX and flash-attn at import. Neither is used, so ignore them.

## Telling Podling about it

The episode file never says what program to run. It only names a profile (`[tts] sidecar = "qwen"`).
A shared episode file must not be able to start a process.
Profiles live in the user-level `~/.config/podling/sidecars.toml`:

```toml
[sidecars.qwen]
# argv, never a shell string. Podling appends --port 0 --run-dir <dir>
# and reads the "listening" line from stdout.
program = "/home/you/.local/bin/uv"
args = ["run", "--project", "/path/to/Podling/sidecars/tts", "--extra", "qwen",
        "podling-tts", "--backend", "qwen"]
```

Free the GPU first. The worker checks free VRAM before it loads and refuses with a readable message:

```bash
ollama stop <model>      # or let Podling do it: unload_after = true in [llm]/[embedding]
```

## Protocol v1

Every route lives under `/v1/podling`, and every body is JSON. A request with an unknown field,
a wrong type or a missing field gets HTTP 400, with the reason in `{"error": ...}`.

- **`GET /health`** returns `{protocol, backend, model, weights, loaded, capabilities}`.
  - `weights` is the model snapshot's commit, so Podling's cache key changes when the weights do.
  - `capabilities` is `{multi_speaker, max_chunk_secs, max_speakers, native_sample_rate}`.
- **`POST /synthesize`** takes:

  ```json
  {
    "turns": [{"speaker": "host", "text": "…", "emotion": "awed",
               "nonverbal": [{"kind": "backchannel", "by": "guest", "at": "over", "text": "Wow."}]}],
    "voices": {"host": {"reference": "<run-dir>/host.wav", "transcript": "…"}},
    "seed": 1908,
    "out_path": "<run-dir>/chunk-0001.wav",
    "context": {"turns": [], "audio": null, "callbacks": []}
  }
  ```

  It writes a mono 32-bit float WAV at `out_path` (temp file + rename), then returns
  `{sample_rate, samples, turn_spans, dropped, clips}`:
  - `turn_spans` are `[start, end)` sample ranges per turn.
  - `dropped` lists features the model cannot express. Qwen drops `emotion`, `laugh`, `chuckle` and `sigh`.
  - `clips` are backchannels rendered as their own files beside `out_path`, for the assembler's second track.
  - `context` is accepted and validated. The Qwen backend speaks one turn per call, so it does not use it.
- **`POST /unload`** with `{}` frees the model but keeps the process.

Status codes for other failures:
- 503: the model cannot run, e.g. not enough free VRAM.
- 500: the model failed during synthesis.
- 403: a foreign `Host` header.
- 415: the body is not `application/json`.
- 413: the body is over 1 MiB.

Defences:
- The worker binds `127.0.0.1` only.
- Every path in a request is resolved with symlinks followed, and must lie inside the `--run-dir`
  the worker was started with. Podling copies the voice clips into that directory.
- A `Host` check stops DNS-rebinding pages, and the JSON-only rule forces a CORS preflight,
  which the worker never approves.
- The worker exits when the process that started it dies, so an orphan cannot keep the GPU.

## Licences

Every direct dependency and the model weights are commercial-safe. Nothing is AGPL, non-commercial
or revenue-capped.

### Direct dependencies

| Package | Licence | Extra |
|---|---|---|
| qwen-tts 0.1.1 | Apache-2.0 | qwen |
| torch (cu128) | BSD-3-Clause | qwen |
| torchaudio (cu128) | BSD-2-Clause | qwen |
| numpy | BSD-3-Clause | qwen |
| huggingface-hub | Apache-2.0 | qwen |
| pytest | MIT | dev |
| hatchling (build) | MIT | build |

Points from the audit of the full `qwen` environment (`uv.lock`):
- **`soxr`:** LGPL-2.1-or-later, used as an unmodified, dynamically loaded library, which the LGPL permits.
- **`certifi`, `tqdm`, `orjson`:** they include MPL-2.0, a file-level copyleft that covers only
  changes to their own files.
- **NVIDIA CUDA runtime wheels:** proprietary but redistributable; torch pulls them in.
- **Unused packages:** `qwen-tts` also pulls in `gradio` and `onnxruntime` (Apache-2.0 / MIT),
  which the worker never imports.
- **`sox` package:** BSD. It would call the GPL SoX program, but that program is not installed
  and the worker does not need it.

### Model weights

| Weights | Licence | Source |
|---|---|---|
| Qwen3-TTS-12Hz-1.7B-Base, with its speech tokenizer | Apache-2.0 | https://huggingface.co/Qwen/Qwen3-TTS-12Hz-1.7B-Base |

Voice clips are not part of the worker. Each `[[cast]]` voice in an episode records its own
licence (CC0 or CC-BY only), and Podling copies that licence into `audio.json`.
