# Album recommender prototype

`index.ts` is a dependency-free recommendation CLI for `musicd` album seeds. It can:

1. build a collection profile
2. build a targeted external catalog
3. generate normalized album text
4. generate/cache embeddings from an OpenAI-compatible `/embeddings` endpoint
5. recommend owned and discovery albums
6. optionally rerank/explain recommendations with an OpenAI-compatible chat model
7. optionally emit a `musicd` recommendation import payload

The default collection input is a local operator-provided `seed.json`. It is intentionally ignored by git, along with generated catalogs, embedding indexes, recommendation payloads, and API caches. Use `bun` for the examples below; the active Node version in this workspace is too old to run this TypeScript file directly.

See `DATA_LICENSES.md` for the recommender data boundary and third-party dataset notes.

## Quick Start

Preview the full run:

```sh
bun scripts/recommender/index.ts run \
  --config scripts/recommender/config.example.json \
  --dry-run
```

Run the full pipeline with OpenAI embeddings:

```sh
OPENAI_API_KEY=... bun scripts/recommender/index.ts run \
  --config scripts/recommender/config.example.json
```

Existing catalog and embedding artifacts are reused by default. Rebuild them with:

```sh
OPENAI_API_KEY=... bun scripts/recommender/index.ts run \
  --config scripts/recommender/config.example.json \
  --force
```

## Config Files

- `config.example.json` is the normal end-to-end pipeline.
- `config.musicbrainz-dump.example.json` is the end-to-end pipeline using a local MusicBrainz dump candidate catalog.
- `config.local-test.json` is a no-network smoke test that uses fictional fixture data from `fixtures/sample-collection.json` and `test-external-catalog.json`.

The config controls:

- `collection`: seed JSON path
- `database`: optional recommender SQLite database path
- `artifacts.profile`: collection profile output
- `artifacts.catalog`: external catalog output/input
- `artifacts.embeddings`: optional flat-file embedding index output/input
- `artifacts.recommendations`: recommendation output
- `catalog`: MusicBrainz/Last.fm catalog settings
- `embeddings`: embedding provider settings
- `recommendations`: seed albums and recommendation counts
- `tidal`: optional TIDAL album-link enrichment for recommendation outputs

The recommender treats the external catalog as admin-provided local data. MusicBrainz and Last.fm tooling are optional ways to build a compatible catalog, not bundled data dependencies. Operators can replace those inputs with any compatible catalog source.

When `database` is set, embeddings are stored incrementally in SQLite and reused by `album_id`, embedding model, provider base URL, text schema, and text hash. The example configs use `recommender.sqlite` and request/store 1536-dimensional vectors. If a provider returns a larger vector, the embed step truncates to the requested dimensions before normalization and storage.

Set `recommendations.format` to `"import"` when you want `artifacts.recommendations` to be directly uploadable to `musicd`'s import endpoint. In that mode, multi-seed runs are flattened into one top-level `recommendations` array.

Set `recommendations.recentDiscoveryCount` to reserve discovery slots for current/previous-year albums. The example config reserves two discovery slots. By default, catalog generation searches the current and previous calendar year; set `catalog.recentYears` only when you want to pin that window.

Set `tidal.enabled` to `true` to resolve recommendations to TIDAL album links and write high-confidence matches into `tidal_url`. By default, those matches also replace `external_url` so older clients can use them; set `tidal.overwriteExternalUrl` to `false` to preserve generic links. This uses TIDAL client credentials from `TIDAL_CLIENT_ID` and `TIDAL_CLIENT_SECRET`; set `TIDAL_COUNTRY_CODE` or `tidal.countryCode` for region-specific catalog matching.

Run the local smoke test:

```sh
bun scripts/recommender/index.ts run \
  --config scripts/recommender/config.local-test.json
```

## Individual Commands

### Profile

```sh
bun scripts/recommender/index.ts profile \
  --collection scripts/recommender/seed.json \
  --output scripts/recommender/profile.json
```

### Catalog

Build a targeted external catalog from the collection profile using the MusicBrainz web API and optional Last.fm enrichment:

