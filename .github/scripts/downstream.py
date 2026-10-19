"""Discover, patch, and summarize downstream socsim compatibility checks."""

from __future__ import annotations

import argparse
import configparser
import json
import re
import sys
import time
import tomllib
import urllib.error
import urllib.request
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any


PARENT_REPO = "akitenkrad/social-simulation-replications"
CURRENT_REPO = "akitenkrad/rs-social-simulation-tools"
SOCSIM_GIT_URL = "https://github.com/akitenkrad/rs-social-simulation-tools"
GITMODULES_URL = (
    f"https://raw.githubusercontent.com/{PARENT_REPO}/HEAD/.gitmodules"
)
UNAVAILABLE_REASON = "not fetchable (private, or no Cargo.lock)"
SOCSIM_SOURCE_PREFIX = f'source = "git+{SOCSIM_GIT_URL}'
_GITHUB_URL_PATTERNS = (
    re.compile(
        r"git@github\.com:(?P<owner>[A-Za-z0-9_.-]+)/"
        r"(?P<repo>[A-Za-z0-9_.-]+?)(?:\.git)?$"
    ),
    re.compile(
        r"https://github\.com/(?P<owner>[A-Za-z0-9_.-]+)/"
        r"(?P<repo>[A-Za-z0-9_.-]+?)(?:\.git)?/?$"
    ),
)


class FetchError(RuntimeError):
    """Raised after a remote resource repeatedly fails to download."""


def fetch_text(url: str, timeout: float = 30, attempts: int = 3) -> str | None:
    """Fetch UTF-8 text, returning None for HTTP 404 and retrying other errors."""
    last_error: BaseException | None = None
    for attempt in range(attempts):
        try:
            with urllib.request.urlopen(url, timeout=timeout) as response:
                return response.read().decode("utf-8")
        except urllib.error.HTTPError as error:
            if error.code == 404:
                return None
            last_error = error
        except (urllib.error.URLError, TimeoutError, OSError) as error:
            last_error = error

        if attempt + 1 < attempts:
            time.sleep(1)

    raise FetchError(f"failed to fetch {url} after {attempts} attempts") from last_error


def normalize_github_repo(url: str) -> str:
    """Normalize a supported GitHub clone URL to owner/repository."""
    candidate = url.strip()
    for pattern in _GITHUB_URL_PATTERNS:
        match = pattern.fullmatch(candidate)
        if match:
            owner = match.group("owner")
            repository = match.group("repo")
            if owner not in {".", ".."} and repository not in {".", ".."}:
                return f"{owner}/{repository}"
    raise ValueError(f"unsupported GitHub repository URL: {url}")


def lock_uses_socsim(contents: str) -> bool:
    """Return whether a Cargo.lock contains a socsim git dependency source."""
    return any(
        line.strip().startswith(SOCSIM_SOURCE_PREFIX)
        for line in contents.splitlines()
    )


def _parse_submodules(contents: str) -> list[tuple[str, str]]:
    parser = configparser.ConfigParser(interpolation=None)
    parser.read_string(contents)
    submodules = []
    for section in parser.sections():
        if section.startswith('submodule "'):
            submodules.append((parser.get(section, "path"), parser.get(section, "url")))
    return submodules


def discover_repositories(
    fetcher: Callable[[str], str | None] = fetch_text,
) -> tuple[dict[str, list[dict[str, str]]], list[dict[str, str]]]:
    """Discover public submodules whose lockfiles reference socsim by git."""
    gitmodules = fetcher(GITMODULES_URL)
    if gitmodules is None:
        raise FetchError(f"could not fetch required file: {GITMODULES_URL}")

    included: list[dict[str, str]] = []
    skipped: list[dict[str, str]] = []
    for path, url in sorted(_parse_submodules(gitmodules), key=lambda item: item[0]):
        try:
            repo = normalize_github_repo(url)
        except ValueError:
            continue
        if repo == CURRENT_REPO:
            continue

        name = repo.rsplit("/", 1)[1]
        lock_url = f"https://raw.githubusercontent.com/{repo}/HEAD/Cargo.lock"
        lock_contents = fetcher(lock_url)

        if lock_contents is None:
            skipped.append(
                {
                    "name": name,
                    "repo": repo,
                    "path": path,
                    "reason": UNAVAILABLE_REASON,
                }
            )
        elif lock_uses_socsim(lock_contents):
            included.append({"name": name, "repo": repo, "path": path})

    return {"include": included}, skipped


def write_discovery_outputs(
    matrix: dict[str, list[dict[str, str]]],
    skipped: list[dict[str, str]],
    output_path: Path | None,
) -> None:
    """Write compact JSON values in GitHub Actions output format."""
    lines = (
        f"matrix={json.dumps(matrix, ensure_ascii=False, separators=(',', ':'))}\n"
        f"skipped={json.dumps(skipped, ensure_ascii=False, separators=(',', ':'))}\n"
    )
    if output_path is None:
        sys.stdout.write(lines)
    else:
        with output_path.open("a", encoding="utf-8") as output:
            output.write(lines)


