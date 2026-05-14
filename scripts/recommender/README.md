# Album recommender prototype

`index.ts` is a dependency-free recommendation CLI for `musicd` album seeds. It can:

1. build a collection profile
2. build a targeted external catalog
3. generate normalized album text
4. generate/cache embeddings from an OpenAI-compatible `/embeddings` endpoint
5. recommend owned and discovery albums
6. optionally rerank/explain recommendations with an OpenAI-compatible chat model
7. optionally emit a `musicd` recommendation import payload

The default collection input is `seed.json`. Use `bun` for the examples below; the active Node version in this workspace is too old to run this TypeScript file directly.

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
- `config.local-test.json` is a no-network smoke test that uses `test-external-catalog.json`.

The config controls:

- `collection`: seed JSON path
- `artifacts.profile`: collection profile output
- `artifacts.catalog`: external catalog output/input
- `artifacts.embeddings`: embedding index output/input
- `artifacts.recommendations`: recommendation output
- `catalog`: MusicBrainz/Last.fm catalog settings
- `embeddings`: embedding provider settings
- `recommendations`: seed albums and recommendation counts

Set `recommendations.format` to `"import"` when you want `artifacts.recommendations` to be directly uploadable to `musicd`'s import endpoint. In that mode, multi-seed runs are flattened into one top-level `recommendations` array.

Set `recommendations.recentDiscoveryCount` to reserve discovery slots for current/previous-year albums. The example config reserves two discovery slots. By default, catalog generation searches the current and previous calendar year; set `catalog.recentYears` only when you want to pin that window.

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

Build a targeted external catalog from the collection profile:

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
  --output scripts/recommender/embeddings.json
```

Generate embeddings with LM Studio or another OpenAI-compatible local server:

```sh
bun scripts/recommender/index.ts embed \
  --external-catalog scripts/recommender/external-catalog.json \
  --embedding-base-url http://localhost:1234/v1 \
  --embedding-model your-embedding-model \
  --output scripts/recommender/embeddings.json
```

Embedding indexes are schema-checked during recommendation. If album text changes, `recommend --embeddings` fails with a stale/missing embedding message instead of mixing vector spaces.

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

For a clean local testing slate, wipe uploaded recommendations first:

```sh
curl -sS -X DELETE http://localhost:8787/api/recommendations
```

If you are using `run`, make sure the config contains:

```json
{
  "recommendations": {
    "format": "import"
  }
}
```

## External Catalog Shape

The recommender accepts an array, `{ "albums": [...] }`, or `{ "seeds": [...] }`.

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

## Notes

- Local TF-IDF is still available as the fallback path.
- Embeddings are stored in JSON for now. Moving to pgvector later should mostly be a storage change.
- Discovery quality depends heavily on catalog quality. A small/noisy catalog will produce repetitive recommendations.
- `test-external-catalog.json` is intentionally tiny and exists only for smoke testing.
