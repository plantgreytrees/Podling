# Voice clips

`episode-tts.toml` clones two voices from short reference recordings. Each
`[[cast]]` entry names its clip, says what is spoken in it, and records its
licence, which Podling copies into `audio.json`. Only CC0 or CC-BY clips go
here: many voice datasets are non-commercial (Kyutai's Expresso and EARS
voices, for example, are CC-BY-NC), and a cloned voice carries its clip's
terms.

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
