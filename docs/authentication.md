# Authentication

`musicd` has two kinds of credentials:

- **Web logins.** A username and password, which give the browser a session cookie (`musicd_session`: `HttpOnly`, `SameSite=Strict`, 30-day sliding expiry). Web users can do everything, admin actions included.
- **API tokens.** Bearer tokens (`Authorization: Bearer mdt_...`) with a scope of `admin` or `client`. `client` covers everything the Android app and `musicdctl` need; `admin` also covers the admin-only routes below. The server stores only a SHA-256 hash of each token, so a token is shown once, when it's created. Devices get `client` tokens by [pairing](#pairing); you can also create and revoke tokens on the `/account` page.

## First start

When the database has no users, `musicd` creates `admin` with the password `password`. The first sign-in with it goes straight to `/account/password`, and every other page and API call (for that session) is refused until the password is changed. The new password must be at least 8 characters and can't be `password`.

Changing the password signs out every other browser session.

## Modes (`MUSICD_AUTH`)

| Mode | Admin routes | Stream and artwork | Everything else |
| --- | --- | --- | --- |
| `off` | open | open | open |
| `optional` (default) | web login or `admin` token | open | open |
| `required` | web login or `admin` token | signed URL, web login or any token | web login or any token |

An unrecognised value is treated as `required`.

In every mode other than `off`, a request carrying an unknown or revoked bearer token gets `401`, even on routes that would otherwise be open.

Unauthenticated browser page loads and form posts are redirected to `/login?next=...`. `/api/*`, `/mcp`, `/stream/*`, `/artwork/*` and the web UI's own fetch endpoints get `401` with `WWW-Authenticate: Bearer` instead. A client token on an admin route gets `403`.

## Route classes

**Public** (never need credentials):

- `/health`, `/description.xml`, `/metrics`, `/assets/*`
- `/login`, `/logout`, `/account/password`. The password page checks for a session itself.
- `POST /api/pair/start`, `POST /api/pair/poll`

**Media:** `/stream/*` and `/artwork/*`. These are treated like standard routes, except that a valid `sig` query parameter also grants access (see [Signed media URLs](#signed-media-urls)).

**Admin:**

- `POST /rescan`, `GET /rescan-progress` (this starts a scan)
- `POST /api/tidal/auth-url`, `POST /api/tidal/complete-auth`
- `POST /api/recommendations/import`, `DELETE /api/recommendations`
- `/api/auth/tokens*`
- `/account` and everything under `/account/` except `/account/password`

**Standard:** everything else.

## Endpoints

### Web

- `GET /login?next=<path>`: sign-in form
- `POST /login` with `username`, `password`, `next`: sets the session cookie and redirects. Five failures from one IP within 15 minutes lock that IP out for the rest of the window (`429`).
- `POST /logout`: ends the session
- `GET /account/password`, `POST /account/password` with `current_password`, `new_password`, `confirm_password`, `next`

### Tokens (admin)

- `GET /api/auth/tokens` lists tokens, newest first, with revoked ones last:

  ```json
  {"ok":true,"tokens":[{"id":"5ecb7e441f704652","name":"phone","scope":"client","created_unix":1790753830,"last_used_unix":1790753830,"revoked_unix":null}]}
  ```

- `POST /api/auth/tokens` with `name` (1-100 characters) and `scope` (`admin` or `client`) returns `201`. The response includes the secret, and this is the only time it's shown:

  ```json
  {"ok":true,"token":{"id":"5ecb7e441f704652","name":"phone","scope":"client","created_unix":1790753830,"last_used_unix":null,"revoked_unix":null,"token":"mdt_..."}}
  ```

- `POST /api/auth/tokens/revoke` with `id` returns `404` if there's no active token with that id.

In the browser, the `/account` page does the same: it lists tokens, creates them (showing the secret once) and revokes them. Your username in the app bar links to it. An admin token can also call these endpoints (`-H "Authorization: Bearer $MUSICD_ADMIN_TOKEN"`).

### Pairing

Pairing lets a device get a `client` token without anyone typing a token:

1. The device calls `POST /api/pair/start` with `name` (1-100 characters, shown to the admin and used as the token's name). The response is `201`:

   ```json
   {"ok":true,"pairing_id":"<64 hex chars>","code":"FFSY-F5VK","expires_in":600,"poll_interval":2}
   ```

   The device shows `code` and keeps `pairing_id` secret.
2. Someone signed in enters the code on `/account`. Codes are case-insensitive and the dash is optional. The page shows the device's name and IP address, with **Approve** and **Deny** buttons.
3. The device polls `POST /api/pair/poll` with `pairing_id` every `poll_interval` seconds:
   - `{"ok":true,"status":"pending"}`
   - `{"ok":true,"status":"approved","token":"mdt_..."}` is returned once; the request is then forgotten
   - `{"ok":true,"status":"denied"}`
   - `404` once the request is unknown, has expired (after 10 minutes) or has already been answered

Pairing requests are held in memory, so a restart cancels any that are pending. At most 32 can be pending at once; beyond that, `start` returns `429`.

### Android app

When a server answers `401`, the connection screen shows **Pair this phone**. It requests a code, shows it together with the server's `/account` URL, and polls until the request is approved, denied or expires, then reconnects. The token is stored per server origin in a file under the app's `noBackupFilesDir`, so it isn't included in cloud backups. It's added only to requests for that origin: API calls, the event stream, Coil artwork and ExoPlayer streams. **Forget pairing** in the server sheet deletes the stored token.

### `musicdctl`

```bash
musicdctl --server http://musicd.local:7878 pair [--name "Office laptop"]
```

This prints a code and waits until it's approved or denied. The token is saved per server in `~/.config/musicd/cli.toml`, which is written with mode `600`, and every request sends it after that. `MUSICD_TOKEN` overrides the saved token. `musicdctl unpair` forgets the saved token; revoke it on `/account` to disable it on the server too. When the server answers `401`, the CLI suggests running `musicdctl pair`.

Local playback in the CLI (`cli-local://` renderers) uses the stream URL the server reports. In `required` mode that URL is signed, so external players such as `mpv` don't need the token.

## Signed media URLs

UPnP renderers fetch stream and artwork URLs themselves and can't send credentials. In `required` mode, the URLs the server gives renderers (and reports back in session and queue data) carry `?sig=<hex>`: the first 128 bits of an HMAC-SHA256 of the URL path. Signatures don't expire. They have to stay stable because the queue tracker compares the URL a renderer reports with the one it generated. The key is created on first start and stored in the `app_state` table as `url_signing_key`. Delete that row and restart to rotate it, which invalidates every URL handed out so far.

In `optional` and `off` mode, stream and artwork URLs are open and unsigned. Web pages that embed artwork rely on the session cookie instead of signatures.

## CSRF

Actions that change state are `POST`-only. That includes the web UI's older form routes (`/play`, `/play-album`, `/transport/*`, `/queue/*`, `/rescan`), which now return `405` for `GET`. `SameSite=Strict` keeps other sites from sending the session cookie. One side effect: when you follow a link to `musicd` from another site, that first page load arrives without the cookie, so it may show the sign-in page.

## Caveats

`musicd` serves plain HTTP, so passwords, cookies and tokens cross the LAN unencrypted. Put a TLS-terminating reverse proxy in front if that matters for your network.