```sh
bun scripts/recommender/index.ts catalog --dry-run

bun scripts/recommender/index.ts catalog \
  --limit 250 \
  --output scripts/recommender/external-catalog.json \
  --musicbrainz-user-agent "musicd-recommender/0.1 (you@example.com)"
```

The catalog command uses MusicBrainz release groups as the main identity source, filters obvious non-album material, and removes albums already present in the collection. It respects MusicBrainz's one-request-per-second guidance by default with `--musicbrainz-delay-ms 1100`.

The catalog step also issues recent-release queries for the top collection genres using MusicBrainz `firstreleasedate` filters. It defaults to the current and previous calendar year. Configure fixed years with `catalog.recentYears` and per-query breadth with `catalog.recentPerQuery`.

Last.fm expansion is optional:

```sh
LASTFM_API_KEY=... bun scripts/recommender/index.ts catalog \
  --limit 500 \
  --output scripts/recommender/external-catalog.json
```

Last.fm adds tag charts, similar artists, and artist top albums.

For larger catalogs, prefer the offline MusicBrainz dump builder. It converts locally downloaded MusicBrainz dump files into a JSONL candidate catalog:

```sh
bun scripts/recommender/index.ts musicbrainz-dump \
  --core scripts/recommender/datasets/mbdump.tar.bz2 \
  --derived scripts/recommender/datasets/mbdump-derived.tar.bz2 \
  --output scripts/recommender/mb-candidate-catalog.jsonl
```

The builder reads MusicBrainz release groups as the album identity backbone, filters to album release groups, excludes common non-discovery secondary types such as compilations/live/remix/soundtrack material, joins first-release dates from `release_group_meta` when that table is present, and optionally joins derived release-group tags. Use `--min-tag-count` to control how noisy derived tags can be, and `--max-rows` for a smaller trial run.

Then sample that local candidate catalog into the recommender's normal external catalog format:

```sh
bun scripts/recommender/index.ts catalog \
  --collection scripts/recommender/seed.json \
  --candidate-catalog scripts/recommender/mb-candidate-catalog.jsonl \
  --limit 50000 \
  --output scripts/recommender/external-catalog.json
```

For an end-to-end run, set `catalog.candidateCatalog` in config or start from `config.musicbrainz-dump.example.json`. Last.fm enrichment remains optional; when an API key is configured, the catalog step can still add Last.fm tag-chart candidates around the collection profile. Do not commit dump files, generated candidate catalogs, or generated external catalogs.

### Embeddings

Preview embedding inputs and request count:

```sh
bun scripts/recommender/index.ts embed \
  --external-catalog scripts/recommender/external-catalog.json \
  --dry-run
```

Generate embeddings with OpenAI:

```sh
OPENAI_API_KEY=... bun scripts/recommender/index.ts embed \
  --external-catalog scripts/recommender/external-catalog.json \
  --embedding-model text-embedding-3-small \
  --embedding-dimensions 1536 \
  --output scripts/recommender/embeddings.json
```

Generate embeddings into the recommender SQLite database with LM Studio or another OpenAI-compatible local server:

```sh
bun scripts/recommender/index.ts embed \
  --external-catalog scripts/recommender/external-catalog.json \
  --database scripts/recommender/recommender.sqlite \
  --embedding-base-url http://127.0.0.1:1234/v1 \
  --embedding-model your-embedding-model \
  --embedding-dimensions 1536
```

Embedding databases are resumable: rerunning `embed --database ...` skips rows whose text hash still matches and only requests missing/stale vectors. Use `--force` to recompute everything. Flat embedding indexes are still supported; they are written through a temporary file and renamed into place after the stream closes, so an interrupted rebuild should not leave a partial `embeddings.json` at the final path. Embeddings are schema-checked during recommendation; if album text changes, recommendation fails with a stale/missing embedding message instead of mixing vector spaces.

### Recommend

Recommend with local TF-IDF vectors:

```sh
bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json
```

Recommend with generated embeddings:

```sh
bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json \
  --embeddings scripts/recommender/embeddings.json
```

Recommend with SQLite-stored embeddings:

```sh
bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json \
  --database scripts/recommender/recommender.sqlite \
  --embedding-base-url http://127.0.0.1:1234/v1 \
  --embedding-model your-embedding-model
```

