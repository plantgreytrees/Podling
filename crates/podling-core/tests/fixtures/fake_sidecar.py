"""A stand-in TTS worker for Podling's tests: just enough of protocol v1.

`--mode` picks a misbehaviour; Podling appends `--port 0 --run-dir <dir>`.
Every request body is saved to `<run-dir>/last_request.json`.
"""

import argparse
import json
import math
import os
import signal
import struct
import sys
import time
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

parser = argparse.ArgumentParser()
parser.add_argument("--mode", default="ok")
parser.add_argument("--tag", default="")  # lets a test find this process
parser.add_argument("--port", type=int, required=True)
parser.add_argument("--run-dir", type=Path, required=True)
args = parser.parse_args()

if args.mode == "crash":
    print("torch.OutOfMemoryError: CUDA out of memory.", file=sys.stderr, flush=True)
    sys.exit(3)
if args.mode == "hang":
    time.sleep(600)
if args.mode == "wrong-protocol":
    print(json.dumps({"listening": "127.0.0.1:9", "protocol": 2}), flush=True)
    time.sleep(600)
if args.mode == "stubborn":
    signal.signal(signal.SIGTERM, signal.SIG_IGN)
if args.mode == "orphan":
    # Like a model process behind `uv run`: a child of the worker that ignores
    # SIGTERM. An ignored signal stays ignored across exec, so it is ignored
    # from the child's first instruction, and its command line carries the tag.
    import subprocess

    signal.signal(signal.SIGTERM, signal.SIG_IGN)
    subprocess.Popen(
        [sys.executable, "-c", "import time; time.sleep(60)", args.tag],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    signal.signal(signal.SIGTERM, signal.SIG_DFL)


def wav(samples: int, rate: int) -> bytes:
    # A quiet 220 Hz tone: loudness meters filter out a constant (DC) signal.
    tone = [0.1 * math.sin(2 * math.pi * 220 * i / rate) for i in range(samples)]
    data = struct.pack(f"<{samples}f", *tone)
    fmt = struct.pack("<HHIIHH", 3, 1, rate, rate * 4, 4, 32)
    return (
        b"RIFF"
        + struct.pack("<I", 4 + 8 + len(fmt) + 8 + len(data))
        + b"WAVE"
        + b"fmt "
        + struct.pack("<I", len(fmt))
        + fmt
        + b"data"
        + struct.pack("<I", len(data))
        + data
    )


class Handler(BaseHTTPRequestHandler):
    def reply(self, status: int, body: dict) -> None:
        raw = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(raw)))
        self.end_headers()
        self.wfile.write(raw)

    def do_GET(self) -> None:
        self.reply(
            200,
            {
                "protocol": 1,
                "backend": "stub",
                "model": "stub-model",
                "weights": "abc123",
                "loaded": False,
                "capabilities": {
                    "multi_speaker": False,
                    "max_chunk_secs": 120,
                    "max_speakers": 8,
                    "native_sample_rate": 24000,
                },
            },
        )

    def do_POST(self) -> None:
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        (args.run_dir / "last_request.json").write_text(json.dumps(body))
        if args.mode == "busy":
            self.reply(503, {"error": "only 900 MiB of GPU memory is free"})
            return
        if args.mode == "die":
            sys.stderr.write("Segmentation fault in the model\n")
            sys.stderr.flush()
            os._exit(9)
        rate, per_turn = 24000, 2400
        turns = len(body["turns"])
        Path(body["out_path"]).write_bytes(wav(per_turn * turns, rate))
        reported = per_turn * turns + (1 if args.mode == "liar" else 0)
        self.reply(
            200,
            {
                "sample_rate": rate,
                "samples": reported,
                "turn_spans": [
                    [i * per_turn, (i + 1) * per_turn] for i in range(turns)
                ],
                "dropped": [{"turn": 0, "kind": "emotion"}],
                "clips": [],
            },
        )

    def log_message(self, *_: object) -> None:
        pass


server = HTTPServer(("127.0.0.1", args.port), Handler)
print(
    json.dumps({"listening": f"127.0.0.1:{server.server_port}", "protocol": 1}),
    flush=True,
)
server.serve_forever()
