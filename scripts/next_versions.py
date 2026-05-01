#!/usr/bin/env python3

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CARGO_TOML = ROOT / "Cargo.toml"
ANDROID_BUILD = ROOT / "apps/musicd-android/app/build.gradle.kts"

VERSION_RE = re.compile(r'^\s*version\s*=\s*"(?P<version>\d+\.\d+\.\d+)"\s*$')
VERSION_NAME_RE = re.compile(r'^\s*versionName\s*=\s*"(?P<version>\d+\.\d+\.\d+)"\s*$')
COMMIT_RE = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]+)\))?(?P<breaking>!)?:\s+(?P<description>.+)$",
    re.IGNORECASE,
)

APP_SCOPES = {"app", "android", "mobile"}
API_SCOPES = {"api", "backend", "musicd"}
SHARED_SCOPES = {"shared", "repo", "both", "all"}
BUMP_ORDER = {"none": 0, "patch": 1, "minor": 2, "major": 3}
PATCH_TYPES = {"fix", "perf", "refactor", "revert"}


@dataclass
class CommitInfo:
    sha: str
    subject: str
    body: str
    type_name: str | None
    scopes: list[str]
    components: set[str]
    bump: str
    breaking: bool


def read_line_match(path: Path, pattern: re.Pattern[str], description: str) -> str:
    for line in path.read_text().splitlines():
        match = pattern.match(line)
        if match:
            return match.group("version")
    raise RuntimeError(f"Could not find {description} in {path}")


def run_git(args: list[str]) -> str:
    result = subprocess.run(
        ["git", *args],
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


def list_tags(prefix: str) -> list[str]:
    raw = run_git(["tag", "--list", f"{prefix}*"])
    tags = [line.strip() for line in raw.splitlines() if line.strip()]
    return sorted(tags, key=semver_key, reverse=True)


def semver_key(tag: str) -> tuple[int, int, int]:
    match = re.search(r"(\d+)\.(\d+)\.(\d+)$", tag)
    if not match:
        return (0, 0, 0)
    return tuple(int(part) for part in match.groups())


def latest_tag(prefix: str) -> str | None:
    tags = list_tags(prefix)
    return tags[0] if tags else None


def collect_commits(since_tag: str | None) -> list[CommitInfo]:
    args = ["log", "--format=%H%x1f%s%x1f%b%x1e"]
    if since_tag:
        args.insert(1, f"{since_tag}..HEAD")

    raw = run_git(args)
    commits: list[CommitInfo] = []
    for entry in raw.split("\x1e"):
        if not entry.strip():
            continue
        sha, subject, body = (entry.split("\x1f", 2) + ["", "", ""])[:3]
        match = COMMIT_RE.match(subject.strip())
        type_name = None
        scopes: list[str] = []
        components: set[str] = set()
        bump = "none"
        breaking = False
        if match:
            type_name = match.group("type").lower()
            raw_scope = (match.group("scope") or "").strip().lower()
            scopes = [part.strip() for part in re.split(r"[,/]", raw_scope) if part.strip()]
            breaking = bool(match.group("breaking")) or "BREAKING CHANGE:" in body
            components = scopes_to_components(scopes)
            bump = classify_bump(type_name, breaking)

        commits.append(
            CommitInfo(
                sha=sha,
                subject=subject.strip(),
                body=body.strip(),
                type_name=type_name,
                scopes=scopes,
                components=components,
                bump=bump,
                breaking=breaking,
            )
        )
    return commits


def scopes_to_components(scopes: list[str]) -> set[str]:
    components: set[str] = set()
    for scope in scopes:
        if scope in APP_SCOPES:
            components.add("app")
        elif scope in API_SCOPES:
            components.add("api")
        elif scope in SHARED_SCOPES:
            components.update({"app", "api"})
    return components


def classify_bump(type_name: str, breaking: bool) -> str:
    if breaking:
        return "major"
    if type_name == "feat":
        return "minor"
    if type_name in PATCH_TYPES:
        return "patch"
    return "none"


def highest_bump(commits: list[CommitInfo], component: str) -> str:
    highest = "none"
    for commit in commits:
        if component not in commit.components:
            continue
        if BUMP_ORDER[commit.bump] > BUMP_ORDER[highest]:
            highest = commit.bump
    return highest


def bump_version(version: str, bump: str) -> str:
    major, minor, patch = (int(part) for part in version.split("."))
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    return version


def release_plan(name: str, current_version: str, tag_prefix: str) -> dict[str, object]:
    tag = latest_tag(tag_prefix)
    if tag is None:
        return {
            "component": name,
            "current_version": current_version,
            "last_tag": None,
            "recommended_bump": "none",
            "next_version": current_version,
            "matched_commits": [],
            "note": f"No {tag_prefix}<semver> tag found yet. Treat {current_version} as the baseline release.",
        }

    commits = collect_commits(since_tag=tag)
    bump = highest_bump(commits, name)
    matched = [
        {
            "sha": commit.sha[:8],
            "subject": commit.subject,
            "bump": commit.bump,
            "scopes": commit.scopes,
        }
        for commit in commits
        if name in commit.components and commit.bump != "none"
    ]
    note = "No scoped conventional commits since the last release tag."
    if matched:
        note = f"{len(matched)} scoped conventional commit(s) since {tag}."
    return {
        "component": name,
        "current_version": current_version,
        "last_tag": tag,
        "recommended_bump": bump,
        "next_version": bump_version(current_version, bump),
        "matched_commits": matched,
        "note": note,
    }


def render_markdown(result: dict[str, object]) -> str:
    lines = [
        "## Version Plan",
        "",
        "| Component | Current | Last tag | Bump | Next |",
        "| --- | --- | --- | --- | --- |",
    ]
    for component in ("api", "app"):
        plan = result[component]
        lines.append(
            "| {component} | {current_version} | {last_tag} | {recommended_bump} | {next_version} |".format(
                component=component,
                current_version=plan["current_version"],
                last_tag=plan["last_tag"] or "baseline only",
                recommended_bump=plan["recommended_bump"],
                next_version=plan["next_version"],
            )
        )

    for component in ("api", "app"):
        plan = result[component]
        lines.extend(
            [
                "",
                f"### {component.upper()}",
                "",
                plan["note"],
            ]
        )
        matched = plan["matched_commits"]
        if matched:
            lines.append("")
            for commit in matched:
                scopes = ",".join(commit["scopes"]) or "unscoped"
                lines.append(
                    f"- `{commit['sha']}` `{commit['bump']}` `{scopes}` {commit['subject']}"
                )
    return "\n".join(lines) + "\n"


def build_result() -> dict[str, object]:
    api_version = read_line_match(CARGO_TOML, VERSION_RE, "workspace version")
    app_version = read_line_match(ANDROID_BUILD, VERSION_NAME_RE, "Android versionName")
    return {
        "api": release_plan("api", api_version, "api-v"),
        "app": release_plan("app", app_version, "app-v"),
    }


def main() -> int:
    parser = argparse.ArgumentParser(description="Calculate next app/api versions from scoped conventional commits.")
    parser.add_argument("--format", choices=("json", "markdown"), default="json")
    args = parser.parse_args()

    try:
        result = build_result()
    except (RuntimeError, subprocess.CalledProcessError) as exc:
        print(str(exc), file=sys.stderr)
        return 1

    if args.format == "json":
        print(json.dumps(result, indent=2))
    else:
        print(render_markdown(result), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