Use a MusicBrainz/collection album id instead of a seed string:

```sh
bun scripts/recommender/index.ts recommend \
  --seed-album-id c848a9ad07ec189e \
  --external-catalog scripts/recommender/external-catalog.json \
  --embeddings scripts/recommender/embeddings.json
```

### LM Studio Reranking

If LM Studio is serving an OpenAI-compatible chat model:

```sh
bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json \
  --embeddings scripts/recommender/embeddings.json \
  --lm-studio-url http://localhost:1234/v1 \
  --rerank-model your-local-chat-model
```

The LLM receives only the seed album, collection summary, and retrieved candidates. It does not search the catalog.

### Import Payload

Produce a payload for `musicd`'s `/api/recommendations/import` endpoint:

```sh
bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json \
  --embeddings scripts/recommender/embeddings.json \
  --format import \
  --output scripts/recommender/recommendations-import.json
```

Upload it:

```sh
curl -sS -X POST http://localhost:8787/api/recommendations/import \
  -H 'content-type: application/json' \
  --data-binary @scripts/recommender/recommendations-import.json
```

Add TIDAL deep links during import payload generation:

```sh
TIDAL_CLIENT_ID=... TIDAL_CLIENT_SECRET=... bun scripts/recommender/index.ts recommend \
  --seed "AIR - Moon Safari" \
  --external-catalog scripts/recommender/external-catalog.json \
  --embeddings scripts/recommender/embeddings.json \
  --format import \
  --tidal \
  --tidal-country-code US \
  --output scripts/recommender/recommendations-import.json
```

For a clean local testing slate, wipe uploaded recommendations first:

```sh
curl -sS -X DELETE http://localhost:8787/api/recommendations
```

If you are using `run`, make sure the config contains:

```json
{
  "recommendations": {
    "format": "import"
  },
  "tidal": {
    "enabled": true,
    "countryCode": "US"
  }
}
```

## External Catalog Shape

The recommender accepts an array, `{ "albums": [...] }`, or `{ "seeds": [...] }`.

The external catalog is deliberately source-agnostic. MusicBrainz, Last.fm, ListenBrainz, Discogs, private catalogs, and hand-curated JSON can all feed the recommender if they produce this shape. Unknown top-level metadata is ignored, so local builders can include their own provenance fields.

Useful album fields:

- `artist`
- `title`
- `release_date` or `year`
- `genres`
- `tags`
- `moods`
- `style_descriptors`
- `description`
- `musicbrainz_release_id`
- `musicbrainz_release_group_id`
- `artwork_url`
- `external_url`
- `tidal_url`

Recommended top-level provenance fields for generated local catalogs:

- `catalog_schema_version`
- `generated_at`
- `sources`
- `data_license_notes`

## Notes

- Local TF-IDF is still available as the fallback path.
- Embeddings can be stored incrementally in a separate recommender SQLite database. Flat JSON embedding indexes remain available for small/debug runs. Moving to pgvector later should mostly be a storage/search implementation change.
- Discovery quality depends heavily on catalog quality. A small/noisy catalog will produce repetitive recommendations.
- `test-external-catalog.json` is intentionally tiny and exists only for smoke testing.
- Third-party dumps, API caches, generated catalogs, generated embeddings, generated recommendations, and local collection exports are local operator artifacts and should stay out of git.
- Large runs are memory-sensitive. The `run` command prepares recommendation data once and reuses it across seeds, but embedding model dimension and `catalog.limit` still dominate memory. If a full run strains memory, lower `catalog.limit`, use a smaller embedding model, reduce `embeddings.batchSize`, or split the job into catalog/embed/recommend steps.
- Long-running commands write progress logs to stderr with a `[recommender:<stage>]` prefix, so stdout remains usable for JSON output. Embedding logs include provider response size/hash, parsed row counts, vector dimensions, SQLite reuse/store counts, flat-file write progress, final file byte counts, and parse diagnostics for malformed embedding indexes. Recommendation logs include timing for album loading, SQLite vector loading, profile building, owned/discovery retrieval, selection, serialization, reranking, and TIDAL enrichment.
