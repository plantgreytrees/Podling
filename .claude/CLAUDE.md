# Podling

AI podcast generator focused on story-driven, source-grounded episodes (narrative non-fiction first; fiction modes later).

## Decisions (2026-09-29)
- **Language:** Rust core (pet project; learning Rust is a goal — explain idioms when introducing them). Python only as isolated model-worker sidecars where no native option exists (e.g. Dia2 / MOSS-TTSD TTS, Docling / PaddleOCR-VL for scanned pages).
- **Modularity:** three plugin kinds — *providers* (LLM, TTS, embeddings, NLI, ASR; LLMs via OpenAI-compatible HTTP), *source connectors* (MCP), *analysers* (quotes, timeline, …; opt-in per episode).
- **Grounding:** a claim ledger, not debating agents. Claims are extracted per chunk, clustered, NLI-scored, and given a deterministic status (Corroborated / SingleSource / Contested / Unsupported); only Contested claims go to an LLM adjudicator.
- **Quotes** are verbatim source spans with offsets; an LLM may select a quote but never write one.
- **Artifacts** are serde types exported as JSON Schema (the language-neutral contract); every stage is cached by a content hash of its inputs.
- **Licensing:** assume possible commercial or open-source release. Avoid non-commercial weights (e.g. Bespoke-MiniCheck-7B), AGPL dependencies (PyMuPDF), and revenue-capped weights (Marker).
- **Hardware target:** RTX 5060 (8 GB VRAM), 62 GB RAM. Models run one stage at a time and are unloaded between stages.
