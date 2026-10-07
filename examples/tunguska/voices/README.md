# Voice clips

`episode-tts.toml` clones two voices from short reference recordings. Each
`[[cast]]` entry names its clip, says what is spoken in it, and records its
licence, which Podling copies into `audio.json`. Only CC0 or CC-BY clips go
here: many voice datasets are non-commercial (Kyutai's Expresso and EARS
voices, for example, are CC-BY-NC), and a cloned voice carries its clip's
terms. Podling enforces this: an episode whose clip licence is not exactly
`CC0-1.0`, `CC-BY-3.0`, `CC-BY-4.0` or `LicenseRef-Podling-Generated` (a
self-designed voice, below) is refused when it is read.

| File | Speaker | Clip | Licence | Source |
|---|---|---|---|---|
| `host.wav` | Mara (host) | LibriTTS-R test-clean `4446_2275_000016_000000`, 6.2 s | CC-BY-4.0 | [OpenSLR 141](https://www.openslr.org/141/) |
| `guest.wav` | Tomas (co-host) | LibriTTS-R test-clean `1089_134691_000002_000001`, 7.3 s | CC-BY-4.0 | [OpenSLR 141](https://www.openslr.org/141/) |
| `tone-host.wav`, `tone-guest.wav` | the offline example (`episode.toml`) | plain sine tones made for this repository | CC0-1.0 | this repository |

Attribution: LibriTTS-R (Koizumi et al., 2023), a restored edition of
LibriTTS, which derives from LibriSpeech, under
[CC-BY-4.0](https://creativecommons.org/licenses/by/4.0/). The clips are used
unchanged.

The two recorded clips are not committed (`.gitignore`). To fetch them,
stream the test-clean archive (1.3 GB) and keep only the two files:

```bash
cd examples/tunguska/voices
curl -L https://www.openslr.org/resources/141/test_clean.tar.gz \
  | tar -xz --strip-components=4 \
      LibriTTS_R/test-clean/4446/2275/4446_2275_000016_000000.wav \
      LibriTTS_R/test-clean/1089/134691/1089_134691_000002_000001.wav
mv 4446_2275_000016_000000.wav host.wav
mv 1089_134691_000002_000001.wav guest.wav
```

The transcripts in `episode-tts.toml` are the clips' own (from the archive's
`.normalized.txt` files). If you swap in another voice, change the transcript
and the licence with it: the TTS model conditions on both the sound and the
words.

## Self-designed voices

When no recorded clip fits a speaker, design one from a description with the
offline tool in [`scripts/voice_design/`](../../../scripts/voice_design/README.md)
(Qwen3-TTS VoiceDesign, Apache-2.0). It writes the clip, its transcript and
`<clip>.wav.provenance.json`, and prints the `voice = {...}` line to paste
into the speaker's `[[cast]]` entry:

```toml
voice = { reference = "voices/designed-host.wav", transcript = "The forest was flattened for two thousand square kilometres.", licence = "LicenseRef-Podling-Generated" }
```

Podling accepts `LicenseRef-Podling-Generated` only with a valid provenance
file beside the clip (`designed-host.wav.provenance.json`: `model`,
`weights_commit`, `design_prompt`, `seed`, `tool_version`, `clip_blake3`). Without one, or if its hash is not the clip's, the
run stops before any stage, naming the speaker and the missing file.

Whether a designed clip may carry an open licence is not decided yet, so
designed clips stay on the machine that made them and are not committed.
