# Android Controller Plan

## Why this app

The Android app should be a native controller for `musicd`, optimized for phone use while keeping the door open for tablets and foldables. The main purpose is still browsing the library, managing the queue, and controlling playback on renderers, but that renderer set can now include the phone itself as an optional `android_local` target.

That means the Android app should treat `musicd` as the source of truth for:

- library data
- artwork
- queue state
- renderer discovery and selection
- transport/session state

An implementation plan for phone-as-renderer playback now lives in [docs/android-local-renderer-plan.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/android-local-renderer-plan.md).

## Current status

The Android app is now well past the “plan only” stage. The current implementation already includes:

- native server onboarding and renderer selection
- `Home`, `Library`, and `Queue`
- artist and album browsing
- grouped search across artists, albums, and tracks
- queue editing and transport controls
- SSE-backed live updates
- media notification and lock-screen/media-session controls
- optional `android_local` playback, with `musicd` still owning the queue

This document still matters as a direction-setting artifact, but the sections below should be read as architecture guidance and next-step planning, not as a description of a greenfield app.

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

Status:

- this MVP scope is effectively implemented

### Later

Good follow-on features after the first release:

- persistent recent servers
- favorites / recently played
- better queue editing gestures
- tablet and foldable two-pane layouts
- offline cache for lightweight metadata and artwork
- signed release packaging and store/distribution polish

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

The Android app now uses `SSE` for queue/session updates, which is the right long-lived model for the controller. The remaining discipline is:

- avoid refetching the full library on every connection or renderer change
- keep library views demand-driven
- let live updates focus on now-playing, queue, and renderer/session state

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

Status:

- complete

### Phase 1: App skeleton

- new Android project
- Compose + navigation + theme
- server onboarding
- API client layer
- renderer/session repository
- bottom navigation shell

Status:

- complete

### Phase 2: Browse and play

- Home
- Library
- Album detail
- renderer picker
- play / queue actions

Status:

- complete

### Phase 3: Queue-first control

- queue screen
- transport controls
- queue editing
- live queue and now-playing updates

Status:

- complete

### Phase 4: polish

- artwork transitions
- adaptive tablet layouts
- better empty/error states
- cached images and metadata

Status:

- in progress
- empty/error handling, mobile-first queue UX, and notification/media-session work are already in
- adaptive large-screen work and local caching are still open

### Phase 5: local playback and packaging

- `android_local` renderer support
- prebuffering and recovery polish for local playback
- signed release packaging
- release distribution workflow

Status:

- `android_local` playback is implemented
- prebuffering and recovery are in progress
- signed release packaging is still open

## Suggested repository shape

If we add the Android app to this repo, a clean starting layout would be:

- `apps/musicd`
- `apps/musicd-android`
- `crates/musicd-core`
- `crates/musicd-upnp`

That keeps the Android client close to the backend while making the boundary explicit.

## Recommended next step

The Android foundation is now strong enough that the best next work is polish rather than scaffolding:

1. local metadata/artwork caching for resilience and faster startup
2. tablet/foldable adaptation
3. signed release packaging and distribution

The current app-facing backend contract lives in [docs/android-api-contract.md](/Users/andrewrumble/Documents/Codex/2026-04-28-i-m-looking-to-make-an/docs/android-api-contract.md).
