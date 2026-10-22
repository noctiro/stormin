"""Check terminal restoration without starting any request workers.

Build with `cargo build --locked`, then run:
    python3 tests/terminal_cleanup.py target/debug/stormin

These checks use Unix pseudo-terminals and only pass missing or invalid configs.
"""

import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import struct
import subprocess
import sys
import tempfile
import termios
import time
import unittest


BINARY = (
    Path(sys.argv.pop(1)).resolve()
    if len(sys.argv) > 1
    else Path("target/debug/stormin").resolve()
)


class TerminalCleanupTests(unittest.TestCase):
    def run_in_terminal(self, config, *, answer_cursor_query=True):
        master, slave = pty.openpty()
        process = None
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 80, 0, 0))
            original = termios.tcgetattr(slave)
            process = subprocess.Popen(
                [str(BINARY), f"--config={config}"],
                stdin=slave,
                stdout=slave,
                stderr=slave,
                start_new_session=True,
            )
            output = bytearray()
            answered_queries = 0
            deadline = time.monotonic() + 10
            while True:
                if time.monotonic() > deadline:
                    self.fail("Application did not exit after configuration failure")
                readable, _, _ = select.select([master], [], [], 0.1)
                if readable:
                    try:
                        chunk = os.read(master, 65536)
                    except OSError as error:
                        if error.errno != errno.EIO:
                            raise
                        break
                    if not chunk:
                        break
                    output.extend(chunk)
                    if answer_cursor_query:
                        queries = output.count(b"\x1b[6n")
                        while answered_queries < queries:
                            os.write(master, b"\x1b[1;1R")
                            answered_queries += 1
                elif process.poll() is not None:
                    break
            returncode = process.wait(timeout=1)
            restored = termios.tcgetattr(slave)
            self.assertEqual(restored, original, "Terminal input settings were not restored")
            return returncode, bytes(output)
        finally:
            if process is not None and process.poll() is None:
                process.kill()
                process.wait()
            os.close(master)
            os.close(slave)

    def assert_cleanup_precedes_error(self, output):
        self.assertIn(b"\x1b[?1049h", output, "TUI did not enter the alternate screen")
        message_at = output.find(b"Failed to initialize application:")
        self.assertGreaterEqual(message_at, 0)
        for command in (
            b"\x1b[?1000l",  # Disable mouse input.
            b"\x1b[?1006l",  # Disable SGR mouse encoding.
            b"\x1b[?25h",  # Show the cursor.
            b"\x1b[?1049l",  # Leave the alternate screen.
        ):
            self.assertIn(command, output[:message_at], "Cleanup must precede the error message")

    def assert_tui_failure_is_restored(self, config):
        returncode, output = self.run_in_terminal(config)
        self.assertNotEqual(returncode, 0)
        self.assertIn(b"\x1b[2J", output, "Terminal setup must finish before loading config")
        self.assert_cleanup_precedes_error(output)

    def test_missing_config_restores_terminal_before_error(self):
        with tempfile.TemporaryDirectory(prefix="stormin-terminal-test-") as directory:
            self.assert_tui_failure_is_restored(Path(directory) / "missing.toml")

    def test_invalid_config_restores_terminal_before_error(self):
        with tempfile.TemporaryDirectory(prefix="stormin-terminal-test-") as directory:
            config = Path(directory) / "invalid.toml"
            config.write_text("invalid = [", encoding="utf-8")
            self.assert_tui_failure_is_restored(config)

    def test_partial_terminal_setup_failure_restores_terminal(self):
        with tempfile.TemporaryDirectory(prefix="stormin-terminal-test-") as directory:
            returncode, output = self.run_in_terminal(
                Path(directory) / "missing.toml", answer_cursor_query=False
            )
        self.assertNotEqual(returncode, 0)
        self.assertNotIn(b"\x1b[2J", output, "Terminal setup should fail before clearing")
        self.assert_cleanup_precedes_error(output)

    def test_cli_failure_does_not_emit_terminal_commands(self):
        with tempfile.TemporaryDirectory(prefix="stormin-terminal-test-") as directory:
            result = subprocess.run(
                [str(BINARY), "--cli", f"--config={Path(directory) / 'missing.toml'}"],
                input=b"",
                capture_output=True,
                timeout=10,
                check=False,
                start_new_session=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"Failed to initialize application:", result.stderr)
        self.assertNotIn(b"\x1b[", result.stdout + result.stderr)

    def test_tui_failure_without_a_terminal_does_not_emit_terminal_commands(self):
        with tempfile.TemporaryDirectory(prefix="stormin-terminal-test-") as directory:
            result = subprocess.run(
                [str(BINARY), f"--config={Path(directory) / 'missing.toml'}"],
                input=b"",
                capture_output=True,
                timeout=10,
                check=False,
                start_new_session=True,
            )
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(b"Failed to initialize application:", result.stderr)
        self.assertNotIn(b"\x1b[", result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
