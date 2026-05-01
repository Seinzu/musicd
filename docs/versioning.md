# Versioning

This repository now has a first-pass split versioning model:

- `api`: the Rust backend and Docker image release line
- `app`: the Android controller release line

The current baseline for both is `1.0.0`.

## Current baseline

- `api`: `1.0.0`
- `app`: `1.0.0`

The intended Git tag names are:

- `api-v1.0.0`
- `app-v1.0.0`

The calculator treats `1.0.0` as the baseline until those tags exist.

## Conventional commit scopes

Use scoped conventional commits so the version planner can decide which release line to bump:

- `feat(api): add renderer capability caching`
- `fix(app): avoid duplicate artist keys in library list`
- `feat(shared): expose server version in the API and Android client`
- `feat(api)!: change queue payload shape`

Supported scopes:

- app-only: `app`, `android`, `mobile`
- api-only: `api`, `backend`, `musicd`
- both: `shared`, `repo`, `both`, `all`

## Bump rules

- breaking changes (`!` or `BREAKING CHANGE:`): `major`
- `feat(...)`: `minor`
- `fix(...)`, `perf(...)`, `refactor(...)`, `revert(...)`: `patch`
- anything else: no version bump

Unscoped commits are ignored by the planner on purpose. If a change should affect a release line, give it an explicit scope.

## Tooling

The repository now includes:

- `scripts/next_versions.py`: calculates the next `app` and `api` versions from Git history
- `.github/workflows/version-plan.yml`: publishes a version summary on pushes, pull requests, and manual runs

Examples:

```bash
python3 scripts/next_versions.py --format json
python3 scripts/next_versions.py --format markdown
python3 scripts/next_versions.py --tag-plan --format markdown
python3 scripts/next_versions.py --tag-plan --create-tags --format markdown
```

## Tag creation

`scripts/next_versions.py` can now also decide which Git tags should be created next from the scoped conventional commits since the last component tag.

Dry-run the next tags:

```bash
python3 scripts/next_versions.py --tag-plan --format markdown
```

Create the tags on the current `HEAD` commit:

```bash
python3 scripts/next_versions.py --tag-plan --create-tags --format markdown
```

Behavior:

- if no `api-v...` or `app-v...` tag exists yet, it proposes the current baseline tag such as `api-v1.0.0`
- if scoped commits since the last tag imply a bump, it proposes the next semver tag for that component
- if no scoped bump is needed, it creates nothing for that component

## Docker publishing

The Docker publish workflow is now ready for API-specific release tags:

- `api-v1.2.3`
- legacy `v1.2.3` still works during the transition

Release tags publish:

- `latest`
- the original Git tag
- semver aliases like `1.2.3`, `1.2`, and `1`
- `sha-<commit>`
