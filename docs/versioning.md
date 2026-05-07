# Versioning

This repository uses a split versioning model:

- `api`: the Rust backend and Docker image release line
- `cli`: the `musicdctl` cargo-dist release line
- `app`: the Android controller release line

## Current source versions

The version values currently checked into the repo are:

- `api`: `3.1.0`
- `cli`: `2.4.0`
- `app`: `2.0.0`

These come from:

- [Cargo.toml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/Cargo.toml) for the backend workspace version
- [apps/musicd-cli/Cargo.toml](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/apps/musicd-cli/Cargo.toml) for the CLI package version
- [apps/musicd-android/app/build.gradle.kts](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/apps/musicd-android/app/build.gradle.kts) for the Android app

Component release tags remain component-scoped:

- `api-vX.Y.Z`
- `musicd-cli-vX.Y.Z`
- `app-vX.Y.Z`

Use the planner script to see the current Git-derived release state rather than relying on a hardcoded baseline in this document.

## Conventional commit scopes

Use scoped conventional commits so the version planner can decide which release line to bump:

- `feat(api): add renderer capability caching`
- `feat(cli): add a local playback renderer`
- `fix(app): avoid duplicate artist keys in library list`
- `feat(shared): expose server version in the API and Android client`
- `feat(api)!: change queue payload shape`

Supported scopes:

- app-only: `app`, `android`, `mobile`
- api-only: `api`, `backend`, `musicd`
- cli-only: `cli`, `ctl`, `musicd-cli`, `musicdctl`
- all release lines: `shared`, `repo`, `both`, `all`

## Bump rules

- breaking changes (`!` or `BREAKING CHANGE:`): `major`
- `feat(...)`: `minor`
- `fix(...)`, `perf(...)`, `refactor(...)`, `revert(...)`: `patch`
- anything else: no version bump

Unscoped commits are ignored by the planner on purpose. If a change should affect a release line, give it an explicit scope.

## Tooling

The repository now includes:

- `scripts/next_versions.py`: calculates the next `api`, `cli`, and `app` versions from Git history
- `scripts/next_versions.py --write-versions`: updates the checked-in Cargo, Cargo.lock, and Gradle versions to the planned versions
- `.github/workflows/version-plan.yml`: publishes a version summary on pushes, pull requests, and manual runs

Examples:

```bash
python3 scripts/next_versions.py --format json
python3 scripts/next_versions.py --format markdown
python3 scripts/next_versions.py --write-versions --tag-plan --format markdown
python3 scripts/next_versions.py --tag-plan --format markdown
python3 scripts/next_versions.py --tag-plan --create-tags --format markdown
```

## Tag creation

`scripts/next_versions.py` can now also decide which Git tags should be created next from the scoped conventional commits since the last component tag.

Before creating tags, update and commit the source versions:

```bash
python3 scripts/next_versions.py --write-versions --tag-plan --format markdown
git add Cargo.toml Cargo.lock apps/musicd-cli/Cargo.toml apps/musicd-android/app/build.gradle.kts
git commit -m "chore(repo): bump versions"
```

Dry-run the next tags:

```bash
python3 scripts/next_versions.py --tag-plan --format markdown
```

Create the tags on the current `HEAD` commit:

```bash
python3 scripts/next_versions.py --tag-plan --create-tags --format markdown
```

Behavior:

- if no `api-v...`, `musicd-cli-v...`, or `app-v...` tag exists yet, it proposes the current in-tree version for that component
- if scoped commits since the last tag imply a bump, it proposes the next semver tag for that component
- if no scoped bump is needed, it creates nothing for that component
- `--create-tags` refuses to create tags when source versions do not match the planned tag versions

## Docker publishing

The Docker publish workflow is now ready for API-specific release tags:

- `api-v1.2.3`
- legacy `v1.2.3` still works during the transition

Release tags publish:

- `latest`
- the original Git tag
- semver aliases like `1.2.3`, `1.2`, and `1`
- `sha-<commit>`
