"""The native gate must bind clean source to the configured repository revision."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "fork_terra_gate", Path(__file__).resolve().parents[1] / "tools/fork_terra_gate.py")
GATE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(GATE)


class TerraSourceTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.git("init", "-q")
        self.git("config", "user.name", "Gate test")
        self.git("config", "user.email", "gate@example.invalid")
        self.file = self.root / "horizon/tools/stage.py"
        self.file.parent.mkdir(parents=True)
        self.file.write_text("# fixture\n")
        self.git("add", ".")
        self.git("-c", "core.hooksPath=", "commit", "-qm", "fixture")
        self.revision = self.git("rev-parse", "HEAD")

    def git(self, *args):
        return subprocess.check_output(["git", "-C", str(self.root), *args],
                                       text=True, stderr=subprocess.DEVNULL).strip()

    def test_clean_exact_revision(self):
        self.assertEqual(GATE.check_terra_revision(self.root, self.revision), self.revision)

    def test_wrong_revision_rejected(self):
        with self.assertRaisesRegex(ValueError, "HEAD changed"):
            GATE.check_terra_revision(self.root, "0" * 40)

    def test_modified_helper_rejected(self):
        self.file.write_text("# changed\n")
        with self.assertRaisesRegex(ValueError, "uncommitted"):
            GATE.check_terra_revision(self.root, self.revision)

    def test_untracked_bundle_file_rejected(self):
        bundle = self.root / "horizon/mixins/terraforged_prepared"
        bundle.mkdir(parents=True)
        (bundle / "extra.rs").write_text("// changed bundle\n")
        with self.assertRaisesRegex(ValueError, "uncommitted"):
            GATE.check_terra_revision(self.root, self.revision)

    def test_nested_checkout_rejected(self):
        with self.assertRaisesRegex(ValueError, "worktree root"):
            GATE.check_terra_revision(self.file.parent, self.revision)


if __name__ == "__main__":
    unittest.main()
