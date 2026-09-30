# Authentication

`musicd` has two kinds of credentials:

- **Web logins.** A username and password, which give the browser a session cookie (`musicd_session`: `HttpOnly`, `SameSite=Strict`, 30-day sliding expiry). Web users can do everything, admin actions included.
- **API tokens.** Bearer tokens (`Authorization: Bearer mdt_...`) with a scope of `admin` or `client`. `client` covers everything the Android app and `musicdctl` need; `admin` also covers the admin-only routes below. The server stores only a SHA-256 hash of each token, so a token is shown once, when it's created.

## First start

When the database has no users, `musicd` creates `admin` with the password `password`. The first sign-in with it goes straight to `/account/password`, and every other page and API call (for that session) is refused until the password is changed. The new password must be at least 8 characters and can't be `password`.

Changing the password signs out every other browser session.

## Modes (`MUSICD_AUTH`)

| Mode | Admin routes | Everything else |
| --- | --- | --- |
| `off` | open | open |
| `optional` (default) | web login or `admin` token | open |
| `required` | web login or `admin` token | web login or any token |

An unrecognised value is treated as `required`.

In every mode other than `off`, a request carrying an unknown or revoked bearer token gets `401`, even on routes that would otherwise be open.

Unauthenticated browser page loads and form posts are redirected to `/login?next=...`. `/api/*`, `/mcp` and the web UI's own fetch endpoints get `401` with `WWW-Authenticate: Bearer` instead. A client token on an admin route gets `403`.

## Route classes

**Public** (never need credentials):

- `/health`, `/description.xml`, `/metrics`, `/assets/*`
- `/stream/*` and `/artwork/*`. UPnP renderers fetch these and can't send credentials. Signed URLs are planned.
- `/login`, `/logout`, `/account/password`. The password page checks for a session itself.

**Admin:**

- `POST /rescan`, `GET /rescan-progress` (this starts a scan)
- `POST /api/tidal/auth-url`, `POST /api/tidal/complete-auth`
- `POST /api/recommendations/import`, `DELETE /api/recommendations`
- `/api/auth/tokens*`

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

The web UI page for managing tokens hasn't been built yet. Until then, you can mint a token from the command line. Change the default password in a browser first, then run:

```bash
curl -sS -c /tmp/musicd.cookies -d "username=admin" --data-urlencode "password=$MUSICD_PASSWORD" \
  http://musicd.local:7878/login -o /dev/null
curl -sS -b /tmp/musicd.cookies -d "name=recommender&scope=admin" \
  http://musicd.local:7878/api/auth/tokens
rm /tmp/musicd.cookies
```

An existing admin token can also mint new ones (`-H "Authorization: Bearer $MUSICD_ADMIN_TOKEN"`).

## CSRF

Actions that change state are `POST`-only. That includes the web UI's older form routes (`/play`, `/play-album`, `/transport/*`, `/queue/*`, `/rescan`), which now return `405` for `GET`. `SameSite=Strict` keeps other sites from sending the session cookie. One side effect: when you follow a link to `musicd` from another site, that first page load arrives without the cookie, so it may show the sign-in page.

## Caveats

`musicd` serves plain HTTP, so passwords, cookies and tokens cross the LAN unencrypted. Put a TLS-terminating reverse proxy in front if that matters for your network.
