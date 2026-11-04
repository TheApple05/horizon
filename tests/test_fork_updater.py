"""Real local Git repositories exercise durable discovery and failure gates."""
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location("fork_updater", Path(__file__).resolve().parents[1] / "tools/fork_updater.py")
MODULE = importlib.util.module_from_spec(SPEC)
sys.path.insert(0, str(Path(SPEC.origin).parent))
try:
    SPEC.loader.exec_module(MODULE)
finally:
    sys.path.pop(0)


class UpdaterTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.upstream = self.root / "upstream"
        self.upstream.mkdir()
        self.git(self.upstream, "init", "-b", "main")
        self.git(self.upstream, "config", "user.email", "tests@example.invalid")
        self.git(self.upstream, "config", "user.name", "Tests")
        self.baseline = self.commit("initial")
        self.repo = self.root / "fork"
        self.git(self.root, "clone", str(self.upstream), str(self.repo))
        self.git(self.repo, "remote", "add", "upstream", str(self.upstream))
        self.config_path = self.root / "trusted.json"
        self.config = dict(repo=str(self.repo), state_dir=str(self.root / "state"), min_free_gb=0, gates=[
            dict(name=kind, category=kind, argv=["{python}", "-c", "print('gate passed')"],
                 success_regex="gate passed", timeout=10)
            for kind in sorted(MODULE.REQUIRED)])
        self.save_config()
        self.updater = MODULE.Updater(self.config_path)
        self.addCleanup(self.updater.db.close)
        self.updater.init(self.baseline)

    @staticmethod
    def git(repo, *args):
        return subprocess.run(["git", "-C", str(repo), *args], check=True,
                              capture_output=True, text=True).stdout.strip()

    def commit(self, value):
        (self.upstream / "value.txt").write_text(value)
        self.git(self.upstream, "add", "value.txt")
        self.git(self.upstream, "commit", "-m", value)
        return self.git(self.upstream, "rev-parse", "HEAD")

    def save_config(self):
        self.config_path.write_text(json.dumps(self.config))

    def reload(self):
        self.updater.db.close()
        self.updater = MODULE.Updater(self.config_path)
        self.addCleanup(self.updater.db.close)

    def test_burst_and_duplicate_poll_preserve_every_commit(self):
        expected = [self.commit(str(n)) for n in range(3)]
        self.assertEqual(self.updater.poll()["discovered"], 3)
        self.assertEqual(self.updater.poll()["discovered"], 0)
        actual = [r[0] for r in self.updater.db.execute("SELECT upstream FROM queue ORDER BY id")]
        self.assertEqual(actual, expected)
        self.assertEqual(self.updater.meta("cursor"), expected[-1])

    def test_non_first_parent_commit_is_not_silently_skipped(self):
        self.git(self.upstream, "checkout", "-b", "feature")
        (self.upstream / "feature.txt").write_text("feature")
        self.git(self.upstream, "add", "feature.txt")
        self.git(self.upstream, "commit", "-m", "feature")
        feature = self.git(self.upstream, "rev-parse", "HEAD")
        self.git(self.upstream, "checkout", "main")
        direct = self.commit("direct")
        self.git(self.upstream, "merge", "--no-ff", "feature", "-m", "merge feature")
        merged = self.git(self.upstream, "rev-parse", "HEAD")
        self.updater.poll()
        actual = [r[0] for r in self.updater.db.execute("SELECT upstream FROM queue ORDER BY id")]
        self.assertEqual(set(actual), {feature, direct, merged})
        self.assertEqual(actual[-1], merged)

    def test_rewrite_refused_without_advancing_watermark(self):
        previous = self.commit("previous")
        self.updater.poll()
        self.git(self.upstream, "reset", "--hard", self.baseline)
        self.commit("rewritten")
        with self.assertRaises(RuntimeError):
            self.updater.poll()
        self.assertEqual(self.updater.meta("cursor"), previous)
        self.assertEqual(self.updater.status()["counts"], {"queued": 1})

    def test_failed_gate_cannot_validate_and_next_commit_still_runs(self):
        self.commit("first")
        second = self.commit("second")
        self.updater.poll()
        self.config["gates"][0]["argv"] = ["{python}", "-c", "raise SystemExit(4)"]
        self.save_config()
        self.reload()
        failed = self.updater.run()
        self.assertEqual(failed["status"], "failed")
        self.assertFalse(failed["merge_ready"])
        self.config["gates"][0]["argv"] = ["{python}", "-c", "print('gate passed')"]
        self.save_config()
        self.reload()
        validated = self.updater.run()
        self.assertEqual(validated["upstream"], second)
        self.assertEqual(validated["status"], "validated")
        self.assertTrue(validated["merge_ready"])
        self.assertFalse((Path(validated["run_dir"]) / "source").exists())
        self.assertEqual(self.git(self.repo, "rev-parse", f"refs/fork-updater/validated/{second}"),
                         validated["candidate"])

    def test_recovery_uses_fresh_attempt_after_process_death(self):
        sha = self.commit("retry")
        self.updater.poll()
        with self.updater.db:
            self.updater.db.execute("UPDATE queue SET status='running', run_dir='old-attempt'")
        self.reload()
        result = self.updater.run()
        self.assertEqual(result["status"], "validated")
        self.assertEqual(result["upstream"], sha)
        self.assertNotEqual(result["run_dir"], "old-attempt")

    def test_zero_or_missing_gate_categories_refused_without_claiming(self):
        self.commit("gate-required")
        self.updater.poll()
        self.config["gates"] = []
        self.save_config()
        self.reload()
        with self.assertRaises(ValueError):
            self.updater.run()
        self.assertEqual(self.updater.status()["counts"], {"queued": 1})

    def test_expected_output_absent_is_failure_even_with_zero_exit(self):
        self.commit("no-tests")
        self.updater.poll()
        self.config["gates"][0]["argv"] = ["{python}", "-c", "print('0 tests')"]
        self.save_config()
        self.reload()
        self.assertEqual(self.updater.run()["status"], "failed")

    def test_overlapping_worker_is_refused(self):
        with MODULE.lock(self.updater.state / "worker.lock"):
            with self.assertRaises(RuntimeError):
                self.updater.run()

    def test_queue_and_cursor_commit_atomically(self):
        sha = self.commit("atomic")
        self.updater.db.execute("CREATE TRIGGER interrupt_poll BEFORE UPDATE ON meta "
                                "BEGIN SELECT RAISE(ABORT, 'power loss'); END")
        self.updater.db.commit()
        with self.assertRaises(Exception):
            self.updater.poll()
        self.assertEqual(self.updater.meta("cursor"), self.baseline)
        self.assertEqual(self.updater.status()["counts"], {})
        self.updater.db.execute("DROP TRIGGER interrupt_poll")
        self.updater.db.commit()
        self.updater.poll()
        self.assertEqual(self.updater.status()["recent"][0]["upstream"], sha)

    def test_timeout_is_terminal_failure(self):
        self.commit("timeout")
        self.updater.poll()
        self.config["gates"][0].update(argv=["{python}", "-c", "import time; time.sleep(30)"], timeout=0.1)
        self.save_config()
        self.reload()
        result = self.updater.run()
        self.assertEqual(result["status"], "failed")
        self.assertIn("timed out", result["error"])

    def test_retry_requires_failed_item_and_preserves_attempt(self):
        sha = self.commit("retry-failure")
        self.updater.poll()
        self.config["gates"][0]["argv"] = ["{python}", "-c", "raise SystemExit(1)"]
        self.save_config()
        self.reload()
        failed = self.updater.run()
        self.updater.retry(sha)
        self.assertTrue(Path(failed["run_dir"]).exists())
        self.assertEqual(self.updater.status()["counts"], {"queued": 1})
        with self.assertRaises(ValueError):
            self.updater.retry(sha)

    def test_low_disk_preserves_unclaimed_queue(self):
        self.commit("low-disk")
        self.updater.poll()
        self.config["min_free_gb"] = 1000000
        self.save_config()
        self.reload()
        self.assertEqual(self.updater.run()["status"], "backpressure")
        self.assertEqual(self.updater.status()["counts"], {"queued": 1})

    def test_validated_item_can_retry_against_advanced_fork_base(self):
        sha = self.commit("upstream-change")
        self.updater.poll()
        original = self.updater.run()
        self.assertEqual(original["status"], "validated")
        self.git(self.repo, "config", "user.email", "tests@example.invalid")
        self.git(self.repo, "config", "user.name", "Tests")
        (self.repo / "fork.txt").write_text("fork advanced")
        self.git(self.repo, "add", "fork.txt")
        self.git(self.repo, "commit", "-m", "advance fork")
        advanced = self.git(self.repo, "rev-parse", "HEAD")
        self.git(self.repo, "remote", "set-url", "origin", str(self.repo))
        self.updater.poll()
        self.updater.retry(sha)
        self.assertEqual(self.updater.status()["counts"], {"queued": 1})
        self.assertTrue(Path(original["run_dir"]).is_dir())
        self.assertEqual(self.git(self.repo, "rev-parse", f"refs/fork-updater/validated/{sha}"), original["candidate"])
        current = self.updater.run()
        self.assertEqual(current["status"], "validated")
        self.assertNotEqual(current["run_dir"], original["run_dir"])
        self.assertNotEqual(current["candidate"], original["candidate"])
        self.assertEqual(self.updater.status()["recent"][0]["base"], advanced)
        self.assertEqual(self.git(self.repo, "rev-parse", f"refs/fork-updater/attempts/{sha}/{original['candidate']}"),
                         original["candidate"])

    def test_service_once_polls_and_validates_one_commit(self):
        sha = self.commit("service-once")
        command = [sys.executable, str(Path(MODULE.__file__).with_name("fork_updater_service.py")),
                   "--config", str(self.config_path), "--once"]
        result = subprocess.run(command, capture_output=True, text=True, timeout=30)
        self.assertEqual(result.returncode, 0, result.stderr)
        status = json.loads((self.updater.state / "service-status.json").read_text())
        self.assertEqual(status["result_status"], "validated")
        self.assertEqual(status["upstream"], sha)
        self.assertEqual(self.updater.status()["counts"], {"validated": 1})


if __name__ == "__main__":
    unittest.main()
