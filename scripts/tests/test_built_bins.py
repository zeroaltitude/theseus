"""A measuring test must not start binaries from another checkout."""
import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from scripts.tests import test_view_kit, test_view_proof


class CurrentBinaries(unittest.TestCase):
    def setUp(self):
        env = patch.dict(os.environ, {"CARGO_TARGET_DIR": ""})
        env.start()
        self.addCleanup(env.stop)

    def test_external_target_directory_and_skip_path(self):
        with tempfile.TemporaryDirectory() as tmp:
            target = Path(tmp)
            debug = target / "debug"
            debug.mkdir()
            (debug / "theseusd").write_bytes(b"theseus-build-stamp:" + b"a" * 40 + b":")
            (debug / "theseus").write_bytes(b"cli")
            for module in (test_view_kit, test_view_proof):
                with patch.dict(os.environ, {"CARGO_TARGET_DIR": str(target)}):
                    with patch("subprocess.check_output", side_effect=["a" * 40, "", "a" * 40]):
                        try:
                            selected = module.built_bins()
                        except unittest.SkipTest as error:
                            self.fail(f"matching external binaries were skipped: {error}")
                        self.assertEqual(selected, debug)
                    with patch("subprocess.check_output", side_effect=["b" * 40, ""]):
                        with self.assertRaises(unittest.SkipTest) as skipped:
                            module.built_bins()
                        self.assertIn(str(debug), str(skipped.exception))

    def test_old_binaries_are_not_selected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            debug = root / "target" / "debug"
            debug.mkdir(parents=True)
            for name in ("theseus", "theseusd"):
                (debug / name).write_bytes(b"not a build of this tree")
            for module in (test_view_kit, test_view_proof):
                with self.subTest(module=module.__name__), patch.object(module, "ROOT", root), patch("subprocess.check_output", side_effect=["a" * 40, ""]):
                    with self.assertRaisesRegex(unittest.SkipTest, "cannot verify"):
                        module.built_bins()

        self._matching_stamps_are_selected()

    def _matching_stamps_are_selected(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            debug = root / "target" / "debug"
            debug.mkdir(parents=True)
            (debug / "theseusd").write_bytes(b"theseus-build-stamp:" + b"a" * 40 + b":")
            (debug / "theseus").write_bytes(b"cli")
            for module in (test_view_kit, test_view_proof):
                with self.subTest(module=module.__name__), patch.object(module, "ROOT", root), patch("subprocess.check_output", side_effect=["a" * 40, "", "a" * 40]):
                    self.assertEqual(module.built_bins(), debug)

    def test_an_edited_tree_is_not_a_verified_build(self):
        for module in (test_view_kit, test_view_proof):
            with self.subTest(module=module.__name__), patch("subprocess.check_output", side_effect=["a" * 40, " M source.rs"]):
                with self.assertRaisesRegex(unittest.SkipTest, "edited tree"):
                    module.built_bins()

    def test_a_stale_cli_is_not_selected_beside_a_current_daemon(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            debug = root / "target" / "debug"
            debug.mkdir(parents=True)
            (debug / "theseusd").write_bytes(b"theseus-build-stamp:" + b"a" * 40 + b":")
            (debug / "theseus").write_bytes(b"cli")
            for module in (test_view_kit, test_view_proof):
                with self.subTest(module=module.__name__), patch.object(module, "ROOT", root), patch("subprocess.check_output", side_effect=["a" * 40, "", "b" * 40]):
                    with self.assertRaisesRegex(unittest.SkipTest, "current CLI"):
                        module.built_bins()
