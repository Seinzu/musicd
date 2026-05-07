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
CLI_CARGO_TOML = ROOT / "apps/musicd-cli/Cargo.toml"
LOCKFILE = ROOT / "Cargo.lock"
ANDROID_BUILD = ROOT / "apps/musicd-android/app/build.gradle.kts"

VERSION_RE = re.compile(r'^\s*version\s*=\s*"(?P<version>\d+\.\d+\.\d+)"\s*$')
VERSION_ASSIGNMENT_RE = re.compile(
    r'(?P<prefix>^(\s*)version\s*=\s*")(?P<version>\d+\.\d+\.\d+)(?P<suffix>"\s*$)'
)
VERSION_NAME_RE = re.compile(r'^\s*versionName\s*=\s*"(?P<version>\d+\.\d+\.\d+)"\s*$')
VERSION_NAME_ASSIGNMENT_RE = re.compile(
    r'(?P<prefix>^(\s*)versionName\s*=\s*")(?P<version>\d+\.\d+\.\d+)(?P<suffix>"\s*$)'
)
VERSION_CODE_ASSIGNMENT_RE = re.compile(
    r"(?P<prefix>^(\s*)versionCode\s*=\s*)(?P<version_code>\d+)(?P<suffix>\s*$)"
)
COMMIT_RE = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]+)\))?(?P<breaking>!)?:\s+(?P<description>.+)$",
    re.IGNORECASE,
)

APP_SCOPES = {"app", "android", "mobile"}
API_SCOPES = {"api", "backend", "musicd"}
CLI_SCOPES = {"cli", "ctl", "musicd-cli", "musicdctl"}
ALL_COMPONENTS = ("api", "cli", "app")
SHARED_SCOPES = {"shared", "repo", "both", "all"}
BUMP_ORDER = {"none": 0, "patch": 1, "minor": 2, "major": 3}
PATCH_TYPES = {"fix", "perf", "refactor", "revert"}
LOCKED_CARGO_PACKAGES = {
    "musicd": "api",
    "musicd-core": "api",
    "musicd-upnp": "api",
    "musicd-cli": "cli",
}


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


def parse_version(version: str) -> tuple[int, int, int]:
    return tuple(int(part) for part in version.split("."))


def format_version(version: tuple[int, int, int]) -> str:
    return ".".join(str(part) for part in version)


def max_version(*versions: str) -> str:
    return max(versions, key=parse_version)


def tag_version(tag: str) -> str:
    return format_version(semver_key(tag))


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
        sha, subject, body = (entry.strip().split("\x1f", 2) + ["", "", ""])[:3]
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
        elif scope in CLI_SCOPES:
            components.add("cli")
        elif scope in SHARED_SCOPES:
            components.update(ALL_COMPONENTS)
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
            "tag_prefix": tag_prefix,
        }

    commits = collect_commits(since_tag=tag)
    bump = highest_bump(commits, name)
    previous_version = tag_version(tag)
    required_version = previous_version
    if bump != "none":
        required_version = bump_version(previous_version, bump)
    next_version = max_version(current_version, required_version)
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
    if parse_version(current_version) < parse_version(required_version):
        note = f"{note} Source version is behind the planned version."
    return {
        "component": name,
        "current_version": current_version,
        "last_tag": tag,
        "last_version": previous_version,
        "recommended_bump": bump,
        "next_version": next_version,
        "matched_commits": matched,
        "note": note,
        "tag_prefix": tag_prefix,
    }


def render_markdown(result: dict[str, object]) -> str:
    lines = [
        "## Version Plan",
        "",
        "| Component | Current | Last tag | Bump | Next |",
        "| --- | --- | --- | --- | --- |",
    ]
    for component in ALL_COMPONENTS:
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

    for component in ALL_COMPONENTS:
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
    cli_version = read_line_match(CLI_CARGO_TOML, VERSION_RE, "CLI package version")
    app_version = read_line_match(ANDROID_BUILD, VERSION_NAME_RE, "Android versionName")
    return {
        "api": release_plan("api", api_version, "api-v"),
        "cli": release_plan("cli", cli_version, "musicd-cli-v"),
        "app": release_plan("app", app_version, "app-v"),
    }


