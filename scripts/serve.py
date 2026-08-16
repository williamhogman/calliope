#!/usr/bin/env python3
"""Calliope dev server.

http.server with caching disabled: python's SimpleHTTPRequestHandler never
sends Cache-Control, so browsers heuristically cache JS/WASM and can pair a
stale wasm-bindgen glue with a freshly rebuilt binary. no-store keeps the
preview honest; production (dist/) relies on the version stamp instead.
"""
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
        super().end_headers()

    def log_message(self, *args):
        pass  # keep the daemon log quiet


if __name__ == "__main__":
    server = ThreadingHTTPServer(
        ("0.0.0.0", 8080), partial(Handler, directory="game/web")
    )
    server.serve_forever()
