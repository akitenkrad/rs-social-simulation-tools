import importlib.util
import io
import json
import tempfile
import unittest
import urllib.error
from pathlib import Path
from unittest import mock


SCRIPT_PATH = Path(__file__).with_name("downstream.py")


def load_downstream():
    spec = importlib.util.spec_from_file_location("downstream", SCRIPT_PATH)
    module = importlib.util.module_from_spec(spec)
    assert spec.loader is not None
    spec.loader.exec_module(module)
    return module


downstream = load_downstream()


class NormalizeGithubRepoTests(unittest.TestCase):
    def test_normalizes_supported_github_urls(self):
        cases = {
            "git@github.com:owner/repository.git": "owner/repository",
            "https://github.com/owner/repository": "owner/repository",
            "https://github.com/owner/repository.git": "owner/repository",
        }

        for url, expected in cases.items():
            with self.subTest(url=url):
                self.assertEqual(downstream.normalize_github_repo(url), expected)

    def test_rejects_non_github_or_malformed_urls(self):
        for url in (
            "https://example.com/owner/repository.git",
            "git@github.com:owner.git",
            "https://github.com/owner/repository/extra",
        ):
            with self.subTest(url=url):
                with self.assertRaises(ValueError):
                    downstream.normalize_github_repo(url)


class FetchTests(unittest.TestCase):
    def test_retries_transient_failures_up_to_three_attempts(self):
        responses = [
            urllib.error.URLError("temporary"),
            urllib.error.URLError("temporary"),
            io.BytesIO(b"available"),
        ]
        with mock.patch.object(
            downstream.urllib.request, "urlopen", side_effect=responses
        ) as urlopen, mock.patch.object(downstream.time, "sleep"):
            contents = downstream.fetch_text("https://example.test/file", timeout=1)

        self.assertEqual(contents, "available")
        self.assertEqual(urlopen.call_count, 3)

    def test_does_not_retry_http_404(self):
        not_found = urllib.error.HTTPError(
            "https://example.test/file", 404, "Not Found", None, None
        )
        with mock.patch.object(
            downstream.urllib.request, "urlopen", side_effect=not_found
        ) as urlopen:
            contents = downstream.fetch_text("https://example.test/file", timeout=1)

        self.assertIsNone(contents)
        self.assertEqual(urlopen.call_count, 1)


class CargoLockTests(unittest.TestCase):
    def test_detects_socsim_git_source(self):
        lock = """
[[package]]
name = "socsim-core"
source = "git+https://github.com/akitenkrad/rs-social-simulation-tools?branch=main#012345"
"""

        self.assertTrue(downstream.lock_uses_socsim(lock))

    def test_ignores_similar_but_nonmatching_sources(self):
        for lock in (
            'source = "registry+https://github.com/akitenkrad/rs-social-simulation-tools"',
            'source = "git+https://github.com/another/rs-social-simulation-tools#012345"',
            'checksum = "git+https://github.com/akitenkrad/rs-social-simulation-tools"',
        ):
            with self.subTest(lock=lock):
                self.assertFalse(downstream.lock_uses_socsim(lock))


class DiscoverTests(unittest.TestCase):
    def test_uses_injected_fetcher_and_sorts_by_submodule_path(self):
        gitmodules = """
[submodule "zeta"]
    path = studies/zeta
    url = git@github.com:owner/zeta.git
[submodule "alpha"]
    path = studies/alpha
    url = https://github.com/owner/alpha.git
[submodule "other"]
    path = studies/other
    url = https://github.com/owner/other
[submodule "private"]
    path = studies/private
    url = https://github.com/owner/private.git
"""
        socsim_lock = (
            'source = "git+https://github.com/akitenkrad/'
            'rs-social-simulation-tools#012345"'
        )
        responses = {
            downstream.GITMODULES_URL: gitmodules,
            "https://raw.githubusercontent.com/owner/alpha/HEAD/Cargo.lock": socsim_lock,
            "https://raw.githubusercontent.com/owner/zeta/HEAD/Cargo.lock": socsim_lock,
            "https://raw.githubusercontent.com/owner/other/HEAD/Cargo.lock": "version = 4",
            "https://raw.githubusercontent.com/owner/private/HEAD/Cargo.lock": None,
        }
        requested = []

        def fetcher(url):
            requested.append(url)
            return responses[url]

        matrix, skipped = downstream.discover_repositories(fetcher=fetcher)

        self.assertEqual(
            matrix,
            {
                "include": [
                    {"name": "alpha", "repo": "owner/alpha", "path": "studies/alpha"},
                    {"name": "zeta", "repo": "owner/zeta", "path": "studies/zeta"},
                ]
            },
        )
        self.assertEqual(
            skipped,
            [
                {
                    "name": "private",
                    "repo": "owner/private",
                    "path": "studies/private",
                    "reason": "not fetchable (private, or no Cargo.lock)",
                }
            ],
        )
        self.assertIn(downstream.GITMODULES_URL, requested)

    def test_does_not_hide_non_404_fetch_failure(self):
        gitmodules = """
[submodule "alpha"]
    path = studies/alpha
    url = https://github.com/owner/alpha.git
"""

        def fetcher(url):
            if url == downstream.GITMODULES_URL:
                return gitmodules
            raise downstream.FetchError("temporary failure after retries")

        with self.assertRaises(downstream.FetchError):
            downstream.discover_repositories(fetcher=fetcher)


