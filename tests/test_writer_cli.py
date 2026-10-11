"""CLI publication must wait for successful native-process shutdown."""
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from inventor_kit.writer_cli import write_job


class WriterCli(unittest.TestCase):
    def test_timeout_or_crash_cannot_publish_staged_outputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);source=root/'input.step';source.write_bytes(b'input')
            for crash in (False, True):
                def child(args, **kwargs):
                    staged=Path(args[-2]);staged.mkdir();(staged/'partial.ipt').write_bytes(b'partial')
                    if crash:
                        return subprocess.CompletedProcess(args, 1)
                    raise subprocess.TimeoutExpired(args, 1)
                with patch('inventor_kit.writer_cli.subprocess.run', side_effect=child):
                    result=write_job(source,root/'output',1)
                self.assertEqual(result['status'],'error');self.assertFalse(result['published'])
                self.assertEqual(list(root.iterdir()),[source])

    def test_destination_race_preserves_the_other_writer(self):
        with tempfile.TemporaryDirectory() as temporary:
            root=Path(temporary);destination=root/'output'
            def child(args, **kwargs):
                staged=Path(args[-2]);staged.mkdir();(staged/'part.ipt').write_bytes(b'ours')
                Path(args[-1]).write_text(json.dumps(dict(status='success',part_count=1)))
                destination.mkdir();(destination/'keep').write_bytes(b'other writer')
                return subprocess.CompletedProcess(args,0)
            with patch('inventor_kit.writer_cli.subprocess.run', side_effect=child):
                result=write_job(root/'source.step',destination,10)
            self.assertEqual(result['status'],'error');self.assertFalse(result['published'])
            self.assertEqual([p.name for p in destination.iterdir()],['keep'])
            self.assertEqual((destination/'keep').read_bytes(),b'other writer')


if __name__=='__main__':unittest.main()
