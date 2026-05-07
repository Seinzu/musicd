# musicd Android

This directory contains the native Android controller for `musicd`.

What is included:

- standalone Gradle Android project under `apps/musicd-android`
- Compose-based single-activity app
- server onboarding and server identity display
- renderer picker with physical renderers plus `This phone`
- `Home`, `Library`, and `Queue` tabs
- artist, album, and track browsing with grouped search
- queue editing and transport controls
- SSE-backed live updates
- playback notification and media-session integration
- optional `android_local` playback using `Media3` / `ExoPlayer`
- thin API client for the Android-facing `musicd` endpoints

What is not included yet:

- release signing config
- tablet/foldable-specific layouts
- local offline metadata/artwork cache
- release automation for signed APKs

Build locally:

```bash
./gradlew :app:assembleDebug
```

CI now also builds and uploads the debug APK from GitHub Actions through `.github/workflows/android-debug-apk.yml`.