def patch_args(socsim_root: str | Path) -> list[str]:
    """Build Cargo --config arguments that patch every local socsim crate."""
    root = Path(socsim_root).resolve()
    arguments: list[str] = []
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        with manifest.open("rb") as stream:
            data = tomllib.load(stream)
        package_name = data["package"]["name"]
        crate_path = json.dumps(str(manifest.parent.resolve()), ensure_ascii=False)
        arguments.extend(
            (
                "--config",
                f'patch."{SOCSIM_GIT_URL}".{package_name}.path={crate_path}',
            )
        )
    return arguments


def _markdown_cell(value: object) -> str:
    return str(value).replace("\\", "\\\\").replace("|", "\\|").replace("\n", " ")


def _load_results(results_dir: Path) -> dict[str, dict[str, Any]]:
    results: dict[str, dict[str, Any]] = {}
    if not results_dir.exists():
        return results
    for result_path in sorted(results_dir.rglob("result.json")):
        with result_path.open(encoding="utf-8") as stream:
            result = json.load(stream)
        results[result["name"]] = result
    return results


def summarize_results(
    results_dir: str | Path,
    skipped: list[dict[str, Any]],
    expected: dict[str, list[dict[str, Any]]],
) -> tuple[str, int]:
    """Render downstream results as Markdown and return it with an exit code."""
    results = _load_results(Path(results_dir))
    expected_rows = sorted(expected.get("include", []), key=lambda item: item["path"])
    expected_names = {item["name"] for item in expected_rows}
    rows: list[dict[str, Any]] = []
    unsuccessful = False

    for item in expected_rows:
        result = results.get(item["name"])
        if result is None:
            rows.append(
                {
                    **item,
                    "build": "no result",
                    "test": "no result",
                    "passed": 0,
                    "failed": 0,
                    "ignored": 0,
                }
            )
            unsuccessful = True
        else:
            rows.append(result)
            if result.get("build") != "ok" or result.get("test") != "ok":
                unsuccessful = True

    for name, result in sorted(
        results.items(), key=lambda item: item[1].get("path", item[0])
    ):
        if name not in expected_names:
            rows.append(result)
            if result.get("build") != "ok" or result.get("test") != "ok":
                unsuccessful = True

    lines = ["## Downstream compatibility", ""]
    if not rows:
        lines.extend(("No downstream repository was tested.", ""))
    else:
        lines.extend(
            (
                "| downstream (path) | build | test | passed | failed | ignored |",
                "|---|---:|---:|---:|---:|---:|",
            )
        )
        for row in rows:
            lines.append(
                "| {path} | {build} | {test} | {passed} | {failed} | {ignored} |".format(
                    **{key: _markdown_cell(value) for key, value in row.items()}
                )
            )
        lines.append("")

    if skipped:
        lines.extend(
            (
                "### Skipped",
                "",
                "| skipped (path) | repo | reason |",
                "|---|---|---|",
            )
        )
        for item in sorted(skipped, key=lambda row: row["path"]):
            lines.append(
                "| {path} | {repo} | {reason} |".format(
                    **{key: _markdown_cell(value) for key, value in item.items()}
                )
            )
        lines.append("")

    return "\n".join(lines).rstrip() + "\n", int(unsuccessful)


def _json_argument(value: str, option: str) -> Any:
    try:
        return json.loads(value)
    except json.JSONDecodeError as error:
        raise SystemExit(f"{option} must be valid JSON: {error}") from error


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="command", required=True)

    discover_parser = subparsers.add_parser("discover")
    discover_parser.add_argument("--github-output", type=Path)

    patch_parser = subparsers.add_parser("patch-args")
    patch_parser.add_argument("socsim_root", type=Path)

    summarize_parser = subparsers.add_parser("summarize")
    summarize_parser.add_argument("results_dir", type=Path)
    summarize_parser.add_argument("--skipped", required=True)
    summarize_parser.add_argument("--expected", required=True)
    summarize_parser.add_argument("--summary", type=Path)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    if args.command == "discover":
        matrix, skipped = discover_repositories()
        write_discovery_outputs(matrix, skipped, args.github_output)
        return 0
    if args.command == "patch-args":
        for argument in patch_args(args.socsim_root):
            print(argument)
        return 0

    skipped = _json_argument(args.skipped, "--skipped")
    expected = _json_argument(args.expected, "--expected")
    markdown, exit_code = summarize_results(args.results_dir, skipped, expected)
    sys.stdout.write(markdown)
    if args.summary:
        with args.summary.open("a", encoding="utf-8") as summary:
            summary.write(markdown)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
