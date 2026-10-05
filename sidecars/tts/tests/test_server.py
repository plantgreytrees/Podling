"""Protocol v1 against the `fake` backend, over real HTTP on 127.0.0.1."""

from __future__ import annotations

import http.client
import json
import struct
import subprocess
import sys
import threading
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest

from podling_tts.backends import BackendError
from podling_tts.backends.fake import FakeBackend
from podling_tts.protocol import PREFIX, PROTOCOL
from podling_tts.server import Worker, build_server


class Client:
    def __init__(self, port: int):
        self.port = port

    def request(
        self,
        method: str,
        route: str,
        body: Any = None,
        *,
        raw: bytes | None = None,
        headers: dict[str, str] | None = None,
    ) -> tuple[int, dict]:
        conn = http.client.HTTPConnection("127.0.0.1", self.port, timeout=10)
        data = (
            raw
            if raw is not None
            else (None if body is None else json.dumps(body).encode())
        )
        sent = {"Content-Type": "application/json"} if data is not None else {}
        sent.update(headers or {})
        conn.request(method, f"{PREFIX}{route}", body=data, headers=sent)
        response = conn.getresponse()
        payload = json.loads(response.read() or b"{}")
        conn.close()
        return response.status, payload

    def synthesize(self, body: Any) -> tuple[int, dict]:
        return self.request("POST", "/synthesize", body)


def serve(backend, run_dir: Path) -> Iterator[Client]:
    server = build_server(Worker(backend, run_dir), 0)
    thread = threading.Thread(
        target=server.serve_forever, kwargs={"poll_interval": 0.02}, daemon=True
    )
    thread.start()
    try:
        yield Client(server.server_address[1])
    finally:
        server.shutdown()
        server.server_close()


@pytest.fixture
def run_dir(tmp_path: Path) -> Path:
    run = tmp_path / "run"
    run.mkdir()
    (run / "host.wav").write_bytes(b"RIFF")
    (run / "guest.wav").write_bytes(b"RIFF")
    return run.resolve()


@pytest.fixture
def client(run_dir: Path) -> Iterator[Client]:
    yield from serve(FakeBackend(), run_dir)


def request(run_dir: Path, **overrides: Any) -> dict:
    body = {
        "turns": [
            {"speaker": "host", "text": "In nineteen oh eight"},
            {"speaker": "guest", "text": "the sky split in two"},
        ],
        "voices": {
            "host": {
                "reference": str(run_dir / "host.wav"),
                "transcript": "Hello there.",
            },
            "guest": {"reference": str(run_dir / "guest.wav"), "transcript": "Hi."},
        },
        "seed": 7,
        "out_path": str(run_dir / "chunk.wav"),
    }
    body.update(overrides)
    return body


def read_f32_wav(path: Path) -> tuple[int, int]:
    """(sample rate, samples) of a mono float WAV, checking the header we write."""
    data = path.read_bytes()
    assert data[:4] == b"RIFF" and data[8:12] == b"WAVE"
    fmt, channels, rate = struct.unpack("<HHI", data[20:28])
    assert (fmt, channels) == (3, 1)
    assert struct.unpack("<I", data[4:8])[0] == len(data) - 8
    data_at = data.index(b"data")
    size = struct.unpack("<I", data[data_at + 4 : data_at + 8])[0]
    assert size == len(data) - data_at - 8
    return rate, size // 4


# --------------------------------------------------------------------------
# 3.1 / 3.2: health, synthesis, validation


def test_health_reports_protocol_backend_and_capabilities(client: Client) -> None:
    status, body = client.request("GET", "/health")
    assert status == 200
    assert body["protocol"] == PROTOCOL
    assert body["backend"] == "fake"
    assert body["capabilities"] == {
        "multi_speaker": False,
        "max_chunk_secs": 120,
        "max_speakers": 8,
        "native_sample_rate": 24_000,
        "context": False,
    }


def test_synthesize_writes_a_wav_with_exact_turn_spans(
    client: Client, run_dir: Path
) -> None:
    status, body = client.synthesize(request(run_dir))
    assert status == 200, body
    rate, samples = read_f32_wav(run_dir / "chunk.wav")
    assert (rate, samples) == (body["sample_rate"], body["samples"])
    spans = body["turn_spans"]
    assert len(spans) == 2
    assert spans[0][0] == 0 and spans[0][1] == spans[1][0] and spans[1][1] == samples
    # Four words then five words at 0.25 s each.
    assert spans[0][1] == 4 * 6_000
    assert body["dropped"] == [] and body["clips"] == []