class PatchArgsTests(unittest.TestCase):
    def test_emits_two_lines_per_crate_with_absolute_paths(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            for directory, package in (("z", "socsim-z"), ("a", "socsim-a")):
                crate = root / "crates" / directory
                crate.mkdir(parents=True)
                (crate / "Cargo.toml").write_text(
                    f'[package]\nname = "{package}"\nversion = "0.1.0"\n',
                    encoding="utf-8",
                )

            lines = downstream.patch_args(root)
            resolved_root = root.resolve()

            self.assertEqual(
                lines,
                [
                    "--config",
                    f'patch."{downstream.SOCSIM_GIT_URL}".socsim-a.path="{resolved_root / "crates" / "a"}"',
                    "--config",
                    f'patch."{downstream.SOCSIM_GIT_URL}".socsim-z.path="{resolved_root / "crates" / "z"}"',
                ],
            )


class SummarizeTests(unittest.TestCase):
    def write_result(self, root, name, **overrides):
        result = {
            "name": name,
            "path": f"studies/{name}",
            "repo": f"owner/{name}",
            "build": "ok",
            "test": "ok",
            "passed": 3,
            "failed": 0,
            "ignored": 1,
        }
        result.update(overrides)
        target = root / name / "result.json"
        target.parent.mkdir(parents=True)
        target.write_text(json.dumps(result), encoding="utf-8")

    def expected(self, *names):
        return {
            "include": [
                {"name": name, "repo": f"owner/{name}", "path": f"studies/{name}"}
                for name in names
            ]
        }

    def test_success_table_and_exit_code(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self.write_result(root, "alpha")

            markdown, exit_code = downstream.summarize_results(
                root, skipped=[], expected=self.expected("alpha")
            )

        self.assertEqual(exit_code, 0)
        self.assertIn("| downstream (path) | build | test | passed | failed | ignored |", markdown)
        self.assertIn("| studies/alpha | ok | ok | 3 | 0 | 1 |", markdown)

    def test_failed_build_or_test_returns_failure(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self.write_result(root, "alpha", test="fail", failed=2)

            markdown, exit_code = downstream.summarize_results(
                root, skipped=[], expected=self.expected("alpha")
            )

        self.assertEqual(exit_code, 1)
        self.assertIn("| studies/alpha | ok | fail | 3 | 2 | 1 |", markdown)

    def test_missing_result_is_reported_and_returns_failure(self):
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            self.write_result(root, "alpha")

            markdown, exit_code = downstream.summarize_results(
                root, skipped=[], expected=self.expected("alpha", "beta")
            )

        self.assertEqual(exit_code, 1)
        self.assertIn("| studies/beta | no result | no result | 0 | 0 | 0 |", markdown)

    def test_all_skipped_returns_success_and_explains_reason(self):
        skipped = [
            {
                "name": "private",
                "repo": "owner/private",
                "path": "studies/private",
                "reason": "not fetchable (private, or no Cargo.lock)",
            }
        ]
        with tempfile.TemporaryDirectory() as temp_dir:
            markdown, exit_code = downstream.summarize_results(
                Path(temp_dir), skipped=skipped, expected={"include": []}
            )

        self.assertEqual(exit_code, 0)
        self.assertIn("No downstream repository was tested.", markdown)
        self.assertIn("| studies/private | owner/private | not fetchable (private, or no Cargo.lock) |", markdown)


if __name__ == "__main__":
    unittest.main()
