# MVP Plan

## Phase 1: Prove renderer playback

Goal:

- play one known audio file to one CXN V2 over the local network

Tasks:

- implement SSDP discovery
- load the device description XML
- find the `AVTransport` control endpoint
- expose one local HTTP audio URL
- send `SetAVTransportURI`
- send `Play`

Exit criteria:

- the CXN V2 starts playback from a URL served by this app
- current scaffold status: implemented locally, needs validation against a real renderer on the LAN

## Phase 2: Index the NAS library

Goal:

- replace the single test track with a browsable library

Tasks:

- scan a mounted NAS directory
- extract basic tags
- persist tracks, albums, artists, and artwork references in SQLite
- detect changed and removed files

Exit criteria:

- a library rescan reflects the current NAS contents

## Phase 3: Add a controller UI

Goal:

- control playback without shell commands

Tasks:

- build library browse and search views
- add renderer picker
- add queue and transport controls
- show current playback state

Exit criteria:

- a browser can browse, select, and play music to the CXN V2

## Phase 4: Improve compatibility

Goal:

- support more renderers and trickier file formats

Tasks:

- add transcoding where needed
- add AirPlay or Chromecast adapters
- improve metadata and artwork handling
- add multiple zones

Exit criteria:

- the app supports more than one renderer family without changing the core library model
