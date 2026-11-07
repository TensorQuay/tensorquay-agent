"""Offline boundaries for the desktop suite's driver side (WEB-E2E-071..077): its Desktop pin, its scenario and its
verdict on the stored pictures."""

import json
from pathlib import Path
import shutil
import tempfile
import time
import unittest
import uuid
from unittest.mock import patch

import compose
import demo_desktop

PNG = b"\x89PNG\r\n\x1a\n" + b"\x00" * 16


class ConfigureTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.directory = Path(self.root.name)
        patcher = patch.object(compose, "ownership")
        patcher.start()
        self.addCleanup(patcher.stop)
        self.addCleanup(self.root.cleanup)

    def test_a_run_naming_no_desktop_image_is_refused(self):
        (self.directory / "gateway.toml").write_text("[runtime.builtin_images]\nminimal = \"sha256:a\"\n")
        with self.assertRaisesRegex(ValueError, "TQA_DESKTOP_TEST_IMAGE"):
            demo_desktop.configure(self.directory, {})

    def test_a_gateway_file_without_the_desktop_pin_is_refused(self):
        (self.directory / "gateway.toml").write_text("[runtime.builtin_images]\nminimal = \"sha256:a\"\n")
        with self.assertRaisesRegex(ValueError, "pins no Desktop"):
            demo_desktop.configure(self.directory, {"TQA_DESKTOP_TEST_IMAGE": "tqa-runtime:desktop-test"})

    def test_the_desktop_pin_is_accepted(self):
        (self.directory / "gateway.toml").write_text(
            "[runtime.builtin_images]\ndesktop = \"sha256:d\"\nminimal = \"sha256:a\"\n")
        demo_desktop.configure(self.directory, {"TQA_DESKTOP_TEST_IMAGE": "tqa-runtime:desktop-test"})


class ScenarioTests(unittest.TestCase):
    def test_each_case_has_its_phrase_and_every_computer_call_names_every_field(self):
        scenario = json.loads((demo_desktop.FIXTURES / "desktop.json").read_text())
        self.assertEqual(scenario["select"], "prompt")
        phrases = {reply["when"] for reply in scenario["responses"]}
        self.assertEqual({phrase.split(" ")[0] for phrase in phrases},
                         {"DESK-A", "DESK-B", "DESK-C", "DESK-D", "DESK-E", "DESK-G", "DESK-H"})
        fields = {"action", "ref", "text", "keys", "ms", "direction", "amount"}
        for reply in scenario["responses"]:
            for event in reply["events"]:
                if event.get("name") == "computer":
                    self.assertEqual(set(event["arguments"]), fields)
                    # A script can't know a snapshot's refs: no clicks, keys and typing only.
                    self.assertIn(event["arguments"]["action"], {"key", "type", "wait", "observe"})


def fake_copy(storage):
    """copy_pictures without a container: the storage root is readable here."""
    def copy(env, identity, destination):
        source = storage / "screens" / identity
        if source.exists():
            shutil.copytree(source, destination / identity, symlinks=True)
        else:
            (destination / identity).mkdir()
    return copy


class PicturesTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.storage = Path(self.root.name) / "storage"
        self.task = str(uuid.uuid4())
        self.folder = self.storage / "screens" / self.task

    def test_png_pictures_pass_and_are_counted(self):
        self.folder.mkdir(parents=True)
        (self.folder / "one.png").write_bytes(PNG)
        (self.folder / "two.png").write_bytes(PNG)
        self.assertEqual(2, demo_desktop.check_pictures(self.storage / "screens", self.task))

    def test_no_pictures_fails(self):
        self.folder.mkdir(parents=True)
        with self.assertRaisesRegex(AssertionError, "kept no pictures"):
            demo_desktop.check_pictures(self.storage / "screens", self.task)

    def test_a_file_that_isnt_a_png_fails(self):
        self.folder.mkdir(parents=True)
        (self.folder / "one.png").write_bytes(b"not a picture")
        with self.assertRaisesRegex(AssertionError, "isn't a PNG"):
            demo_desktop.check_pictures(self.storage / "screens", self.task)

    def test_a_link_out_of_the_folder_fails(self):
        self.folder.mkdir(parents=True)
        outside = Path(self.root.name) / "outside.png"
        outside.write_bytes(PNG)
        (self.folder / "one.png").symlink_to(outside)
        with self.assertRaisesRegex(AssertionError, "escaped its task's folder"):
            demo_desktop.check_pictures(self.storage / "screens", self.task)

    def test_the_copy_reads_the_storage_root_read_only_and_hands_the_copy_back(self):
        env = {"TQA_STORAGE_ROOT": str(self.storage)}
        with patch.object(compose, "host_owner", return_value="0:0"), patch.object(compose, "command") as command:
            demo_desktop.copy_pictures(env, self.task, Path(self.root.name))
        args = command.call_args.args[0]
        self.assertIn("type=bind,source={},target=/storage,readonly".format(self.storage.resolve()), args)
        self.assertIn("type=bind,source={},target=/out".format(Path(self.root.name)), args)
        self.assertEqual(["--network", "none"], args[3:5])
        self.assertEqual([self.task, "0:0"], args[-2:])
        self.assertIn('cp -a "/storage/screens/$1"', args[-4])


class WatchTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.storage = Path(self.root.name) / "storage"
        self.markers = Path(self.root.name) / "markers"
        self.task = str(uuid.uuid4())
        self.env = {"TQA_STORAGE_ROOT": str(self.storage)}

    def ask(self, watch, wanted):
        """As the spec: the old answers removed, the request written, then waits for an answer."""
        for name in ("pictures-checked", "pictures-failed"):
            (self.markers / name).unlink(missing_ok=True)
        (self.markers / "pictures-wanted").write_text(wanted)
        deadline = time.monotonic() + 10
        while not any((self.markers / name).exists() for name in ("pictures-checked", "pictures-failed")):
            self.assertLess(time.monotonic(), deadline, "no answer")
            time.sleep(0.05)

    def watch(self, wanted):
        watch = demo_desktop.PictureWatch(self.markers, self.env, copy=fake_copy(self.storage)).start()
        self.addCleanup(watch.finish)
        self.ask(watch, wanted)
        return watch

    def test_the_wanted_task_is_checked_and_its_number_written(self):
        folder = self.storage / "screens" / self.task
        folder.mkdir(parents=True)
        (folder / "one.png").write_bytes(PNG)
        watch = self.watch(self.task + "\n")
        self.assertEqual(("checked", self.task, 1), watch.finish())
        self.assertEqual("1", (self.markers / "pictures-checked").read_text())
        self.assertFalse((self.markers / "pictures-failed").exists())

    def test_a_task_without_pictures_is_failed_with_why(self):
        watch = self.watch(self.task)
        self.assertEqual("failed", watch.finish()[0])
        self.assertIn("kept no pictures", (self.markers / "pictures-failed").read_text())
        self.assertFalse((self.markers / "pictures-checked").exists())

    def test_an_id_that_isnt_one_is_failed(self):
        watch = self.watch("../elsewhere")
        self.assertEqual(("failed", None), watch.finish()[:2])
        self.assertTrue((self.markers / "pictures-failed").exists())

    def test_a_retried_test_asks_again_and_the_last_answer_counts(self):
        watch = self.watch(self.task)
        self.assertEqual("failed", watch.outcome[0])
        self.assertFalse((self.markers / "pictures-wanted").exists())
        folder = self.storage / "screens" / self.task
        folder.mkdir(parents=True)
        (folder / "one.png").write_bytes(PNG)
        self.ask(watch, self.task)
        self.assertEqual(("checked", self.task, 1), watch.finish())

    def test_never_asked_ends_with_no_outcome(self):
        watch = demo_desktop.PictureWatch(self.markers, self.env, copy=fake_copy(self.storage)).start()
        self.assertIsNone(watch.finish())


class ResultTests(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.TemporaryDirectory()
        self.addCleanup(self.root.cleanup)
        self.task = str(uuid.uuid4())
        self.result = Path(self.root.name) / "browser.json"
        self.result.write_text(json.dumps({"lookedAt": self.task}))
        self.env = {"TQA_STACK_RESULT": str(self.result)}

    def pictures(self, outcome):
        class Watch:
            finished = False

            def finish(self):
                self.finished = True
                return outcome
        return Watch()

    def test_the_looked_at_tasks_checked_pictures_pass(self):
        watch = self.pictures(("checked", self.task, 2))
        demo_desktop.verify_result(self.env, None, watch)
        self.assertTrue(watch.finished)

    def test_pictures_never_checked_fail(self):
        with self.assertRaisesRegex(AssertionError, "never checked"):
            demo_desktop.verify_result(self.env, None, self.pictures(None))

    def test_a_failed_check_fails_naming_why(self):
        with self.assertRaisesRegex(AssertionError, "kept no pictures"):
            demo_desktop.verify_result(self.env, None, self.pictures(("failed", self.task, "kept no pictures")))

    def test_another_tasks_pictures_fail(self):
        with self.assertRaisesRegex(AssertionError, "weren't the looked-at task's"):
            demo_desktop.verify_result(self.env, None, self.pictures(("checked", str(uuid.uuid4()), 1)))

    def test_the_restarter_stops_whatever_the_verdict(self):
        class Restarter:
            finished = False

            def finish(self):
                self.finished = True

        restarter = Restarter()
        with self.assertRaises(AssertionError):
            demo_desktop.verify_result(self.env, restarter, self.pictures(None))
        self.assertTrue(restarter.finished)


if __name__ == "__main__":
    unittest.main()
