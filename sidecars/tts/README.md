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

## Pronunciation

When the model says a name wrong, respell it. Put the names you always want
respelt in `pronounce.toml` beside the `sidecars.toml` in use (with
`--sidecars /x/sidecars.toml`, that is `/x/pronounce.toml`):

```toml
[pronounce]
# The respelling only:
Kulik = "Koolick"
# Or with what Whisper writes when the name is said right, so the speech
# check does not count it as a mistake:
Vanavara = { say = "Vanavahra", heard = ["Vanavarra", "Vana Vara"] }
```

An episode can add or override names in `[tts.pronounce]`, in the same form;
the episode wins per name. No file is no lexicon; a file that does not parse
stops the run before any stage, naming the file.

- Use it for names only, each respelt as one plain word with no hyphens. In
  the word test, respelling ordinary words made them worse.
- A name matches as a whole word, with its case. "Kulik" is respelt in
  "Kulik's", not in "Kuliks" or "kulik".
- **The worker never sees the lexicon.** Podling sends the respelt text as the
  turn's `text`, and nothing else in the request changes. The script, the
  quotes and the speech check keep the real name, so the protocol and the
  adapter version are unchanged.

## Protocol v1

Every route lives under `/v1/podling`, and every body is JSON. A request with an unknown field,
a wrong type or a missing field gets HTTP 400, with the reason in `{"error": ...}`.

- **`GET /health`** returns `{protocol, backend, model, weights, adapter, loaded, capabilities}`.
  - `weights` names the exact weights, so Podling's cache key changes when they do. For a hub
    snapshot it is the snapshot's commit. For `--model-dir` it is `sha256:<hex>` over the
    directory's `*.safetensors` and `*.json` files, computed once at startup.
  - `adapter` is the backend adapter's version (`ADAPTER_VERSION`). It is bumped when the adapter
    changes what the model is asked to say, so cached audio from the old adapter is not reused.
  - `capabilities` is `{multi_speaker, max_chunk_secs, max_speakers, native_sample_rate, context}`.
    `context` says whether the model listens to a request's `context`. When it is false (or missing),
    Podling sends no context and keys no chunk on it, so editing a turn re-synthesises only its chunk.
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
  - A backchannel the speaker says just before or after their own words is said in line, as words.
    Podling only sends those in-voice sounds (`before`/`after` by the turn's speaker); sounds over a
    turn or by someone else are placed by the assembler.
  - `clips` are any other backchannels, rendered as their own files beside `out_path`.
  - `context` is accepted and validated. Neither backend listens to it (Qwen speaks one turn per
    call), so both report `{"kind": "context"}` in `dropped` when one is sent.
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