def tag_plan(result: dict[str, object]) -> list[dict[str, str]]:
    tags: list[dict[str, str]] = []
    for component in ALL_COMPONENTS:
        plan = result[component]
        last_tag = plan["last_tag"]
        next_version = plan["next_version"]
        tag_prefix = plan["tag_prefix"]
        recommended_bump = plan["recommended_bump"]
        last_version = plan.get("last_version")

        if last_tag is None:
            tags.append(
                {
                    "component": component,
                    "tag": f"{tag_prefix}{current_version}",
                    "kind": "baseline",
                    "version": current_version,
                    "reason": f"Create the first {component} release tag at the current baseline.",
                }
            )
            continue

        if last_version is not None and parse_version(next_version) <= parse_version(str(last_version)):
            continue

        reason = f"{recommended_bump} bump from {last_tag}."
        if recommended_bump == "none":
            reason = f"Source version is ahead of {last_tag}."
        tags.append(
            {
                "component": component,
                "tag": f"{tag_prefix}{next_version}",
                "kind": "release",
                "version": next_version,
                "reason": reason,
            }
        )
    return tags


def render_tag_plan_markdown(planned_tags: list[dict[str, str]]) -> str:
    if not planned_tags:
        return "## Tag Plan\n\nNo new tags are required from the current scoped commit history.\n"

    lines = [
        "## Tag Plan",
        "",
        "| Component | Tag | Kind | Reason |",
        "| --- | --- | --- | --- |",
    ]
    for item in planned_tags:
        lines.append(
            f"| {item['component']} | `{item['tag']}` | {item['kind']} | {item['reason']} |"
        )
    return "\n".join(lines) + "\n"


def create_tags(planned_tags: list[dict[str, str]]) -> None:
    if not planned_tags:
        return

    existing_tags = set(list_tags("api-v") + list_tags("app-v"))
    for item in planned_tags:
        tag = item["tag"]
        if tag in existing_tags:
            raise RuntimeError(f"Refusing to create tag {tag}: it already exists.")
        message = f"{item['component']} {item['version']}"
        subprocess.run(
            ["git", "tag", "-a", tag, "-m", message],
            cwd=ROOT,
            check=True,
        )


def replace_assignment(path: Path, pattern: re.Pattern[str], value: str, description: str) -> bool:
    changed = False
    lines: list[str] = []
    matched = False
    for line in path.read_text().splitlines():
        match = pattern.match(line)
        if match:
            matched = True
            new_line = f"{match.group('prefix')}{value}{match.group('suffix')}"
            changed = changed or new_line != line
            lines.append(new_line)
        else:
            lines.append(line)
    if not matched:
        raise RuntimeError(f"Could not find {description} in {path}")
    if changed:
        path.write_text("\n".join(lines) + "\n")
    return changed


def android_version_code(version: str) -> str:
    major, minor, patch = parse_version(version)
    return str(major * 10000 + minor * 100 + patch)


def update_lockfile_versions(versions: dict[str, str]) -> bool:
    if not LOCKFILE.exists():
        return False

    changed = False
    current_package: str | None = None
    lines: list[str] = []
    for line in LOCKFILE.read_text().splitlines():
        name_match = re.match(r'^name = "(?P<name>[^"]+)"$', line)
        if line == "[[package]]":
            current_package = None
        elif name_match:
            current_package = name_match.group("name")

        target_component = LOCKED_CARGO_PACKAGES.get(current_package or "")
        version_match = VERSION_ASSIGNMENT_RE.match(line)
        if target_component and version_match:
            new_version = versions[target_component]
            new_line = f"{version_match.group('prefix')}{new_version}{version_match.group('suffix')}"
            changed = changed or new_line != line
            lines.append(new_line)
        else:
            lines.append(line)

    if changed:
        LOCKFILE.write_text("\n".join(lines) + "\n")
    return changed


