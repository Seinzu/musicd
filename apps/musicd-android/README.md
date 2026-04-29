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

- Gradle wrapper files
- release signing config
- tablet/foldable layouts
- queue editing gestures
- search and album-detail flows

This environment did not have `gradle` installed, so the wrapper could not be generated here. The easiest next step is to open this project in Android Studio and let it finish sync / wrapper setup there.
