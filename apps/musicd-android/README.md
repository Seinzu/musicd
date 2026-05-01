# musicd Android

This directory contains the first Android controller scaffold for `musicd`.

What is included:

- standalone Gradle Android project under `apps/musicd-android`
- Compose-based single-activity app shell
- server onboarding flow
- renderer picker
- `Home`, `Library`, and `Queue` tabs
- thin API client for the Android-facing `musicd` endpoints

What is not included yet:

- release signing config
- tablet/foldable layouts
- queue editing gestures
- background playback / media session integration

Build locally:

```bash
./gradlew :app:assembleDebug
```

CI now also builds and uploads the debug APK from GitHub Actions through `.github/workflows/android-debug-apk.yml`.
