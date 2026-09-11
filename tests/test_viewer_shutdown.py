"""Terminal interrupts must close the worker/server and remove session files."""
from contextlib import redirect_stdout
import io
from pathlib import Path
import signal
import unittest
from unittest.mock import Mock, patch

from inventor_kit.viewer.cli import main, shutdown_signals


class ViewerShutdown(unittest.TestCase):
    def test_loopback_startup_does_not_require_dns(self):
        from tempfile import TemporaryDirectory
        from inventor_kit.viewer.server import create_server
        with TemporaryDirectory() as directory, patch('socket.getfqdn', side_effect=AssertionError('DNS must not be used')):
            server, url = create_server(directory)
            try:
                self.assertEqual(server.server_name, '127.0.0.1')
                self.assertGreater(server.server_port, 0)
                self.assertTrue(url.startswith(f'http://127.0.0.1:{server.server_port}/'))
            finally:
                server.server_close()

    def test_terminal_handlers_request_shutdown_and_restore_previous_handlers(self):
        signals = [signal.SIGINT]
        if hasattr(signal, 'SIGBREAK'):
            signals.append(signal.SIGBREAK)
        for signum in signals:
            previous = signal.getsignal(signum)
            with self.subTest(signal=signum):
                with shutdown_signals() as stopping:
                    self.assertFalse(stopping.is_set())
                    signal.raise_signal(signum)
                    self.assertTrue(stopping.is_set())
                self.assertEqual(signal.getsignal(signum), previous)

    def test_interrupt_during_conversion_closes_resources_and_removes_session(self):
        server, worker = Mock(), Mock()
        server.handle_request.side_effect = lambda: signal.raise_signal(signal.SIGINT)
        sessions = []

        def create(directory, port):
            sessions.append(Path(directory))
            (Path(directory) / 'mesh.bin').write_bytes(b'pending output')
            return server, 'http://127.0.0.1:1234/session/'

        previous = signal.getsignal(signal.SIGINT)
        with patch('inventor_kit.viewer.server.create_server', side_effect=create), \
                patch('inventor_kit.viewer.worker.Job', return_value=worker), redirect_stdout(io.StringIO()):
            main(['part.ipt', '--metadata-only', '--no-browser'])
        worker.poll.assert_called_once()
        worker.stop.assert_called_once()
        server.server_close.assert_called_once()
        self.assertFalse(sessions[0].exists())
        self.assertEqual(signal.getsignal(signal.SIGINT), previous)

    def test_startup_failure_restores_handlers(self):
        previous = signal.getsignal(signal.SIGINT)
        with patch('inventor_kit.viewer.server.create_server', side_effect=OSError('bind failed')), \
                patch('sys.stderr', io.StringIO()), self.assertRaises(SystemExit) as error:
            main(['part.ipt', '--metadata-only', '--no-browser'])
        self.assertEqual(error.exception.code, 1)
        self.assertEqual(signal.getsignal(signal.SIGINT), previous)
