#!/usr/bin/env python3
"""Unit tests for reduction mechanics; Maven parity is tested by Cargo."""
import importlib.util
import json
import pathlib
import tempfile
import unittest
from unittest.mock import patch

SPEC = importlib.util.spec_from_file_location("differential", pathlib.Path(__file__).with_name("differential.py"))
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class MinimizerTests(unittest.TestCase):
    def test_preserves_counterexample_without_editing_original(self):
        with tempfile.TemporaryDirectory() as temp:
            root = pathlib.Path(temp)
            repository = root / "original"
            repository.mkdir()
            pom = repository / "a.pom"
            original = "<project><dependencies><dependency>needed</dependency><dependency>irrelevant</dependency></dependencies></project>"
            pom.write_text(original)
            request = root / "request.json"
            request.write_text(json.dumps({"repositories": [repository.as_uri()], "classpaths": {"main": {"dependencies": ["keep", "remove"]}}}))

            def fake_compare(input_path):
                data = json.loads(input_path.read_text())
                path = pathlib.Path(data["repositories"][0].removeprefix("file://"))
                mismatch = "keep" in data["classpaths"]["main"]["dependencies"] and "needed" in (path / "a.pom").read_text()
                return not mismatch, [{"engine": "rust"}, {"engine": "java"}]

            destination = root / "reduced"
            with patch.object(MODULE, "compare", fake_compare):
                MODULE.minimize(request, destination)
            reduced = json.loads((destination / "request.json").read_text())
            self.assertEqual(reduced["classpaths"]["main"]["dependencies"], ["keep"])
            self.assertNotIn("irrelevant", (destination / "repository-0/a.pom").read_text())
            self.assertEqual(pom.read_text(), original)
            self.assertEqual(json.loads(request.read_text())["classpaths"]["main"]["dependencies"], ["keep", "remove"])
            with self.assertRaises(ValueError):
                MODULE.minimize(request, destination)


if __name__ == "__main__":
    unittest.main()
