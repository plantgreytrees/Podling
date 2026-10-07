# Architecture areas

Human index. Enforced rules live in each `<area>.rules.md`; the overall system prose is [`../architecture.md`](../architecture.md).

- [speech](./speech.md) ([rules](./speech.rules.md)) — TTS text, name pronunciation lexicon, speech-check spelling folds, generated voices.
- [story](./story.md) ([rules](./story.rules.md)) — script story shape: claim order, arc prompt, script prompt version, metrics and the gated act writer.
- [privacy](./privacy.md) ([rules](./privacy.rules.md)) — local-by-default HTTP providers; hosted only with a declared zero-retention policy.
