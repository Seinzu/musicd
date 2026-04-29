# Android Controller Plan

## Why this app

The Android app should be a native controller for `musicd`, optimized for phone use while keeping the door open for tablets and foldables. The main purpose is not local playback on the phone itself; it is browsing the library, managing the queue, and controlling playback on external renderers.

That means the Android app should treat `musicd` as the source of truth for:

- library data
- artwork
- queue state
- renderer discovery and selection
- transport/session state

## Design direction from the PDF

The design PDF points to a clear mobile product shape rather than a generic admin client.

Primary cues from [musicd · hi-fi · print.pdf](</Users/andrewrumble/Downloads/musicd · hi-fi · print.pdf>):

- dark, hi-fi-focused visual language
- large artwork-led surfaces
- bottom navigation with `Home`, `Library`, and `Queue`
- a strong search-first home screen with shortcuts and recently played content
- album detail as a focused hero screen with primary `Play` action
- queue as a first-class destination, not a modal afterthought
- transport controls docked into the queue experience
- renderer selection as a picker sheet, not a buried settings page

That is a good fit for the product we are actually building. It emphasizes browsing and control rather than filesystem management.

## Product scope

### MVP

The first Android release should support:

- connect to a `musicd` server on the local network
- browse albums and tracks
- search the library
- view album detail and track metadata
- select a renderer
- play a track
- play an album
- queue track and album actions
- view the current queue
- use transport controls: play, pause, stop, next, previous
- see now playing information and artwork

### Later

Good follow-on features after the first release:

- persistent recent servers
- favorites / recently played
- better queue editing gestures
- tablet and foldable two-pane layouts
- offline cache for lightweight metadata and artwork
- optional notifications / lock-screen controls for remote control only

## Recommended Android stack

I recommend modern native Android rather than a cross-platform shell.

- `Kotlin`
- `Jetpack Compose`
- `Material 3`
- `Navigation Compose`
- `ViewModel` + unidirectional data flow
- coroutines + `StateFlow`
- `Room` only if we decide to cache local metadata
- `OkHttp` plus either `Retrofit` or a small hand-rolled API client
- `Coil` for artwork loading

Why:

- the app is Android-first
- the design direction is mobile-native
- Compose is the best fit for a highly visual, adaptive controller UI
- the app will mostly orchestrate state from a network service rather than perform heavy local media work

Relevant current Android guidance:

