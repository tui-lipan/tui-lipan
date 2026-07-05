#!/usr/bin/env python3
import os
from http.server import HTTPServer, SimpleHTTPRequestHandler


class Handler(SimpleHTTPRequestHandler):
    extensions_map = dict(SimpleHTTPRequestHandler.extensions_map)
    extensions_map.update(
        {
            ".cjs": "application/javascript",
            ".css": "text/css",
            ".js": "application/javascript",
            ".json": "application/json",
            ".mjs": "application/javascript",
            ".wasm": "application/wasm",
        }
    )

    def do_GET(self):
        if self.path == "/":
            self.send_response(302)
            self.send_header("Location", "/search_palette/")
            self.end_headers()
            return
        super().do_GET()


if __name__ == "__main__":
    os.chdir(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    port = int(os.environ.get("PORT", "8080"))
    bind = os.environ.get("BIND", "0.0.0.0")
    httpd = HTTPServer((bind, port), Handler)
    print(
        f"Serving http://{bind}:{port}/search_palette/ (correct MIME for .mjs / .wasm)"
    )
    httpd.serve_forever()