def test_synthesis_is_deterministic(client: Client, run_dir: Path) -> None:
    client.synthesize(request(run_dir))
    first = (run_dir / "chunk.wav").read_bytes()
    client.synthesize(request(run_dir))
    assert (run_dir / "chunk.wav").read_bytes() == first


def test_unsupported_features_are_dropped_and_backchannels_rendered(
    client: Client, run_dir: Path
) -> None:
    turns = [
        {
            "speaker": "host",
            "text": "It flattened the forest",
            "emotion": "awed",
            "nonverbal": [
                {"kind": "laugh", "by": "host", "at": "after"},
                {"kind": "backchannel", "by": "guest", "at": "over", "text": "wow"},
            ],
        }
    ]
    status, body = client.synthesize(request(run_dir, turns=turns))
    assert status == 200, body
    assert body["dropped"] == [
        {"turn": 0, "kind": "emotion"},
        {"turn": 0, "kind": "laugh"},
    ]
    [clip] = body["clips"]
    assert clip["by"] == "guest" and clip["at"] == "over" and clip["index"] == 1
    assert Path(clip["path"]).parent == run_dir
    assert read_f32_wav(Path(clip["path"]))[1] == clip["samples"] == 6_000


@pytest.mark.parametrize(
    ("change", "reason"),
    [
        ({"extra": 1}, "unknown field(s): extra"),
        ({"seed": "7"}, "seed must be an integer"),
        ({"seed": True}, "seed must be an integer"),
        ({"seed": 2**64}, "unsigned 64-bit"),
        ({"seed": -1}, "unsigned 64-bit"),
        ({"turns": []}, "turns must not be empty"),
        (
            {"turns": [{"speaker": "host", "text": "hi", "pace": "quick"}]},
            "unknown field(s): pace",
        ),
        ({"turns": [{"speaker": "narrator", "text": "hi"}]}, "has no voice"),
        ({"turns": [{"speaker": "host", "text": "  "}]}, "must not be empty"),
        (
            {
                "turns": [
                    {
                        "speaker": "host",
                        "text": "hi",
                        "nonverbal": [
                            {"kind": "backchannel", "by": "guest", "at": "over"}
                        ],
                    }
                ]
            },
            "text is required for a backchannel",
        ),
        (
            {
                "turns": [
                    {
                        "speaker": "host",
                        "text": "hi",
                        "nonverbal": [
                            {"kind": "laugh", "by": "host", "at": "over", "text": "ha"}
                        ],
                    }
                ]
            },
            "only allowed on a backchannel",
        ),
        (
            {
                "turns": [
                    {
                        "speaker": "host",
                        "text": "hi",
                        "nonverbal": [{"kind": "cough", "by": "host", "at": "over"}],
                    }
                ]
            },
            "must be one of",
        ),
        ({"voices": {}}, "voices must be a non-empty object"),
        ({"context": {"turns": [], "previous": "x"}}, "unknown field(s): previous"),
    ],
)
def test_malformed_requests_are_400_with_a_reason(
    client: Client, run_dir: Path, change: dict, reason: str
) -> None:
    status, body = client.synthesize(request(run_dir, **change))
    assert status == 400
    assert reason in body["error"]


def test_missing_field_is_400(client: Client, run_dir: Path) -> None:
    body = request(run_dir)
    del body["seed"]
    status, reply = client.synthesize(body)
    assert status == 400 and "missing field(s): seed" in reply["error"]


def test_a_non_json_body_is_400(client: Client) -> None:
    status, body = client.request("POST", "/synthesize", raw=b"{not json")
    assert status == 400 and "not JSON" in body["error"]


def test_context_paths_are_validated_and_accepted(
    client: Client, run_dir: Path
) -> None:
    context = {
        "turns": [{"speaker": "guest", "text": "Earlier"}],
        "audio": str(run_dir / "host.wav"),
        "callbacks": [str(run_dir / "guest.wav")],
    }
    status, body = client.synthesize(request(run_dir, context=context))
    assert status == 200, body
    # The fake cannot listen to context, and says so.
    assert body["dropped"] == [{"kind": "context"}]


def test_the_speakers_own_backchannel_before_or_after_is_said_in_line(
    client: Client, run_dir: Path
) -> None:
    turns = [
        {
            "speaker": "host",
            "text": "It flattened the forest",
            "nonverbal": [
                {"kind": "backchannel", "by": "host", "at": "before", "text": "Well,"},
                {"kind": "backchannel", "by": "host", "at": "after", "text": "really."},
            ],
        }
    ]
    status, body = client.synthesize(request(run_dir, turns=turns))
    assert status == 200, body
    assert body["dropped"] == [] and body["clips"] == []
    # Four words plus two, a quarter second each.
    assert body["turn_spans"] == [[0, 6 * 6_000]]


