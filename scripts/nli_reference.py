#!/usr/bin/env python3
"""Dump reference NLI probabilities for the candle parity test.

Development only: the pipeline never runs Python. This computes, with
Hugging Face transformers, the softmax scores the Rust cross-encoder must
reproduce, and writes them to
crates/podling-core/tests/fixtures/nli/reference_logits.json.

    hf download cross-encoder/nli-deberta-v3-base --local-dir <dir>
    python3 scripts/nli_reference.py <dir>
"""

import json
import sys
from pathlib import Path

import torch
from transformers import AutoModelForSequenceClassification, AutoTokenizer

PAIRS = [
    # A paraphrase, in both directions: entailment.
    (
        "In June 1908 an explosion flattened about 80 million trees over the Tunguska forest.",
        "About 80 million trees were flattened by an explosion in June 1908.",
    ),
    (
        "About 80 million trees were flattened by an explosion in June 1908.",
        "In June 1908 an explosion flattened about 80 million trees over the Tunguska forest.",
    ),
    # A changed year: contradiction.
    ("The explosion happened in June 1908.", "The explosion happened in June 1907."),
    # Unrelated: neutral.
    ("Leonid Kulik reached the site in 1927.", "No impact crater was found."),
    # Opposite direction: contradiction.
    (
        "The fallen trees pointed away from a central area.",
        "The fallen trees pointed towards a central area.",
    ),
    # A two-sentence premise window: entailment.
    (
        "At breakfast the sky split in two. Fire covered the northern sky above the forest.",
        "Fire covered the sky above the forest.",
    ),
]

LABELS = ("entailment", "neutral", "contradiction")


def main() -> None:
    model_dir = Path(sys.argv[1])
    tokenizer = AutoTokenizer.from_pretrained(model_dir)
    model = AutoModelForSequenceClassification.from_pretrained(model_dir).eval()
    id2label = {int(k): v.lower() for k, v in model.config.id2label.items()}

    out = []
    with torch.no_grad():
        for premise, hypothesis in PAIRS:
            enc = tokenizer(
                premise,
                hypothesis,
                return_tensors="pt",
                truncation=True,
                max_length=512,
            )
            probs = model(**enc).logits.softmax(-1)[0].tolist()
            by_label = {id2label[i]: p for i, p in enumerate(probs)}
            out.append(
                {
                    "premise": premise,
                    "hypothesis": hypothesis,
                    **{k: round(by_label[k], 6) for k in LABELS},
                }
            )

    target = (
        Path(__file__).resolve().parent.parent
        / "crates/podling-core/tests/fixtures/nli/reference_logits.json"
    )
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(
        json.dumps(
            {"model": "cross-encoder/nli-deberta-v3-base", "pairs": out}, indent=2
        )
        + "\n"
    )
    for row in out:
        print(
            f"{row['entailment']:.3f} {row['neutral']:.3f} {row['contradiction']:.3f}"
            f"  {row['hypothesis']}"
        )


if __name__ == "__main__":
    main()
