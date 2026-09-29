# musicd

Local-network music server (Rust) with an Android controller app. See `README.md` for the full layout and `docs/` for design notes.

## Layout

- `apps/musicd`: Rust HTTP service, the `api` component (routes in `src/http/router.rs`, handlers in `src/handlers.rs`, SQLite in `src/db/`)
- `apps/musicd-cli`: `musicdctl` CLI, the `cli` component
- `apps/musicd-android`: Android app, the `app` component (Kotlin + Compose; `app/` and `companion/` modules)
- `crates/musicd-core`, `crates/musicd-upnp`: shared Rust crates (versioned with `api`)
- `scripts/recommender`: Bun/TypeScript tooling that produces recommendation imports

## Commits and PR titles

Use scoped conventional commits: `type(scope): description`, e.g. `feat(app): allow suggestions to be dismissed`.
`scripts/next_versions.py` reads them to decide version bumps, so the scope matters:

- `app` for the Android app, `api` for the Rust HTTP service, `cli` for `musicdctl`
- `shared` when a change spans more than one component (e.g. a new endpoint plus the app using it)
- `feat` gives a minor bump; `fix`, `perf`, `refactor` and `revert` give a patch; `!` or `BREAKING CHANGE:` gives a major. Other types (`test`, `docs`, `chore`, `ci`) don't bump anything
- Unscoped commits are ignored by the planner. Always include a scope

When a change touches several components, prefer one commit per component (e.g. `feat(api): ...` then `feat(app): ...`). Give PR titles the same format, since a squash merge keeps only the title.
Full rules: `docs/versioning.md`.

## Build and test

Rust (from the repo root):

```bash
cargo fmt --all
cargo clippy -p musicd --all-targets
cargo test -p musicd
```

Android (from `apps/musicd-android`, needs `ANDROID_HOME` pointing at an SDK with platform 35):

```bash
./gradlew :app:testDebugUnitTest   # JVM unit tests in app/src/test
./gradlew :app:assembleDebug
```

CI (`.github/workflows/android-debug-apk.yml`) runs the Android unit tests and builds the debug APK for changes under `apps/musicd-android`.

## Conventions

- When adding an HTTP route, also add it to the known-route list in `apps/musicd/src/metrics.rs`, and document routes the Android app uses in `docs/android-api-contract.md`.
- The Android app talks to the server through `data/MusicdApi.kt` → `data/MusicdRepository.kt` → `ui/MusicdViewModel.kt`. Keep pure logic out of the view model (in plain Kotlin files) so it can be unit tested without Android.