# --------------------------------------------------------------------------
# 3.3: paths stay inside the run directory


def test_parent_dir_escape_is_400(client: Client, run_dir: Path) -> None:
    status, body = client.synthesize(
        request(run_dir, out_path=str(run_dir / ".." / "x.wav"))
    )
    assert status == 400 and "outside the run directory" in body["error"]
    voices = request(run_dir)["voices"]
    voices["host"]["reference"] = str(run_dir / ".." / "run" / ".." / "secret.wav")
    status, body = client.synthesize(request(run_dir, voices=voices))
    assert status == 400 and "outside the run directory" in body["error"]


def test_symlink_escape_is_400(client: Client, run_dir: Path, tmp_path: Path) -> None:
    secret = tmp_path / "secret.wav"
    secret.write_bytes(b"RIFF")
    (run_dir / "link.wav").symlink_to(secret)
    voices = request(run_dir)["voices"]
    voices["host"]["reference"] = str(run_dir / "link.wav")
    status, body = client.synthesize(request(run_dir, voices=voices))
    assert status == 400 and "outside the run directory" in body["error"]

    outside = tmp_path / "outside"
    outside.mkdir()
    (run_dir / "out").symlink_to(outside, target_is_directory=True)
    status, body = client.synthesize(
        request(run_dir, out_path=str(run_dir / "out" / "c.wav"))
    )
    assert status == 400 and "outside the run directory" in body["error"]
    assert not (outside / "c.wav").exists()


def test_an_existing_output_symlink_is_refused(
    client: Client, run_dir: Path, tmp_path: Path
) -> None:
    target = tmp_path / "target.wav"
    target.write_bytes(b"keep")
    (run_dir / "chunk.wav").symlink_to(target)
    status, _ = client.synthesize(request(run_dir))
    assert status == 400
    assert target.read_bytes() == b"keep"


@pytest.mark.parametrize(
    "path", ["chunk.wav", "/", "{run}", "{run}/chunk.mp3", "{run}/no/c.wav"]
)
def test_bad_output_paths_are_400(client: Client, run_dir: Path, path: str) -> None:
    status, _ = client.synthesize(request(run_dir, out_path=path.format(run=run_dir)))
    assert status == 400


# --------------------------------------------------------------------------
# HTTP-level defences


def test_wrong_content_type_is_415(client: Client, run_dir: Path) -> None:
    status, _ = client.request(
        "POST", "/synthesize", request(run_dir), headers={"Content-Type": "text/plain"}
    )
    assert status == 415


def test_foreign_host_header_is_403(client: Client) -> None:
    status, _ = client.request("GET", "/health", headers={"Host": "evil.example:80"})
    assert status == 403


def test_oversized_body_is_413(client: Client) -> None:
    # The server refuses on the declared length, before reading the body.
    status, _ = client.request(
        "POST", "/synthesize", raw=b"{}", headers={"Content-Length": str((1 << 20) + 1)}
    )
    assert status == 413


def test_unknown_route_is_404(client: Client) -> None:
    assert client.request("GET", "/nope")[0] == 404
    assert client.request("POST", "/nope", {})[0] == 404


def test_unload_takes_an_empty_object(client: Client) -> None:
    # The fake backend has no model to free, so it stays loaded.
    assert client.request("POST", "/unload", {}) == (200, {"loaded": True})
    assert client.request("POST", "/unload", {"now": True})[0] == 400


class Broken(FakeBackend):
    def synthesize(self, request):
        raise BackendError("only 900 MiB of GPU memory is free")


def test_a_backend_failure_is_503_with_its_message(run_dir: Path) -> None:
    for client in serve(Broken(), run_dir):
        status, body = client.synthesize(request(run_dir))
        assert status == 503 and "900 MiB" in body["error"]


# --------------------------------------------------------------------------
# The real entry point


def test_the_command_prints_where_it_listens_and_answers_health(tmp_path: Path) -> None:
    proc = subprocess.Popen(
        [
            sys.executable,
            "-m",
            "podling_tts.server",
            "--port",
            "0",
            "--backend",
            "fake",
            "--run-dir",
            str(tmp_path),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        ready = json.loads(proc.stdout.readline())
        host, port = ready["listening"].split(":")
        assert host == "127.0.0.1" and ready["protocol"] == PROTOCOL
        status, body = Client(int(port)).request("GET", "/health")
        assert status == 200 and body["backend"] == "fake"
    finally:
        proc.terminate()
        proc.wait(timeout=10)
