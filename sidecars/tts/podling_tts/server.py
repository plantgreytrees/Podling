"""The worker's HTTP server: protocol v1 on 127.0.0.1, one request at a time.

Podling starts this process, waits for `/health`, sends `/synthesize` requests and
kills it when synthesis is done; killing the process is what frees the GPU. The
server is deliberately single-threaded: there is one model on one GPU, so
requests are serialised anyway.

Defences, since anything on the machine can reach a localhost port:
- binds 127.0.0.1 only, and answers only requests whose Host header names it (a web
  page cannot reach it through DNS rebinding);
- POSTs must be `application/json`, which a browser cannot send cross-origin
  without a CORS preflight this server never approves;
- bodies are capped, sockets time out, and every path in a request must resolve
  inside `--run-dir`;
- the worker exits if the process that started it dies, so an orphan cannot hold
  the GPU.
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import tempfile
import threading
import time
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from typing import Any

from podling_tts.backends import ADAPTER_VERSION, Backend, BackendError, make_backend
from podling_tts.protocol import PREFIX, PROTOCOL, ProtocolError, parse_synthesize

HOST = "127.0.0.1"
MAX_BODY = 1 << 20
SOCKET_TIMEOUT_SECS = 30

log = logging.getLogger("podling_tts")


class Worker:
    """The state a request handler needs: the backend and the confined run directory."""

    def __init__(self, backend: Backend, run_dir: Path):
        self.backend = backend
        self.run_dir = run_dir.resolve(strict=True)

    def health(self) -> dict[str, Any]:
        b = self.backend
        return {
            "protocol": PROTOCOL,
            "backend": b.name,
            "model": b.model,
            "weights": b.weights(),
            "adapter": ADAPTER_VERSION,
            "loaded": b.loaded,
            "capabilities": b.capabilities(),
        }

    def synthesize(self, body: Any) -> dict[str, Any]:
        request = parse_synthesize(body, self.run_dir)
        started = time.perf_counter()
        result = self.backend.synthesize(request)
        secs = time.perf_counter() - started
        audio = result.samples / result.sample_rate
        log.info(
            "synthesized turns=%d audio_secs=%.1f elapsed_secs=%.1f rtf=%.2f",
            len(request.turns),
            audio,
            secs,
            secs / audio if audio else 0.0,
        )
        return result.to_json()


class Handler(BaseHTTPRequestHandler):
    server_version = "podling-tts"
    timeout = SOCKET_TIMEOUT_SECS
    worker: Worker  # set on the subclass `serve` builds

    def log_message(self, format: str, *args: Any) -> None:
        log.debug("%s %s", self.address_string(), format % args)

    def _reply(self, status: HTTPStatus, payload: dict[str, Any]) -> None:
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _error(self, status: HTTPStatus, reason: str) -> None:
        self._reply(status, {"error": reason})

    def _host_ok(self) -> bool:
        port = self.server.server_address[1]
        if self.headers.get("Host") in (f"{HOST}:{port}", f"localhost:{port}"):
            return True
        self._error(HTTPStatus.FORBIDDEN, "unexpected Host header")
        return False

    def _body(self) -> Any:
        """The JSON body, or None after an error reply has been sent."""
        kind = (self.headers.get("Content-Type") or "").split(";")[0].strip()
        if kind != "application/json":
            self._error(
                HTTPStatus.UNSUPPORTED_MEDIA_TYPE, "body must be application/json"
            )
            return None
        try:
            length = int(self.headers.get("Content-Length", ""))
        except ValueError:
            self._error(HTTPStatus.LENGTH_REQUIRED, "Content-Length is required")
            return None
        if length < 0:
            self._error(HTTPStatus.BAD_REQUEST, "Content-Length is negative")
            return None
        if length > MAX_BODY:
            self._error(
                HTTPStatus.REQUEST_ENTITY_TOO_LARGE, f"body over {MAX_BODY} bytes"
            )
            return None
        try:
            return json.loads(self.rfile.read(length))
        except (UnicodeDecodeError, json.JSONDecodeError) as err:
            self._error(HTTPStatus.BAD_REQUEST, f"body is not JSON: {err}")
            return None

    def do_GET(self) -> None:
        if not self._host_ok():
            return
        if self.path == f"{PREFIX}/health":
            self._reply(HTTPStatus.OK, self.worker.health())
        else:
            self._error(HTTPStatus.NOT_FOUND, f"no route {self.path}")

    def do_POST(self) -> None:
        if not self._host_ok():
            return
        if self.path not in (f"{PREFIX}/synthesize", f"{PREFIX}/unload"):
            self._error(HTTPStatus.NOT_FOUND, f"no route {self.path}")
            return
        body = self._body()
        if body is None:
            return
        try:
            if self.path.endswith("/unload"):
                if body != {}:
                    raise ProtocolError("unload takes an empty object")
                self.worker.backend.unload()
                self._reply(HTTPStatus.OK, {"loaded": self.worker.backend.loaded})
            else:
                self._reply(HTTPStatus.OK, self.worker.synthesize(body))
        except ProtocolError as err:
            self._error(HTTPStatus.BAD_REQUEST, str(err))
        except BackendError as err:
            log.error("backend failed: %s", err)
            self._error(HTTPStatus.SERVICE_UNAVAILABLE, str(err))
        except Exception as err:  # the model's own failures: report, keep serving
            log.exception("synthesis failed")
            self._error(
                HTTPStatus.INTERNAL_SERVER_ERROR, f"{type(err).__name__}: {err}"
            )


def build_server(worker: Worker, port: int) -> HTTPServer:
    handler = type("BoundHandler", (Handler,), {"worker": worker})
    return HTTPServer((HOST, port), handler)


def watch_parent(server: HTTPServer, interval: float = 1.0) -> None:
    """Shuts the server down when the parent process exits (we get re-parented)."""
    parent = os.getppid()

    def watch() -> None:
        while os.getppid() == parent:
            time.sleep(interval)
        log.warning("parent process %d is gone; exiting", parent)
        server.shutdown()

    threading.Thread(target=watch, daemon=True, name="parent-watch").start()


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="podling-tts", description=__doc__.split("\n")[0]
    )
    parser.add_argument("--port", type=int, default=0, help="0 picks a free port")
    parser.add_argument("--backend", default="fake", choices=["fake", "qwen"])
    parser.add_argument(
        "--run-dir",
        type=Path,
        help="every request path must resolve inside it (default: a new temp dir)",
    )
    parser.add_argument("--model-dir", help="local weights instead of the hub cache")
    parser.add_argument("--preload", action="store_true", help="load the model now")
    parser.add_argument("--log-level", default="INFO")
    args = parser.parse_args(argv)
    logging.basicConfig(
        stream=sys.stderr,
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )

    run_dir = args.run_dir or Path(tempfile.mkdtemp(prefix="podling-tts-"))
    options = {"model_dir": args.model_dir} if args.backend != "fake" else {}
    try:
        backend = make_backend(args.backend, **options)
        worker = Worker(backend, run_dir)
        if args.preload:
            backend.load()
    except (BackendError, OSError) as err:
        print(f"podling-tts: {err}", file=sys.stderr)
        raise SystemExit(2) from err

    server = build_server(worker, args.port)
    watch_parent(server)
    host, port = server.server_address[:2]
    # The one line Podling reads from stdout: where to send requests.
    print(json.dumps({"listening": f"{host}:{port}", "protocol": PROTOCOL}), flush=True)
    log.info(
        "serving backend=%s run_dir=%s pid=%d",
        args.backend,
        worker.run_dir,
        os.getpid(),
    )
    try:
        server.serve_forever()
    finally:
        server.server_close()
        backend.unload()


if __name__ == "__main__":
    main()
