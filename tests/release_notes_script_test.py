from __future__ import annotations

import importlib.util
from pathlib import Path
import unittest
from unittest import mock


SCRIPT_PATH = Path(__file__).resolve().parents[1] / "scripts" / "generate_release_notes.py"
SPEC = importlib.util.spec_from_file_location("generate_release_notes", SCRIPT_PATH)
assert SPEC is not None and SPEC.loader is not None
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ClassifyTagTests(unittest.TestCase):
    def test_semver_stable(self) -> None:
        self.assertEqual(MODULE.classify_tag("v0.1.0"), ("semver", "stable"))

    def test_semver_prerelease(self) -> None:
        self.assertEqual(MODULE.classify_tag("v0.1.0-rc.8"), ("semver", "pre-release"))

    def test_short_sha(self) -> None:
        self.assertEqual(MODULE.classify_tag("bfb531d"), ("short-sha", "snapshot"))

    def test_nightly_channel(self) -> None:
        self.assertEqual(MODULE.classify_tag("nightly"), ("channel", "nightly"))

    def test_rejects_latest(self) -> None:
        with self.assertRaises(ValueError):
            MODULE.classify_tag("latest")

    def test_rejects_unsupported(self) -> None:
        with self.assertRaises(ValueError):
            MODULE.classify_tag("build-20260402")


class PreviousSemverTagTests(unittest.TestCase):
    def test_skips_current_tag(self) -> None:
        with mock.patch.object(
            MODULE,
            "run_git",
            return_value="v0.1.0-rc.8\nv0.1.0-rc.7\nv0.1.0-beta.4\n",
        ):
            self.assertEqual(MODULE.get_previous_tag("v0.1.0-rc.8"), "v0.1.0-rc.7")

    def test_returns_latest_semver_for_channel_tag(self) -> None:
        with mock.patch.object(
            MODULE,
            "run_git",
            return_value="v0.1.0-rc.8\nv0.1.0-rc.7\n",
        ):
            self.assertEqual(MODULE.get_previous_tag("nightly"), "v0.1.0-rc.8")


class RenderNotesTests(unittest.TestCase):
    def test_non_semver_tag_shows_channel_metadata(self) -> None:
        with mock.patch.object(MODULE, "get_release_date", return_value="2026-04-02"):
            notes = MODULE.render_release_notes(
                "nightly",
                "aistar-au/vexapi",
                "v0.1.0-rc.8",
                [("feat: auto-tag on merge", "bfb531d")],
            )

        self.assertIn("- Release channel: nightly", notes)
        self.assertIn("- Tag kind: channel", notes)
        self.assertIn("- Previous semver tag: v0.1.0-rc.8", notes)
        self.assertIn("- auto-tag on merge (bfb531d)", notes)

    def test_short_sha_tag_shows_snapshot_metadata(self) -> None:
        with mock.patch.object(MODULE, "get_release_date", return_value="2026-04-02"):
            notes = MODULE.render_release_notes(
                "bfb531d",
                "aistar-au/vexapi",
                "v0.1.0-rc.8",
                [("ci: update workflow", "abc1234")],
            )

        self.assertIn("- Release channel: snapshot", notes)
        self.assertIn("- Tag kind: short-sha", notes)


if __name__ == "__main__":
    unittest.main()
