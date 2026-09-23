"""Entry point for the local saved-document viewer."""
import argparse
from contextlib import contextmanager
import signal
import tempfile
from types import SimpleNamespace
import webbrowser

from .scene import Options


@contextmanager
def shutdown_signals():
    """Finish cleanup before restoring terminal interrupt handlers."""
    stopping = SimpleNamespace(requested=False)

    def request_stop(*_):
        # Signal handlers must not acquire synchronization locks: another
        # interrupt can arrive while the handler is still running.
        stopping.requested = True

    signals = [signal.SIGINT]
    if hasattr(signal, "SIGBREAK"):
        signals.append(signal.SIGBREAK)
    previous = {}
    try:
        for signum in signals:
            previous[signum] = signal.signal(signum, request_stop)
        yield stopping
    finally:
        for signum, handler in previous.items():
            signal.signal(signum, handler)


def main(argv=None):
    parser = argparse.ArgumentParser(description="View saved Inventor parts, assemblies and IDW drawings locally",
        epilog="Supported IDW drawings open in saved source coordinates by default. Drawing content, physical units and current state remain unverified.")
    parser.add_argument("path")
    parser.add_argument("--no-browser", action="store_true")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument("--quality", choices=("draft", "normal", "fine"), default="normal")
    parser.add_argument("--metadata-only", action="store_true", help="Show document information and saved previews without decoding geometry")
    # Accepted for existing scripts; IDW saved display is now the default.
    parser.add_argument("--experimental-drawing", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--candidate-id")
    parser.add_argument("--require-current-state", action="store_true")
    parser.add_argument("--search-root", action="append", default=[], help="IAM reference search directory (repeatable)")
    parser.add_argument("--allow-unverified-state", action="store_true", help="Allow saved IAM placements with unverified current state")
    parser.add_argument("--allow-partial", action="store_true", help="Allow partial IPT/IAM geometry; IDW already shows partial saved content")
    parser.add_argument("--body-id", action="append", default=[], help="Select an IPT body ID (repeatable)")
    parser.add_argument("--timeout", type=float, default=120, help="Conversion time limit in seconds (default: 120)")
    args = parser.parse_args(argv)
    if not 0 <= args.port <= 65535:
        parser.error("port must be between 0 and 65535")
    try:
        options = Options(quality=args.quality, metadata_only=args.metadata_only,
                          candidate_id=args.candidate_id, require_current_state=args.require_current_state,
                          search_roots=tuple(args.search_root), allow_unverified_state=args.allow_unverified_state,
                          allow_partial=args.allow_partial, body_ids=tuple(args.body_id),
                          timeout=args.timeout, experimental_drawing=args.experimental_drawing)
    except ValueError as error:
        parser.error(str(error))
    # The worker identifies the immutable input snapshot before dispatching.
    # File extensions cannot select dependency requirements or document options.
    from .server import create_server
    from .worker import Job
    with shutdown_signals() as stopping, tempfile.TemporaryDirectory(prefix="inventor-viewer-") as temporary:
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
            while not stopping.requested:
                job.poll()
                server.handle_request()
        except KeyboardInterrupt:
            pass
        finally:
            if job is not None:
                job.stop()
            server.server_close()
