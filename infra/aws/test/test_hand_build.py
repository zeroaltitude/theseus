"""infra/aws/hand/build.sh's own checks, run offline (theseus-i7bz).

The script is copied with its Dockerfile into a scratch git repository laid out as this one, with a stub
`scripts/build.sh` that writes a stand-in binary where the script looks for it, and stub `file` and `docker`
first on PATH. No cargo, Docker or network; the script is never run with `--push`.

    python3 -m unittest discover -s infra/aws/test -p 'test_*.py'
"""
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

HAND = Path(__file__).resolve().parent.parent / "hand"

BUILD_STUB = """#!/bin/sh
# Stands in for scripts/build.sh: writes the binary build.sh looks for.
out="$CARGO_TARGET_DIR/x86_64-unknown-linux-musl/release-thin"
mkdir -p "$out"
echo stand-in > "$out/theseusd"
"""

# `file` says what FAKE_FILE says; docker builds quietly, and `run` prints FAKE_DOCKER_OUT and exits
# with FAKE_DOCKER_STATUS, as `theseusd hand` without its spec does.
FILE_STUB = """#!/bin/sh
echo "$1: $FAKE_FILE"
"""
DOCKER_STUB = """#!/bin/sh
case "$1" in
    build) exit 0 ;;
    run)
        printf '%s\\n' "$FAKE_DOCKER_OUT"
        exit "${FAKE_DOCKER_STATUS:-0}"
        ;;
esac
echo "docker: unexpected $*" >&2
exit 99
"""

HAND_LINE = "theseusd hand: THESEUS_HAND is not set"
STATIC_PIE = "ELF 64-bit LSB pie executable, x86-64, static-pie linked, stripped"
STATIC = "ELF 64-bit LSB executable, x86-64, statically linked, stripped"
DYNAMIC = "ELF 64-bit LSB pie executable, x86-64, dynamically linked, stripped"


def write(path, text, mode=0o755):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    path.chmod(mode)


class BuildScript(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="hand-build-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.repo = self.tmp / "repo"
        hand = self.repo / "infra" / "aws" / "hand"
        hand.mkdir(parents=True)
        shutil.copy(HAND / "build.sh", hand / "build.sh")
        shutil.copy(HAND / "Dockerfile", hand / "Dockerfile")
        write(self.repo / "scripts" / "build.sh", BUILD_STUB)
        bins = self.tmp / "bin"
        write(bins / "file", FILE_STUB)
        write(bins / "docker", DOCKER_STUB)
        self.env = {
            **os.environ,
            "PATH": f"{bins}:{os.environ['PATH']}",
            "CARGO_TARGET_DIR": str(self.tmp / "target"),
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_CONFIG_SYSTEM": os.devnull,
            "GIT_AUTHOR_NAME": "Test",
            "GIT_AUTHOR_EMAIL": "test@example.invalid",
            "GIT_COMMITTER_NAME": "Test",
            "GIT_COMMITTER_EMAIL": "test@example.invalid",
        }
        for args in (["init", "-q"], ["add", "."], ["commit", "-q", "-m", "scratch"]):
            subprocess.run(["git", "-C", str(self.repo), *args], env=self.env, check=True)

    def run_build(self, file_says=STATIC_PIE, docker_says=HAND_LINE, docker_status=1):
        env = {
            **self.env,
            "FAKE_FILE": file_says,
            "FAKE_DOCKER_OUT": docker_says,
            "FAKE_DOCKER_STATUS": str(docker_status),
        }
        return subprocess.run(
            ["bash", str(self.repo / "infra" / "aws" / "hand" / "build.sh")],
            env=env,
            capture_output=True,
            text=True,
        )

    def test_a_static_pie_binary_and_a_hand_that_exits_one_pass(self):
        r = self.run_build()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("answers as a hand", r.stdout)
        self.assertRegex(r.stdout.strip().splitlines()[-1], r"^theseus/hand:[0-9a-f]{12}$")

    def test_a_statically_linked_binary_passes(self):
        r = self.run_build(file_says=STATIC)
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_a_dynamically_linked_binary_is_refused(self):
        r = self.run_build(file_says=DYNAMIC)
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not a static binary", r.stderr)

    def test_an_image_that_prints_anything_else_fails(self):
        r = self.run_build(docker_says="exec format error")
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not run theseusd hand", r.stderr)

    def test_a_hand_that_exits_zero_with_its_line_passes(self):
        r = self.run_build(docker_status=0)
        self.assertEqual(r.returncode, 0, r.stderr)


if __name__ == "__main__":
    unittest.main()
