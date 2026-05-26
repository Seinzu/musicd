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
- companion app module for local Android storage scanning and local playback

What is not included yet:

- release signing config
- tablet/foldable-specific layouts
- local offline metadata/artwork cache
- release automation for signed APKs

## Opening in Android Studio

Open the Gradle project at:

```text
apps/musicd-android
```

Do not open the repository root as the Android Studio project. The Android settings file lives at `apps/musicd-android/settings.gradle.kts` and includes two modules:

- `:app`: the existing controller app
- `:companion`: the local storage / local playback companion app

After sync, Android Studio should show both run configurations. If it only shows one module, use `File > Open` and choose the `apps/musicd-android` folder directly.

## Build Locally

```bash
./gradlew :app:assembleDebug
./gradlew :companion:assembleDebug
```

Or build both:

```bash
./gradlew :app:assembleDebug :companion:assembleDebug
```

Debug APK outputs:

```text
apps/musicd-android/app/build/outputs/apk/debug/app-debug.apk
apps/musicd-android/companion/build/outputs/apk/debug/companion-debug.apk
```

## Companion App

The companion app is currently a separate Android application with package:

```text
io.musicd.android.companion
```

Install both debug APKs on the same Android device. In the controller app, choose `Use local companion`; this points the controller at the companion's localhost API on `127.0.0.1:8788`.

The companion app currently supports:

- adding Storage Access Framework music folders
- scanning MP3, AAC, FLAC, and WAV files
- storing local library metadata in Room
- exposing a localhost-only read/mutation API
- local queue and direct `content://` playback through Media3/ExoPlayer

The companion app must be running for the controller to reach the localhost API. Open `musicd Companion`, add a music folder, then run `Scan music folders` before expecting local albums/tracks to appear in the controller.

CI now also builds and uploads the debug APK from GitHub Actions through `.github/workflows/android-debug-apk.yml`.
