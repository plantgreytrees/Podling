"""The GPU-free parts of design.py: argument checks and the files it writes."""

import dataclasses
import re
from pathlib import Path

import design
import pytest
import tomllib

EPISODE_RS = Path(__file__).parents[3] / "crates/podling-types/src/episode.rs"


def rust_struct_fields(name: str) -> set[str]:
    source = EPISODE_RS.read_text(encoding="utf-8")
    body = re.search(rf"struct {name} \{{(.*?)\n\}}", source, re.DOTALL)
    assert body, f"struct {name} not found in {EPISODE_RS}"
    return set(re.findall(r"^\s*(\w+):", body.group(1), re.MULTILINE))


def test_provenance_has_exactly_the_rust_fields():
    fields = {f.name for f in dataclasses.fields(design.Provenance)}
    assert fields == rust_struct_fields("RawVoiceProvenance")


def test_the_clip_hash_is_podlings_blake3(tmp_path):
    # The same vector as `the_clip_hash_matches_the_voice_design_tool` in
    # crates/podling-types/tests/roundtrip.rs.
    clip = tmp_path / "host.wav"
    clip.write_bytes(b"podling voice")
    assert (
        design.clip_hash(clip)
        == "86a92eb5d621332263d4a33849079e862874f8de9e3c6f46d43e0f7bceac97ea"
    )


def test_the_licence_is_podlings_generated_licence():
    source = EPISODE_RS.read_text(encoding="utf-8")
    assert f'GENERATED_VOICE_LICENCE: &str = "{design.LICENCE}"' in source


def test_the_provenance_path_appends_to_the_clip_name():
    assert design.provenance_path(Path("voices/host.wav")) == Path(
        "voices/host.wav.provenance.json"
    )
    assert design.provenance_path(Path("a.b.wav")) == Path("a.b.wav.provenance.json")
    assert design.transcript_path(Path("voices/host.wav")) == Path("voices/host.txt")


def test_the_voice_line_is_a_valid_cast_voice():
    text = 'He said "north", then left.'
    line = design.voice_line(Path("voices/host.wav"), text)
    voice = tomllib.loads(line)["voice"]
    assert voice == {
        "reference": "voices/host.wav",
        "transcript": text,
        "licence": "LicenseRef-Podling-Generated",
    }


def args(tmp_path: Path, **overrides) -> list[str]:
    values = {
        "--description": "A warm, low voice",
        "--text": "The forest fell.",
        "--seed": "7",
        "--out": str(tmp_path / "host.wav"),
        **overrides,
    }
    return [part for pair in values.items() for part in pair]


def test_main_records_the_hash_of_the_clip_it_wrote(tmp_path, monkeypatch):
    import json

    import blake3
    import numpy as np

    # No model or GPU: the snapshot, the memory check and the voice are stubs.
    monkeypatch.setattr(design, "snapshot", lambda model: tmp_path / "0123abcd")
    monkeypatch.setattr(design, "free_mib", lambda: design.NEEDS_MIB)
    tone = np.sin(np.arange(2400, dtype=np.float32) / 8).astype(np.float32)
    monkeypatch.setattr(design, "generate", lambda model_dir, args: (tone, 24000))

    design.main(args(tmp_path))

    clip = tmp_path / "host.wav"
    provenance = json.loads(design.provenance_path(clip).read_text(encoding="utf-8"))
    assert provenance["clip_blake3"] == blake3.blake3(clip.read_bytes()).hexdigest()
    assert provenance["weights_commit"] == "0123abcd"


def test_good_arguments_parse(tmp_path):
    parsed = design.parse_args(args(tmp_path))
    assert (parsed.seed, parsed.out.name, parsed.force) == (7, "host.wav", False)


@pytest.mark.parametrize(
    "overrides",
    [
        {"--description": "  "},
        {"--text": ""},
        {"--seed": "-1"},
        {"--seed": "seven"},
        {"--seed": str(2**32)},
        {"--out": "host.mp3"},
        {"--out": "/no/such/dir/host.wav"},
    ],
)
def test_bad_arguments_are_refused(tmp_path, overrides):
    with pytest.raises(SystemExit) as exit:
        design.parse_args(args(tmp_path, **overrides))
    assert exit.value.code == 2


@pytest.mark.parametrize(
    "existing", ["host.wav", "host.txt", "host.wav.provenance.json"]
)
def test_existing_files_are_kept_unless_forced(tmp_path, existing):
    (tmp_path / existing).write_text("old")
    with pytest.raises(SystemExit):
        design.parse_args(args(tmp_path))
    assert design.parse_args([*args(tmp_path), "--force"]).force


def test_importing_design_loads_no_model_libraries():
    import subprocess
    import sys

    code = "import sys, design; print(sorted({'torch', 'qwen_tts'} & set(sys.modules)))"
    out = subprocess.run(
        [sys.executable, "-c", code],
        cwd=Path(design.__file__).parent,
        capture_output=True,
        text=True,
        check=True,
    )
    assert out.stdout.strip() == "[]"
