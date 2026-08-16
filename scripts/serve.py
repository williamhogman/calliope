#!/usr/bin/env python3
"""Calliope dev server.

http.server with caching disabled: python's SimpleHTTPRequestHandler never
sends Cache-Control, so browsers heuristically cache JS/WASM and can pair a
stale wasm-bindgen glue with a freshly rebuilt binary. no-store keeps the
preview honest; production (dist/) relies on the version stamp instead.

Content-Encoding negotiation (E3.10): when a sibling `<file>.br` exists and
the client accepts `br`, the precompressed artifact is served with the
original file's MIME type. `scripts/build.sh` writes the .br files into
dist/; a tree without them (game/web during dev) falls through untouched.
"""
import os
import sys
from functools import partial
from http.server import SimpleHTTPRequestHandler, ThreadingHTTPServer


class Handler(SimpleHTTPRequestHandler):
    extensions_map = {
        **SimpleHTTPRequestHandler.extensions_map,
        ".js": "text/javascript",
        ".mjs": "text/javascript",
        ".wasm": "application/wasm",
    }

    def end_headers(self):
        self.send_header("Cache-Control", "no-store")
        # E7.8 — cross-origin isolation: every asset is same-origin, so the
        # strict pair costs nothing and unlocks SharedArrayBuffer +
        # high-resolution timers for dev profiling. Production hosting sets
        # its own headers; nothing shipped depends on isolation (ADR-0015).
        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")
        super().end_headers()

    def _brotli_path(self):
        """Path of a servable precompressed sibling, or None."""
        if "br" not in self.headers.get("Accept-Encoding", ""):
            return None
        path = self.translate_path(self.path.split("?", 1)[0].split("#", 1)[0])
        if path.endswith(".br") or not os.path.isfile(path + ".br"):
            return None
        return path

    def _serve_brotli(self, orig, head_only):
        try:
            f = open(orig + ".br", "rb")
        except OSError:
            return False
        try:
            st = os.fstat(f.fileno())
            self.send_response(200)
            self.send_header("Content-Type", self.guess_type(orig))
            self.send_header("Content-Encoding", "br")
            self.send_header("Content-Length", str(st.st_size))
            self.send_header("Vary", "Accept-Encoding")
            self.end_headers()
            if not head_only:
                self.copyfile(f, self.wfile)
            return True
        finally:
            f.close()

    def do_GET(self):
        orig = self._brotli_path()
        if orig and self._serve_brotli(orig, head_only=False):
            return
        super().do_GET()

    def do_HEAD(self):
        orig = self._brotli_path()
        if orig and self._serve_brotli(orig, head_only=True):
            return
        super().do_HEAD()

    def log_message(self, *args):
        pass  # keep the daemon log quiet


if __name__ == "__main__":
    root = sys.argv[1] if len(sys.argv) > 1 else "game/web"
    server = ThreadingHTTPServer(("0.0.0.0", 8080), partial(Handler, directory=root))
    server.serve_forever()
