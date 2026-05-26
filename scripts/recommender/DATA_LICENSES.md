# Recommender Data Boundary

The recommender code can consume external catalog data, but this repository should not distribute third-party music metadata dumps, generated catalogs, generated embeddings, API caches, or local library exports.

Operators are responsible for obtaining datasets, API keys, and generated artifacts under terms that fit their use case. A compatible catalog can come from MusicBrainz, Last.fm enrichment, ListenBrainz, Discogs, a private catalog, or a hand-curated JSON file as long as it matches the external catalog shape described in `README.md`.

## Repository Policy

- Keep third-party dataset files out of git.
- Keep generated external catalogs, embedding indexes, and recommendation payloads out of git.
- Keep local collection exports such as `seed.json` out of git.
- Keep API keys in local environment/config files only.
- Document optional dataset builders as tooling; do not bundle their outputs.

## MusicBrainz

MusicBrainz splits database data into two broad license groups:

- Core data is CC0/public-domain-style data.
- Supplementary data is CC BY-NC-SA 3.0 and includes user annotations, tags, ratings, derived statistics, search indexes, edit history, and non-personal user data.

MusicBrainz genre associations are part of the user tag data, so generated catalogs that include MusicBrainz tags/genres may be derived from supplementary data. Treat those artifacts as local operator data unless you have confirmed that redistribution fits your use case.

The `musicbrainz-dump` command is tooling only. It expects operator-provided core and optional derived dump files, and it writes a local JSONL candidate catalog that should stay out of git.

References:

- https://musicbrainz.org/doc/About/Data_License
- https://musicbrainz.org/doc/MusicBrainz_Database/Download

## Last.fm

Last.fm enrichment is optional and requires an operator-provided API key. Do not commit Last.fm keys, API responses, enrichment caches, or generated artifacts that contain Last.fm-derived data.

Reference:

- https://www.last.fm/api

## Open Source Code vs. Data

The project source code license does not grant rights to third-party datasets consumed by local operators. Generated catalogs and embeddings may carry obligations from the datasets used to produce them.
