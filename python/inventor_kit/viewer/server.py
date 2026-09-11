"""Serve only packaged assets and resources named by the current scene."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from importlib.resources import files
import json
import mimetypes
from pathlib import Path
import secrets
from urllib.parse import urlsplit


def create_server(directory, port=0):
    directory = Path(directory)
    static = files(__package__).joinpath("static")
    manifest = json.loads(static.joinpath("manifest.json").read_text(encoding="utf-8"))
    assets = set(manifest["outputs"])
    token = secrets.token_urlsafe(24)

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def do_GET(self):
            authority = f"127.0.0.1:{self.server.server_port}"
            if self.headers.get("Host") != authority:
                self.send_error(403)
                return
            origin = self.headers.get("Origin")
            if origin is not None and origin != "http://" + authority:
                self.send_error(403)
                return
            path = urlsplit(self.path).path
            prefix = f"/{token}/"
            if not path.startswith(prefix):
                self.send_error(404)
                return
            name = path[len(prefix):] or "index.html"
            try:
                if name in assets:
                    body = static.joinpath(name).read_bytes()
                elif name == "state.json":
                    body = (directory / name).read_bytes()
                else:
                    scene = json.loads((directory / "state.json").read_text(encoding="utf-8"))
                    resources = {t["resource"] for t in scene["thumbnails"]}
                    resources.update(b["resource"] for m in scene["meshes"] for b in m["buffers"].values())
                    if name not in resources:
                        self.send_error(404)
                        return
                    body = (directory / name).read_bytes()
            except (FileNotFoundError, IsADirectoryError):
                self.send_error(404)
                return
            mime = {".js": "text/javascript", ".json": "application/json", ".bin": "application/octet-stream"}.get(
                Path(name).suffix, mimetypes.guess_type(name)[0] or "application/octet-stream")
            self.send_response(200)
            self.send_header("Content-Type", mime)
            self.send_header("Content-Length", str(len(body)))
            self.send_header("Cache-Control", "no-store")
            self.send_header("X-Content-Type-Options", "nosniff")
            self.send_header("Referrer-Policy", "no-referrer")
            self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'")
            self.end_headers()
            try:
                self.wfile.write(body)
            except (BrokenPipeError, ConnectionResetError):
                pass

    server = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    server.daemon_threads = True
    server.timeout = 0.1
    return server, f"http://127.0.0.1:{server.server_port}/{token}/"
