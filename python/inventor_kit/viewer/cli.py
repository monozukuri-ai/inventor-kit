"""Entry point for the local saved-part viewer."""
import argparse
from importlib.util import find_spec
import tempfile
import webbrowser

from .scene import Options


def main(argv=None):
    parser = argparse.ArgumentParser(description="View saved Inventor part geometry and document information locally")
    parser.add_argument("path")
    parser.add_argument("--no-browser", action="store_true")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--quality", choices=("draft", "normal", "fine"), default="normal")
    parser.add_argument("--metadata-only", action="store_true")
    parser.add_argument("--candidate-id")
    parser.add_argument("--require-current-state", action="store_true")
    parser.add_argument("--timeout", type=float, default=120, help="Conversion time limit in seconds (default: 120)")
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535:
        parser.error("port must be between 0 and 65535")
    try:
        options = Options(quality=args.quality, metadata_only=args.metadata_only,
                          candidate_id=args.candidate_id, require_current_state=args.require_current_state,
                          timeout=args.timeout)
    except ValueError as error:
        parser.error(str(error))
    if not args.metadata_only and find_spec("ocp_tessellate") is None:
        parser.error("Install viewer dependencies: python -m pip install 'inventor-kit[viewer]'")
    from .server import create_server
    from .worker import Job
    with tempfile.TemporaryDirectory(prefix="inventor-viewer-") as temporary:
        try:
            server, url = create_server(temporary, args.port)
        except (OSError, FileNotFoundError) as error:
            parser.exit(1, f"Viewer could not start: {error}\n")
        job = None
        try:
            job = Job(args.path, temporary, options)
            print(url, flush=True)
            if not args.no_browser:
                try:
                    webbrowser.open(url)
                except webbrowser.Error:
                    pass  # The printed URL remains usable.
            while True:
                job.poll()
                server.handle_request()
        except KeyboardInterrupt:
            pass
        finally:
            if job is not None:
                job.stop()
            server.server_close()