def write_versions(result: dict[str, object]) -> list[Path]:
    versions = {
        component: str(result[component]["next_version"])
        for component in ALL_COMPONENTS
    }
    changed: list[Path] = []
    if replace_assignment(
        CARGO_TOML, VERSION_ASSIGNMENT_RE, versions["api"], "workspace version"
    ):
        changed.append(CARGO_TOML)
    if replace_assignment(
        CLI_CARGO_TOML, VERSION_ASSIGNMENT_RE, versions["cli"], "CLI package version"
    ):
        changed.append(CLI_CARGO_TOML)
    if replace_assignment(
        ANDROID_BUILD, VERSION_NAME_ASSIGNMENT_RE, versions["app"], "Android versionName"
    ):
        changed.append(ANDROID_BUILD)
    if replace_assignment(
        ANDROID_BUILD,
        VERSION_CODE_ASSIGNMENT_RE,
        android_version_code(versions["app"]),
        "Android versionCode",
    ):
        changed.append(ANDROID_BUILD)
    if update_lockfile_versions(versions):
        changed.append(LOCKFILE)
    return list(dict.fromkeys(changed))


def validate_sources_match_planned_tags(
    planned_tags: list[dict[str, str]], result: dict[str, object]
) -> None:
    stale_components = []
    for item in planned_tags:
        component = item["component"]
        if result[component]["current_version"] != item["version"]:
            stale_components.append(
                f"{component} source is {result[component]['current_version']} but planned tag is {item['tag']}"
            )
    if stale_components:
        details = "\n".join(f"- {line}" for line in stale_components)
        raise RuntimeError(
            "Refusing to create tags until source versions match the planned tags. "
            "Run with --write-versions, commit the changes, then create tags.\n"
            f"{details}"
        )


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Calculate next api/cli/app versions from scoped conventional commits."
    )
    parser.add_argument("--format", choices=("json", "markdown"), default="json")
    parser.add_argument(
        "--tag-plan",
        action="store_true",
        help="Print the api/cli/app Git tags that should be created next.",
    )
    parser.add_argument(
        "--write-versions",
        action="store_true",
        help="Update Cargo, Cargo.lock, and Android Gradle versions to the planned versions.",
    )
    parser.add_argument("--create-tags", action="store_true", help="Create the planned Git tags on HEAD.")
    args = parser.parse_args()

    if args.create_tags and not args.tag_plan:
        args.tag_plan = True

    try:
        result = build_result()
    except (RuntimeError, subprocess.CalledProcessError) as exc:
        print(str(exc), file=sys.stderr)
        return 1

    if args.write_versions and args.create_tags:
        print(
            "Refusing to write files and create tags in one run. "
            "Run --write-versions, commit the changes, then run --create-tags.",
            file=sys.stderr,
        )
        return 1

    changed_files: list[Path] = []
    if args.write_versions:
        try:
            changed_files = write_versions(result)
            result = build_result()
        except RuntimeError as exc:
            print(str(exc), file=sys.stderr)
            return 1

    if args.tag_plan:
        planned_tags = tag_plan(result)
        if args.create_tags:
            try:
                validate_sources_match_planned_tags(planned_tags, result)
                create_tags(planned_tags)
            except (RuntimeError, subprocess.CalledProcessError) as exc:
                print(str(exc), file=sys.stderr)
                return 1

        if args.format == "json":
            print(json.dumps({"versions": result, "tags": planned_tags}, indent=2))
        else:
            print(render_markdown(result), end="")
            print()
            print(render_tag_plan_markdown(planned_tags), end="")
        if args.write_versions:
            print_changed_files(changed_files)
    elif args.format == "json":
        print(json.dumps(result, indent=2))
        if args.write_versions:
            print_changed_files(changed_files, stderr=True)
    else:
        print(render_markdown(result), end="")
        if args.write_versions:
            print_changed_files(changed_files)
    return 0


def print_changed_files(changed_files: list[Path], stderr: bool = False) -> None:
    stream = sys.stderr if stderr else sys.stdout
    if changed_files:
        print("\nUpdated version files:", file=stream)
        for path in changed_files:
            print(f"- {path.relative_to(ROOT)}", file=stream)
    else:
        print("\nVersion files already matched the plan.", file=stream)


if __name__ == "__main__":
    raise SystemExit(main())
