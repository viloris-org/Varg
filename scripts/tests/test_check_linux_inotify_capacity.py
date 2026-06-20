from __future__ import annotations

import errno
import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).parents[1] / "check-linux-inotify-capacity.py"
SPEC = importlib.util.spec_from_file_location("check_linux_inotify_capacity", SCRIPT)
assert SPEC and SPEC.loader
CHECK = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(CHECK)


class InotifyCapacityTests(unittest.TestCase):
    def test_reports_inotify_exhaustion_with_usage(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            proc_root = Path(directory)
            (proc_root / "sys/fs/inotify").mkdir(parents=True)
            (proc_root / "sys/fs/inotify/max_user_instances").write_text(
                "128\n", encoding="utf-8"
            )

            with (
                mock.patch.object(CHECK.resource, "getrlimit", return_value=(4096, 4096)),
                mock.patch.object(CHECK, "count_process_fds", return_value=12),
                mock.patch.object(CHECK, "inotify_usage", return_value=(128, [])),
            ):
                message = CHECK.format_inotify_failure(proc_root)

        self.assertIn("Linux file-watch instance limit reached", message)
        self.assertIn("need 4 spare inotify instances", message)
        self.assertIn("not a general app/process/thread limit", message)
        self.assertIn("128/128", message)
        self.assertIn("fs.inotify.max_user_instances", message)
        self.assertIn("For local development", message)

    def test_reports_process_open_file_limit(self) -> None:
        with (
            mock.patch.object(CHECK.resource, "getrlimit", return_value=(256, 256)),
            mock.patch.object(CHECK, "count_process_fds", return_value=252),
        ):
            message = CHECK.format_startup_resource_failure()

        self.assertIsNotNone(message)
        assert message is not None
        self.assertIn("open-file limit is too low", message)
        self.assertIn("(252/256)", message)
        self.assertIn("ulimit -n 4096", message)

    def test_reports_low_shell_open_file_limit_before_tauri_starts(self) -> None:
        with (
            mock.patch.object(CHECK.resource, "getrlimit", return_value=(1024, 1024)),
            mock.patch.object(CHECK, "count_process_fds", return_value=12),
        ):
            message = CHECK.format_startup_resource_failure()

        self.assertIsNotNone(message)
        assert message is not None
        self.assertIn("open-file limit is too low", message)
        self.assertIn("(12/1024)", message)
        self.assertIn("not a general app or thread limit", message)

    def test_accepts_reasonable_shell_open_file_limit(self) -> None:
        with (
            mock.patch.object(CHECK.resource, "getrlimit", return_value=(4096, 4096)),
            mock.patch.object(CHECK, "count_process_fds", return_value=12),
        ):
            message = CHECK.format_startup_resource_failure()

        self.assertIsNone(message)

    def test_probe_preserves_emfile(self) -> None:
        with mock.patch.object(
            CHECK.ctypes,
            "CDLL",
            return_value=mock.Mock(inotify_init1=mock.Mock(return_value=-1)),
        ), mock.patch.object(CHECK.ctypes, "get_errno", return_value=errno.EMFILE):
            with self.assertRaises(OSError) as raised:
                CHECK.open_inotify_instance()

        self.assertEqual(raised.exception.errno, errno.EMFILE)


if __name__ == "__main__":
    unittest.main()