- Compose and Material 3: [Material Design 3 in Compose](https://developer.android.com/develop/ui/compose/designsystems/material3)
- app architecture: [Recommended app architecture](https://developer.android.com/topic/architecture)
- `ViewModel` as screen state holder: [ViewModel overview](https://developer.android.com/topic/libraries/architecture/viewmodel?hl=en)
- adaptive layouts and navigation: [About adaptive layouts](https://developer.android.com/develop/ui/compose/layouts/adaptive), [Build adaptive navigation](https://developer.android.com/develop/ui/compose/layouts/adaptive/build-adaptive-navigation)

## App architecture

### Layers

- `ui`
  Compose screens, components, navigation, and app state
- `domain`
  optional at first; use only if orchestration becomes complex
- `data`
  repositories and HTTP clients for `musicd`

### State model

Each top-level screen should have its own screen state holder.

- `HomeViewModel`
- `LibraryViewModel`
- `AlbumViewModel`
- `QueueViewModel`
- `RendererPickerViewModel`
- `ConnectionViewModel`

This maps well to current Android guidance around screen-level state holders and unidirectional data flow.

## Proposed navigation

### Bottom navigation

- `Home`
- `Library`
- `Queue`

### Secondary flows

- album detail
- renderer picker sheet
- connection / server selection
- track metadata sheet or detail screen

## Screen plan

### Home

Purpose:

- search entry point
- recently played / shortcuts
- quick route into albums, artists, tracks
- lightweight now playing strip

Notes:

- the PDF’s greeting + search layout is a good direction
- the current backend does not yet expose enough “recently played” data, so the first version may substitute `recently added` or `recent queue items`

### Library

Purpose:

- browse albums and tracks
- filter/search
- launch album detail

Notes:

- on phones, start with album-first browsing
- keep track rows available from search results and album detail

### Album detail

Purpose:

- show artwork, album metadata, track list
- primary `Play`
- `Play Next` and `Queue`

Notes:

- the PDF already gives us a strong shape here
- this screen should probably own the “track metadata” affordance too

### Queue

Purpose:

- show now playing
- show upcoming queue
- transport controls
- queue editing
- renderer switching

Notes:

- this should become the operational center of the app
- the current web app already proves this concept

### Renderer picker

Purpose:

- show known renderers
- indicate active renderer
- discover new renderers
- switch target renderer

Notes:

- the PDF suggests a bottom sheet, which is the right mobile interaction

## Backend/API implications

The biggest planning insight is this: the Android app is now practical, but `musicd` needs a cleaner app-facing JSON API before implementation gets pleasant.

Today we already have useful pieces:

- `GET /api/albums`
- `GET /api/queue?renderer_location=...`
- `GET /api/tracks`
- `GET /api/tracks/<track_id>`
- `GET /api/renderers/discover`
- track and artwork streaming endpoints

For Android, I would want us to add or normalize these:

- `GET /api/albums/<album_id>`
- `GET /api/renderers`
- `POST /api/renderers/discover` or `POST /api/renderers/scan`
- `POST /api/play`
- `POST /api/play-album`
- `POST /api/queue/append-track`
- `POST /api/queue/append-album`
- `POST /api/queue/play-next-track`
- `POST /api/queue/play-next-album`
- `POST /api/queue/move`
- `POST /api/queue/remove`
- `POST /api/transport/play`
- `POST /api/transport/pause`
- `POST /api/transport/stop`
- `POST /api/transport/next`
- `POST /api/transport/previous`
- `GET /api/session?renderer_location=...`

The current web app still leans on query-string driven HTML flows for many mutations. That is fine for the browser UI, but the Android app should use explicit JSON APIs instead.

## Live updates

The Android app should not poll the entire library view constantly. For MVP:

- poll queue/session state on a short interval only while the queue screen is visible
- refresh album/library views on demand or after local actions

A better follow-on step is adding a dedicated live endpoint from `musicd`, likely:

- `SSE` for queue/session updates

That would help both the web UI and the Android app.

## Connection model

The app needs a simple server onboarding flow.

### Phase 1

- manual server entry
- optional auto-discovery later
- remember multiple servers locally

Suggested user input:

- display name, for example `Home NAS`
- base URL, for example `http://192.168.1.10:8787`

We should assume LAN-only access first. Authentication can wait unless you want remote access.

## Android milestones

### Phase 0: API contract cleanup

Before serious Android coding:

- stabilize JSON response shapes
- add mutation endpoints for queue and transport
- add a single session/status endpoint
- document the contract

### Phase 1: App skeleton

- new Android project
- Compose + navigation + theme
- server onboarding
- API client layer
- renderer/session repository
- bottom navigation shell

### Phase 2: Browse and play

- Home
- Library
- Album detail
- renderer picker
- play / queue actions

### Phase 3: Queue-first control

- queue screen
- transport controls
- queue editing
- polling or SSE-backed updates

### Phase 4: polish

- artwork transitions
- adaptive tablet layouts
- better empty/error states
- cached images and metadata

## Suggested repository shape

If we add the Android app to this repo, a clean starting layout would be:

- `apps/musicd`
- `apps/musicd-android`
- `crates/musicd-core`
- `crates/musicd-upnp`

That keeps the Android client close to the backend while making the boundary explicit.

## Recommended next step

The best next move is not immediately creating the Android app module.

It is:

1. define the Android-facing API contract
2. add the missing JSON mutation endpoints to `musicd`
3. then scaffold the Android app against that contract

That will save us from building the Android UI on top of browser-oriented redirect endpoints.

The first contract draft now lives in [docs/android-api-contract.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/android-api-contract.md).
