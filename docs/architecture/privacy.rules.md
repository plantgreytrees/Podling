---
area: privacy
governs: ["crates/podling-core/src/plugin/http.rs", "crates/podling-core/src/plugin/openai.rs", "crates/podling-core/src/plugin/openai_embeddings.rs", "crates/podling-core/src/plugin/ollama.rs", "crates/podling-types/src/episode.rs"]
human: docs/architecture/privacy.md
source: docs/ideas/story-driven-script.md
verified_at: 6eeb1c984f579fd78d0cfd55bbd5618bd2d4fc02
updated: 2026-10-07
---
# ARCH privacy — enforced rules

- **ARCH-PRIVACY-01** [decided] MUST classify an HTTP provider's `base_url` host as local only when it is `localhost`, a loopback IP literal (127.0.0.0/8, ::1) or a private IP literal (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, fc00::/7); every other host, including any other hostname, is hosted — check: unit tests for `localhost`, `127.5.0.1`, `[::1]`, `10.0.0.5`, `192.168.1.2`, `[fd00::1]` (local) and `localhost.example.com`, `127.0.0.1.nip.io`, `192.169.0.1`, `172.32.0.1`, `api.together.xyz` (hosted) — cite: crates/podling-core/src/plugin/http.rs:389
- **ARCH-PRIVACY-02** [decided] MUST enforce the policy in `Transport::new`, which every HTTP provider uses, refusing a hosted `base_url` with a config error naming the section and the fix unless that section declares `data_policy = "zero_retention"`; MUST NOT build an HTTP agent anywhere else — check: `Transport::new` tests for refused, declared and local; `ureq::Agent` built only in `http.rs` — cite: crates/podling-core/src/plugin/http.rs:83
- **ARCH-PRIVACY-03** [decided] MUST add `data_policy: Option<DataPolicy>` (an enum whose only variant is `ZeroRetention`, serialised `zero_retention`) to `LlmConfig::OpenAiCompat` and `EmbeddingConfig::OpenAiCompat`, skipped when `None`, and MUST bump `SCHEMA_VERSION` and refresh the schema snapshot — check: snapshot diff shows only the new optional field; `SCHEMA_VERSION` incremented — cite: crates/podling-types/src/episode.rs:125
- **ARCH-PRIVACY-04** [decided] MUST pass `[llm]`'s `data_policy` to its Ollama-unload transport, and none to the TTS sidecar transport, so a non-local sidecar is refused — check: `OllamaUnload::new` and `SidecarTts` transport configs; a test with a hosted sidecar URL — cite: crates/podling-core/src/plugin/ollama.rs:27
- **ARCH-PRIVACY-05** [decided] MUST include `data_policy` in `OpenAiCompat` and `OpenAiEmbeddings` fingerprints, never the API key — check: fingerprint tests — cite: crates/podling-core/src/plugin/openai.rs:210
- **ARCH-PRIVACY-06** [decided] MUST keep redirects disabled (`max_redirects(0)`) and the rejection of credentials in `base_url` — check: both remain in `http.rs` with tests — cite: crates/podling-core/src/plugin/http.rs:129
- **ARCH-PRIVACY-07** [decided] MUST log at `info`, once per transport at construction, the section, `local` or `hosted`, and the declared `data_policy`, never the key — check: `tracing::info!` in `Transport::new` — cite: crates/podling-core/src/plugin/http.rs:93
- **ARCH-PRIVACY-08** [decided] MUST translate `data_policy` into a host's machine-readable control when Podling supports one (e.g. OpenRouter's `provider.data_collection = "deny"` and `zdr = true`) instead of adding a new config field, bumping `openai.rs`'s `PROMPT_VERSION` when the request body changes — check: no new privacy field beside `data_policy`; request-body test for the translated host — cite: crates/podling-core/src/plugin/openai.rs:26
