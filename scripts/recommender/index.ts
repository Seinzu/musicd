#!/usr/bin/env node

import { createReadStream, createWriteStream, existsSync } from "node:fs";
import { readFile, rename, stat, unlink, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path, { basename } from "node:path";
import { fileURLToPath } from "node:url";

const TEXT_SCHEMA_VERSION = "album_profile_v1";
const LOCAL_EMBEDDING_MODEL = "local-tfidf-v1";
const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));

type AlbumInput = {
  album_id?: string;
  id?: string;
  artist?: string;
  title?: string;
  release_date?: string | null;
  year?: number | string | null;
  genres?: string[];
  tags?: string[];
  moods?: string[];
  style_descriptors?: string[];
  description?: string;
  source?: string;
  musicbrainz_release_id?: string | null;
  musicbrainz_release_group_id?: string | null;
  artwork_url?: string | null;
  external_url?: string | null;
  tidal_url?: string | null;
  track_count?: number;
};

type AlbumRecord = {
  albumId: string;
  artist: string;
  title: string;
  releaseDate: string | null;
  year: number | null;
  decade: string | null;
  genres: string[];
  tags: string[];
  descriptors: string[];
  description: string | null;
  owned: boolean;
  source: string;
  musicbrainzReleaseId: string | null;
  musicbrainzReleaseGroupId: string | null;
  artworkUrl: string | null;
  externalUrl: string | null;
  tidalUrl: string | null;
  trackCount: number | null;
  normalizedText: string;
  tokens: string[];
  vector: Map<number, number>;
  denseVector?: Float32Array;
};

type CollectionProfile = {
  text_schema_version: string;
  embedding_model: string;
  album_count: number;
  top_genres: [string, number][];
  top_artists: [string, number][];
  era_distribution: [string, number][];
  style_descriptors: [string, number][];
  popularity_obscurity_tendency: string;
  representative_albums: {
    album_id: string;
    artist: string;
    title: string;
    year: number | null;
    genres: string[];
  }[];
};

type Candidate = {
  album: AlbumRecord;
  score: number;
  embeddingSimilarity: number;
  genreAffinity: number;
  eraCompatibility: number;
  diversityBonus: number;
  artistPenalty: number;
  rationale: string;
};

type RetrievalScoringContext = {
  profileGenreSet: Set<string>;
  seedGenreSignals: Set<string>;
  seedTagSignals: string[];
  seedDescriptorSet: Set<string>;
};

type RecommendOptions = {
  collectionPath: string;
  externalPath?: string;
  embeddingsPath?: string;
  databasePath?: string;
  embeddingModel?: string;
  embeddingBaseUrl?: string;
  embeddingDimensions?: number;
  seed?: string;
  seedAlbumId?: string;
  ownedCount: number;
  discoveryCount: number;
  recentDiscoveryCount: number;
  poolSize: number;
  output?: string;
  format: "recommendations" | "import";
  lmStudioUrl?: string;
  rerankModel?: string;
  tidal?: TidalOptions;
};

type RecommendationContext = {
  collection: AlbumRecord[];
  external: AlbumRecord[];
  embeddingModel: string;
  profile: CollectionProfile;
};

type EmbedOptions = {
  collectionPath: string;
  externalPath?: string;
  output?: string;
  databasePath?: string;
  baseUrl: string;
  apiKey?: string;
  model: string;
  batchSize: number;
  dimensions?: number;
  dryRun: boolean;
  force: boolean;
};

type EmbeddingBatchLogContext = {
  batch: number;
  batches: number;
  start: number;
  count: number;
};

type AlbumEmbedding = {
  album_id: string;
  artist: string;
  title: string;
  owned: boolean;
  text_hash: string;
  dimensions: number;
  embedding: number[];
  musicbrainz_release_id?: string | null;
  musicbrainz_release_group_id?: string | null;
};

type EmbeddingIndex = {
  embedding_schema_version: "album_embeddings_v1";
  text_schema_version: string;
  embedding_model: string;
  embedding_base_url: string;
  generated_at: string;
  album_count: number;
  embeddings: AlbumEmbedding[];
};

type EmbeddingBuildSummary = {
  embedding_schema_version: "album_embeddings_v1";
  text_schema_version: string;
  embedding_model: string;
  embedding_base_url: string;
  generated_at: string;
  album_count: number;
  database_path: string;
  requested_dimensions: number | null;
  stored_dimensions: number | null;
  reused_embeddings: number;
  stale_embeddings: number;
  embedded_embeddings: number;
};

type CatalogOptions = {
  collectionPath: string;
  output?: string;
  candidateCatalog?: string;
  limit: number;
  topGenres: number;
  topArtists: number;
  perQuery: number;
  recentYears: number[];
  recentPerQuery: number;
  dryRun: boolean;
  musicBrainzUserAgent: string;
  musicBrainzDelayMs: number;
  lastfmApiKey?: string;
  lastfmDelayMs: number;
};

type MusicBrainzDumpOptions = {
  corePath: string;
  derivedPath?: string;
  output?: string;
  includeDerivedTags: boolean;
  minTagCount: number;
  maxRows: number;
};

type CatalogPlan = {
  same_genre_queries: CatalogQuery[];
  adjacent_genre_queries: CatalogQuery[];
  exploratory_queries: CatalogQuery[];
  recent_queries: CatalogQuery[];
  lastfm_artist_queries: string[];
};

type CatalogQuery = {
  bucket: "same_genre" | "adjacent" | "exploratory" | "recent";
  tag: string;
  query: string;
  target_count: number;
};

type ExternalCatalogAlbum = AlbumInput & {
  catalog_id: string;
  catalog_sources: string[];
  source_evidence: string[];
  popularity_score?: number;
};

type CatalogCandidate = {
  album: ExternalCatalogAlbum;
  bucket: CatalogQuery["bucket"] | "lastfm_artist" | "lastfm_tag";
  score: number;
};

type RateLimitState = {
  lastRequestAt: number;
};

type TidalOptions = {
  enabled: boolean;
  clientId?: string;
  clientSecret?: string;
  countryCode: string;
  minConfidence: number;
  maxCandidates: number;
  delayMs: number;
  overwriteExternalUrl: boolean;
  apiBaseUrl: string;
  tokenUrl: string;
};

type TidalToken = {
  accessToken: string;
  expiresAt: number;
};

type TidalAlbumMatch = {
  id: string;
  url: string;
  confidence: number;
  title: string;
  artist: string | null;
  releaseDate: string | null;
  albumType: string | null;
  reason: string;
};

type RunConfig = {
  collection?: string;
  database?: string | {
    enabled?: boolean;
    path?: string;
  };
  artifacts?: {
    profile?: string;
    catalog?: string;
    embeddings?: string;
    recommendations?: string;
    importPayload?: string;
  };
  catalog?: Partial<
    Pick<
      CatalogOptions,
      "limit" | "topGenres" | "topArtists" | "perQuery" | "recentYears" | "recentPerQuery" | "musicBrainzDelayMs" | "lastfmDelayMs"
    >
  > & {
    enabled?: boolean;
    reuseExisting?: boolean;
    dryRun?: boolean;
    candidateCatalog?: string;
    musicBrainzUserAgent?: string;
    lastfmApiKey?: string;
  };
  embeddings?: Partial<Pick<EmbedOptions, "baseUrl" | "apiKey" | "model" | "batchSize" | "dimensions">> & {
    enabled?: boolean;
    reuseExisting?: boolean;
    dryRun?: boolean;
  };
  recommendations?: {
    enabled?: boolean;
    seeds?: string[];
    seedAlbumIds?: string[];
    ownedCount?: number;
    discoveryCount?: number;
    recentDiscoveryCount?: number;
    poolSize?: number;
    format?: RecommendOptions["format"];
    lmStudioUrl?: string;
    rerankModel?: string;
  };
  tidal?: {
    enabled?: boolean;
    clientId?: string;
    clientSecret?: string;
    countryCode?: string;
    minConfidence?: number;
    maxCandidates?: number;
    delayMs?: number;
    overwriteExternalUrl?: boolean;
    apiBaseUrl?: string;
    tokenUrl?: string;
  };
};

type RunOptions = {
  configPath: string;
  dryRun: boolean;
  force: boolean;
};

const STOP_WORDS = new Set([
  "a",
  "an",
  "and",
  "are",
  "as",
  "at",
  "be",
  "by",
  "for",
  "from",
  "in",
  "into",
  "is",
  "it",
  "of",
  "on",
  "or",
  "the",
  "to",
  "with",
]);

const GENRE_ALIASES = new Map<string, string>([
  ["hip-hop", "Hip-Hop"],
  ["hip hop", "Hip-Hop"],
  ["hip hop/rap", "Hip-Hop"],
  ["hip-hop [amerikanisch]", "Hip-Hop"],
  ["rap", "Hip-Hop"],
  ["electronica", "Electronic"],
  ["electronica/dance", "Electronic"],
  ["ambient", "Ambient"],
  ["alternative & punk", "Alternative"],
  ["general alternative", "Alternative"],
  ["indie rock", "Indie Rock"],
]);

const STYLE_LEXICON: Record<string, string[]> = {
  ambient: ["atmospheric", "spacious", "textural", "immersive"],
  alternative: ["guitar-driven", "left-field", "college-radio", "restless"],
  electronic: ["synthetic", "rhythmic", "textural", "club-adjacent"],
  folk: ["acoustic", "intimate", "warm", "storytelling"],
  funk: ["groove-led", "syncopated", "bass-forward", "danceable"],
  hiphop: ["sample-based", "rhythmic", "lyrical", "beat-driven"],
  indie: ["off-kilter", "melodic", "guitar-driven", "understated"],
  jazz: ["improvisational", "harmonic", "organic", "instrumental"],
  metal: ["heavy", "distorted", "intense", "riff-driven"],
  pop: ["melodic", "hooky", "polished", "song-focused"],
  punk: ["raw", "urgent", "stripped-down", "abrasive"],
  rb: ["soulful", "vocal-forward", "groove-led", "smooth"],
  rock: ["guitar-driven", "band-led", "energetic", "amplified"],
  soul: ["warm", "vocal-forward", "groove-led", "expressive"],
};

const MUSICBRAINZ_API_ROOT = "https://musicbrainz.org/ws/2";
const LASTFM_API_ROOT = "https://ws.audioscrobbler.com/2.0/";
const TIDAL_API_ROOT = "https://openapi.tidal.com/v2";
const TIDAL_TOKEN_URL = "https://auth.tidal.com/v1/oauth2/token";
const RETRIEVAL_SHORTLIST_THRESHOLD = 5_000;
const RETRIEVAL_SHORTLIST_DIMENSIONS = 256;
const RETRIEVAL_SHORTLIST_MULTIPLIER = 40;
const RETRIEVAL_SHORTLIST_MIN = 2_000;
const DEFAULT_USER_AGENT = "musicd-recommender/0.1 (local catalog builder)";
const ARCHIVE_ENTRIES_CACHE = new Map<string, Promise<string[]>>();
const DIMENSION_TRUNCATION_LOGGED = new Set<string>();
const TIDAL_MATCH_CACHE = new Map<string, Promise<TidalAlbumMatch | null>>();

function progress(step: string, message: string, detail?: Record<string, unknown>) {
  const suffix = detail ? ` ${JSON.stringify(detail)}` : "";
  console.error(`[recommender:${step}] ${new Date().toISOString()} ${message}${suffix}`);
}

function progressError(step: string, message: string, error: unknown, detail?: Record<string, unknown>) {
  const errorDetail =
    error instanceof Error
      ? {
          error_name: error.name,
          error_message: error.message,
          error_stack: error.stack?.split("\n").slice(0, 5).join("\n"),
        }
      : { error_message: String(error) };
  progress(step, message, { ...(detail ?? {}), ...errorDetail });
}

function timerStart(): number {
  return performance.now();
}

function timerMs(start: number): number {
  return round(performance.now() - start);
}

const ADJACENT_TAGS = new Map<string, string[]>([
  ["Ambient", ["downtempo", "idm", "drone", "minimalism"]],
  ["Alternative", ["indie rock", "post-punk", "college rock", "noise pop"]],
  ["Electronic", ["idm", "techno", "downtempo", "ambient"]],
  ["Hip-Hop", ["alternative hip-hop", "jazz rap", "underground hip-hop", "trip-hop"]],
  ["Indie", ["indie rock", "lo-fi", "noise pop", "twee pop"]],
  ["Indie Rock", ["college rock", "post-rock", "lo-fi", "noise pop"]],
  ["Jazz", ["jazz fusion", "soul jazz", "spiritual jazz", "free jazz"]],
  ["Pop", ["synthpop", "art pop", "power pop", "dream pop"]],
  ["Rock", ["post-punk", "krautrock", "psychedelic rock", "art rock"]],
]);

const EXPLORATORY_TAGS = [
  "dub",
  "krautrock",
  "post-rock",
  "soul jazz",
  "trip-hop",
  "dream pop",
  "minimal wave",
  "spiritual jazz",
  "library music",
  "ambient techno",
];

function usage(): never {
  console.log(`Usage:
  node scripts/recommender/index.ts profile [--collection scripts/recommender/seed.json] [--output profile.json]
  node scripts/recommender/index.ts musicbrainz-dump --core mbdump.tar.bz2 [--derived mbdump-derived.tar.bz2] [--output mb-candidate-catalog.jsonl]
  node scripts/recommender/index.ts catalog [--collection seed.json] [--output external-catalog.json]
  node scripts/recommender/index.ts embed [--collection seed.json] [--external-catalog albums.json] [--output embeddings.json]
  node scripts/recommender/index.ts recommend --seed-album-id <id> [--collection seed.json] [--external-catalog albums.json]
  node scripts/recommender/index.ts recommend --seed "Artist - Album" [--owned-count 5] [--discovery-count 5]
  node scripts/recommender/index.ts run --config scripts/recommender/config.example.json

Options:
  --config <path>             End-to-end run config JSON.
  --force                     Rebuild catalog/embeddings even if configured artifacts exist.
  --collection <path>         Collection JSON with { "seeds": [...] } or an album array.
  --embedding-base-url <url>  OpenAI-compatible API base URL. Default: OPENAI_BASE_URL or https://api.openai.com/v1.
  --embedding-api-key <key>   Embedding API key. Default: OPENAI_API_KEY.
  --embedding-model <model>   Embedding model. Default: text-embedding-3-small.
  --embedding-batch-size <n>  Embedding inputs per request. Default: 64.
  --embedding-dimensions <n>  Optional dimensions parameter for providers/models that support it.
  --embeddings <path>         Use a generated embedding index for recommendation similarity.
  --database <path>           Use a recommender SQLite database for embedding storage/retrieval.
  --core <path>               MusicBrainz core dump archive/directory for musicbrainz-dump.
  --derived <path>            Optional MusicBrainz derived dump archive/directory for musicbrainz-dump tags.
  --candidate-catalog <path>  Local JSON/JSONL candidate catalog used by catalog instead of web API search.
  --min-tag-count <n>         Minimum MusicBrainz tag count when reading derived dump tags. Default: 2.
  --max-rows <n>              Maximum rows for musicbrainz-dump output. Default: unlimited.
  --limit <n>                 External catalog album target for catalog. Default: 250.
  --recent-per-query <n>      Recent MusicBrainz albums per top-genre query. Default: 10.
  --dry-run                   Print catalog query plan without calling external APIs.
  --lastfm-api-key <key>      Optional Last.fm API key. Defaults to LASTFM_API_KEY.
  --musicbrainz-user-agent    MusicBrainz User-Agent. Include contact info for real use.
  --external-catalog <path>   Optional unowned catalogue JSON with { "albums": [...] } or an album array.
  --seed <text>               Seed album as "artist - title" or a title/artist search string.
  --seed-album-id <id>        Seed album id from the collection JSON.
  --owned-count <n>           Owned recommendations to return. Default: 6.
  --discovery-count <n>       Discovery recommendations to return. Default: 6.
  --recent-discovery-count <n> Discovery slots reserved for current/previous-year albums. Default: 2.
  --pool-size <n>             Retrieval pool before diversification. Default: 60.
  --format <name>             recommendations or import. Default: recommendations.
  --lm-studio-url <url>       Optional OpenAI-compatible base URL, e.g. http://localhost:1234/v1.
  --rerank-model <model>      Optional LM Studio chat model for reranking/explanations.
  --tidal                     Resolve recommendations to TIDAL album links. Requires TIDAL_CLIENT_ID/TIDAL_CLIENT_SECRET or flags below.
  --tidal-client-id <id>      TIDAL developer app client id. Default: TIDAL_CLIENT_ID.
  --tidal-client-secret <key> TIDAL developer app client secret. Default: TIDAL_CLIENT_SECRET.
  --tidal-country-code <cc>   TIDAL catalog country code. Default: TIDAL_COUNTRY_CODE or US.
  --tidal-min-confidence <n>  Minimum match confidence before replacing external_url. Default: 0.82.
  --output <path>             Write JSON output instead of printing it.
`);
  process.exit(1);
}

async function main() {
  const [command, ...rest] = process.argv.slice(2);
  const args = parseArgs(rest);

  if (!command || args.help || args.h) {
    usage();
  }

  if (command === "run") {
    const options: RunOptions = {
      configPath: stringArg(args, "config") ?? path.join(SCRIPT_DIR, "config.example.json"),
      dryRun: Boolean(args["dry-run"]),
      force: Boolean(args.force),
    };
    const result = await runPipeline(options);
    await emitJson(result, stringArg(args, "output"));
    return;
  }

  if (command === "profile") {
    const collectionPath = stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json");
    const collection = await loadAlbums(collectionPath, true);
    applyLocalVectorIndex(collection);
    const profile = buildCollectionProfile(collection);
    await emitJson(profile, stringArg(args, "output"));
    return;
  }

  if (command === "embed") {
    const dryRun = Boolean(args["dry-run"]);
    const databasePath = stringArg(args, "database");
    const options: EmbedOptions = {
      collectionPath: stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json"),
      externalPath: stringArg(args, "external-catalog"),
      output: stringArg(args, "output") ?? (dryRun || databasePath ? undefined : path.join(SCRIPT_DIR, "embeddings.json")),
      databasePath,
      baseUrl: stringArg(args, "embedding-base-url") ?? process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
      apiKey: stringArg(args, "embedding-api-key") ?? process.env.OPENAI_API_KEY,
      model: stringArg(args, "embedding-model") ?? "text-embedding-3-small",
      batchSize: numberArg(args, "embedding-batch-size", 64),
      dimensions: optionalNumberArg(args, "embedding-dimensions"),
      dryRun,
      force: Boolean(args.force),
    };
    const result = await buildEmbeddingIndex(options);
    await emitJson(result, options.output);
    return;
  }

  if (command === "musicbrainz-dump") {
    const corePath = stringArg(args, "core") ?? stringArg(args, "core-dump");
    if (!corePath) throw new Error("musicbrainz-dump requires --core <mbdump.tar.bz2 or extracted directory>");
    const options: MusicBrainzDumpOptions = {
      corePath,
      derivedPath: stringArg(args, "derived") ?? stringArg(args, "supplementary") ?? stringArg(args, "supplementary-dump"),
      output: stringArg(args, "output") ?? path.join(SCRIPT_DIR, "mb-candidate-catalog.jsonl"),
      includeDerivedTags: !Boolean(args["no-derived-tags"]),
      minTagCount: numberArg(args, "min-tag-count", 2),
      maxRows: numberArg(args, "max-rows", 0),
    };
    const result = await buildMusicBrainzDumpCandidateCatalog(options);
    await emitJson(result);
    return;
  }

  if (command === "catalog") {
    const options: CatalogOptions = {
      collectionPath: stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json"),
      output: stringArg(args, "output"),
      candidateCatalog: stringArg(args, "candidate-catalog"),
      limit: numberArg(args, "limit", 250),
      topGenres: numberArg(args, "top-genres", 8),
      topArtists: numberArg(args, "top-artists", 10),
      perQuery: numberArg(args, "per-query", 25),
      recentYears: recentYears(),
      recentPerQuery: numberArg(args, "recent-per-query", 10),
      dryRun: Boolean(args["dry-run"]),
      musicBrainzUserAgent: stringArg(args, "musicbrainz-user-agent") ?? DEFAULT_USER_AGENT,
      musicBrainzDelayMs: numberArg(args, "musicbrainz-delay-ms", 1100),
      lastfmApiKey: stringArg(args, "lastfm-api-key") ?? process.env.LASTFM_API_KEY,
      lastfmDelayMs: numberArg(args, "lastfm-delay-ms", 250),
    };
    const result = await buildExternalCatalog(options);
    await emitJson(result, options.output);
    return;
  }

  if (command === "recommend") {
    const options: RecommendOptions = {
      collectionPath: stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json"),
      externalPath: stringArg(args, "external-catalog"),
      embeddingsPath: stringArg(args, "embeddings"),
      databasePath: stringArg(args, "database"),
      embeddingModel: stringArg(args, "embedding-model"),
      embeddingBaseUrl: stringArg(args, "embedding-base-url"),
      embeddingDimensions: optionalNumberArg(args, "embedding-dimensions"),
      seed: stringArg(args, "seed"),
      seedAlbumId: stringArg(args, "seed-album-id"),
      ownedCount: numberArg(args, "owned-count", 6),
      discoveryCount: numberArg(args, "discovery-count", 6),
      recentDiscoveryCount: numberArg(args, "recent-discovery-count", 2),
      poolSize: numberArg(args, "pool-size", 60),
      output: stringArg(args, "output"),
      format: (stringArg(args, "format") as RecommendOptions["format"]) ?? "recommendations",
      lmStudioUrl: stringArg(args, "lm-studio-url"),
      rerankModel: stringArg(args, "rerank-model"),
      tidal: tidalOptionsFromArgs(args),
    };
    const result = await recommend(options);
    await emitJson(result, options.output);
    return;
  }

  usage();
}

async function recommend(options: RecommendOptions) {
  const context = await prepareRecommendationContext(
    options.collectionPath,
    options.externalPath,
    options.embeddingsPath,
    undefined,
    options.databasePath,
    options.embeddingModel,
    options.embeddingBaseUrl,
    options.embeddingDimensions,
  );
  return recommendWithContext(context, options);
}

async function prepareRecommendationContext(
  collectionPath: string,
  externalPath?: string,
  embeddingsPath?: string,
  preloadedCollection?: AlbumRecord[],
  databasePath?: string,
  embeddingModel?: string,
  embeddingBaseUrl?: string,
  embeddingDimensions?: number,
): Promise<RecommendationContext> {
  const totalTimer = timerStart();
  progress("recommendations", "loading recommendation albums", {
    collection: collectionPath,
    external: externalPath ?? null,
    embeddings: embeddingsPath ?? null,
    database: databasePath ?? null,
  });
  const collectionTimer = timerStart();
  const collection = preloadedCollection ?? await loadAlbums(collectionPath, true);
  progress("recommendations", "collection albums loaded", { albums: collection.length, duration_ms: timerMs(collectionTimer) });
  const externalTimer = timerStart();
  const externalRaw = externalPath ? await loadAlbums(externalPath, false) : [];
  progress("recommendations", "external albums loaded", { albums: externalRaw.length, duration_ms: timerMs(externalTimer) });
  const dedupeTimer = timerStart();
  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
  const external = externalRaw.filter((album) => !albumIdentityKeys(album).some((key) => ownedIdentities.has(key)));
  const allAlbums = [...collection, ...external];
  progress("recommendations", "recommendation albums deduped", {
    collection: collection.length,
    external_raw: externalRaw.length,
    external: external.length,
    total: allAlbums.length,
    duration_ms: timerMs(dedupeTimer),
  });
  const embeddingTimer = timerStart();
  const resolvedEmbeddingModel = databasePath
    ? await applyEmbeddingDatabaseIndex(allAlbums, databasePath, embeddingModel, embeddingBaseUrl, embeddingDimensions)
    : embeddingsPath
    ? await applyEmbeddingIndex(allAlbums, embeddingsPath)
    : applyLocalVectorIndex(allAlbums);
  progress("recommendations", "recommendation vectors prepared", {
    embedding_model: resolvedEmbeddingModel,
    duration_ms: timerMs(embeddingTimer),
  });
  const profileTimer = timerStart();
  const profile = buildCollectionProfile(collection, resolvedEmbeddingModel);
  progress("recommendations", "collection profile built", { duration_ms: timerMs(profileTimer) });
  progress("recommendations", "loaded recommendation albums", {
    collection: collection.length,
    external: external.length,
    total: allAlbums.length,
    embedding_model: resolvedEmbeddingModel,
    duration_ms: timerMs(totalTimer),
  });
  return { collection, external, embeddingModel: resolvedEmbeddingModel, profile };
}

async function recommendWithContext(context: RecommendationContext, options: RecommendOptions) {
  const totalTimer = timerStart();
  const seed = findSeed(context.collection, options);
  progress("recommendations", "seed resolved", {
    seed_album_id: seed.albumId,
    artist: seed.artist,
    title: seed.title,
  });
  const ownedTimer = timerStart();
  const ownedPool = retrieve(seed, context.collection, context.profile, options.poolSize, true);
  progress("recommendations", "owned retrieval complete", {
    seed_album_id: seed.albumId,
    candidates: context.collection.length,
    pool: ownedPool.length,
    duration_ms: timerMs(ownedTimer),
  });
  const discoveryTimer = timerStart();
  const discoveryPool = retrieve(seed, context.external, context.profile, options.poolSize, false);
  progress("recommendations", "discovery retrieval complete", {
    seed_album_id: seed.albumId,
    candidates: context.external.length,
    pool: discoveryPool.length,
    duration_ms: timerMs(discoveryTimer),
  });

  const selectionTimer = timerStart();
  const owned = diversify(ownedPool, options.ownedCount, new Set([seed.artist]));
  const discovery = selectDiscoveryBatch(seed, discoveryPool, owned, options);
  progress("recommendations", "recommendation selection complete", {
    seed_album_id: seed.albumId,
    owned: owned.length,
    discovery: discovery.length,
    duration_ms: timerMs(selectionTimer),
  });

  const serializeTimer = timerStart();
  const baseResult = {
    text_schema_version: TEXT_SCHEMA_VERSION,
    embedding_model: context.embeddingModel,
    seed_album: publicAlbum(seed),
    collection_profile: context.profile,
    owned_recommendations: owned.map(candidateToJson),
    discovery_recommendations: discovery.map(candidateToJson),
  };
  progress("recommendations", "recommendation result serialized", {
    seed_album_id: seed.albumId,
    duration_ms: timerMs(serializeTimer),
  });

  const rerankTimer = timerStart();
  const maybeReranked =
    options.lmStudioUrl && options.rerankModel
      ? await rerankWithLmStudio(baseResult, options.lmStudioUrl, options.rerankModel).catch(
          (error: unknown) => ({
            ...baseResult,
            rerank_warning: `LM Studio rerank failed: ${String(error)}`,
          }),
        )
      : baseResult;
  if (options.lmStudioUrl && options.rerankModel) {
    progress("recommendations", "recommendation rerank complete", {
      seed_album_id: seed.albumId,
      duration_ms: timerMs(rerankTimer),
    });
  }

  if (options.format === "import") {
    const payload = toMusicdImportPayload(seed, maybeReranked);
    if (options.tidal?.enabled) {
      const tidalTimer = timerStart();
      const enriched = await enrichMusicdImportPayloadWithTidal(payload, options.tidal);
      progress("recommendations", "tidal enrichment complete", {
        seed_album_id: seed.albumId,
        duration_ms: timerMs(tidalTimer),
        total_duration_ms: timerMs(totalTimer),
      });
      return enriched;
    }
    progress("recommendations", "recommendation complete", { seed_album_id: seed.albumId, duration_ms: timerMs(totalTimer) });
    return payload;
  }

  if (options.tidal?.enabled) {
    const tidalTimer = timerStart();
    const enriched = await enrichRecommendationResultWithTidal(maybeReranked, options.tidal);
    progress("recommendations", "tidal enrichment complete", {
      seed_album_id: seed.albumId,
      duration_ms: timerMs(tidalTimer),
      total_duration_ms: timerMs(totalTimer),
    });
    return enriched;
  }

  progress("recommendations", "recommendation complete", { seed_album_id: seed.albumId, duration_ms: timerMs(totalTimer) });
  return maybeReranked;
}

async function runPipeline(options: RunOptions) {
  progress("run", "starting", { config: options.configPath, dry_run: options.dryRun, force: options.force });
  const config = JSON.parse(await readFile(options.configPath, "utf8")) as RunConfig;
  const configDir = path.dirname(options.configPath);
  const databasePath = databasePathFromConfig(config, configDir);
  const collectionPath = resolveConfigPath(config.collection ?? "seed.json", configDir);
  const artifacts = {
    profile: resolveOptionalConfigPath(config.artifacts?.profile, configDir),
    catalog: resolveOptionalConfigPath(config.artifacts?.catalog ?? "external-catalog.json", configDir),
    embeddings: resolveOptionalConfigPath(config.artifacts?.embeddings ?? (databasePath ? undefined : "embeddings.json"), configDir),
    recommendations: resolveOptionalConfigPath(config.artifacts?.recommendations ?? "recommendations.json", configDir),
    importPayload: resolveOptionalConfigPath(config.artifacts?.importPayload, configDir),
    database: databasePath,
  };
  const steps: unknown[] = [];

  progress("profile", "loading collection", { collection: collectionPath });
  const collection = await loadAlbums(collectionPath, true);
  progress("profile", "collection loaded", { albums: collection.length });
  applyLocalVectorIndex(collection);
  const profile = buildCollectionProfile(collection);
  if (artifacts.profile) {
    if (options.dryRun) {
      steps.push({ step: "profile", action: "would_write", output: artifacts.profile });
    } else {
      await emitJson(profile, artifacts.profile);
      steps.push({ step: "profile", action: "wrote", output: artifacts.profile });
    }
  }

  const catalogEnabled = config.catalog?.enabled ?? true;
  let catalogPath = artifacts.catalog;
  if (catalogEnabled && catalogPath) {
    const reuseCatalog = !options.dryRun && !options.force && (config.catalog?.reuseExisting ?? true) && (await fileExists(catalogPath));
    if (reuseCatalog) {
      progress("catalog", "reusing existing catalog", { output: catalogPath });
      steps.push({ step: "catalog", action: "reused", output: catalogPath });
    } else {
      const catalogOptions = catalogOptionsFromConfig(config, collectionPath, catalogPath, configDir, options.dryRun);
      if (options.dryRun || catalogOptions.dryRun) {
        const plan = await buildExternalCatalog({ ...catalogOptions, dryRun: true });
        steps.push({ step: "catalog", action: "would_build", output: catalogPath, plan });
      } else {
        progress("catalog", "building catalog", { output: catalogPath });
        const catalog = await buildExternalCatalog(catalogOptions);
        await emitJson(catalog, catalogPath);
        progress("catalog", "catalog written", {
          output: catalogPath,
          albums: Array.isArray((catalog as any).albums) ? (catalog as any).albums.length : 0,
        });
        steps.push({
          step: "catalog",
          action: "wrote",
          output: catalogPath,
          album_count: Array.isArray((catalog as any).albums) ? (catalog as any).albums.length : 0,
        });
      }
    }
  } else {
    if (catalogPath && (await fileExists(catalogPath))) {
      steps.push({ step: "catalog", action: "reused_existing_generation_disabled", output: catalogPath });
    } else {
      catalogPath = undefined;
      steps.push({ step: "catalog", action: "skipped" });
    }
  }

  const embeddingsEnabled = config.embeddings?.enabled ?? true;
  let embeddingsPath = artifacts.embeddings;
  if (embeddingsEnabled && (embeddingsPath || databasePath)) {
    const reuseEmbeddings =
      !databasePath &&
      embeddingsPath &&
      !options.dryRun &&
      !options.force &&
      (config.embeddings?.reuseExisting ?? true) &&
      (await fileExists(embeddingsPath));
    if (reuseEmbeddings) {
      progress("embeddings", "reusing existing embeddings", { output: embeddingsPath });
      steps.push({ step: "embeddings", action: "reused", output: embeddingsPath });
    } else {
      const embedOptions = embedOptionsFromConfig(config, collectionPath, catalogPath, embeddingsPath, databasePath, options.dryRun, options.force);
      if (options.dryRun || embedOptions.dryRun) {
        const preview = await buildEmbeddingIndex({ ...embedOptions, dryRun: true });
        steps.push({
          step: "embeddings",
          action: "would_build",
          output: embeddingsPath ?? databasePath,
          preview,
          note:
            catalogPath && embedOptions.externalPath === undefined
              ? "External catalog does not exist yet; dry-run embedding preview uses collection albums only."
              : undefined,
        });
      } else {
        progress("embeddings", "building embeddings", { output: embeddingsPath ?? null, database: databasePath ?? null });
        const index = await buildEmbeddingIndex(embedOptions);
        if (embeddingsPath) {
          await emitJson(index, embeddingsPath);
        }
        progress("embeddings", "embeddings written", {
          output: embeddingsPath ?? null,
          database: databasePath ?? null,
          albums: (index as EmbeddingIndex | EmbeddingBuildSummary).album_count,
        });
        steps.push({
          step: "embeddings",
          action: databasePath ? "stored" : "wrote",
          output: embeddingsPath,
          database: databasePath,
          album_count: (index as EmbeddingIndex | EmbeddingBuildSummary).album_count,
        });
      }
    }
  } else {
    embeddingsPath = undefined;
    steps.push({ step: "embeddings", action: "skipped" });
  }

  const recommendationsConfig = config.recommendations ?? {};
  const recommendationsEnabled = recommendationsConfig.enabled ?? true;
  if (recommendationsEnabled) {
    const combinedImportTidalOptions =
      recommendationsConfig.format === "import" ? tidalOptionsFromConfig(config) : undefined;
    const recommendationRuns = recommendationOptionsFromConfig(
      config,
      collectionPath,
      catalogPath,
      embeddingsPath,
      databasePath,
      artifacts.recommendations,
    );
    if (options.dryRun) {
      steps.push({
        step: "recommendations",
        action: "would_run",
        output: artifacts.recommendations,
        count: recommendationRuns.length,
        seeds: recommendationRuns.map((run) => run.seed ?? run.seedAlbumId),
      });
    } else {
      const results = [];
      progress("recommendations", "preparing recommendation context", {
        catalog: catalogPath ?? null,
        embeddings: embeddingsPath ?? null,
        database: databasePath ?? null,
        seeds: recommendationRuns.length,
      });
      const recommendationContext = await prepareRecommendationContext(
        collectionPath,
        catalogPath,
        embeddingsPath,
        collection,
        databasePath,
        config.embeddings?.model,
        config.embeddings?.baseUrl,
        config.embeddings?.dimensions,
      );
      progress("recommendations", "recommendation context ready", {
        collection: recommendationContext.collection.length,
        external: recommendationContext.external.length,
        embedding_model: recommendationContext.embeddingModel,
      });
      for (let index = 0; index < recommendationRuns.length; index += 1) {
        const recommendationOptions = recommendationRuns[index];
        progress("recommendations", "running seed", {
          index: index + 1,
          total: recommendationRuns.length,
          seed: recommendationOptions.seed ?? recommendationOptions.seedAlbumId,
        });
        results.push(
          await recommendWithContext(recommendationContext, {
            ...recommendationOptions,
            tidal: combinedImportTidalOptions?.enabled ? undefined : recommendationOptions.tidal,
          }),
        );
      }
      let output =
        recommendationsConfig.format === "import"
          ? combinedMusicdImportPayload(results, options.configPath)
          : {
              generated_at: new Date().toISOString(),
              config_path: options.configPath,
              result_count: results.length,
              results,
            };
      if (combinedImportTidalOptions?.enabled) {
        const tidalTimer = timerStart();
        const beforeCount = Array.isArray((output as any).recommendations) ? (output as any).recommendations.length : 0;
        progress("recommendations", "enriching combined import payload with TIDAL", { recommendations: beforeCount });
        output = await enrichMusicdImportPayloadWithTidal(output, combinedImportTidalOptions);
        progress("recommendations", "combined import TIDAL enrichment complete", {
          recommendations: beforeCount,
          duration_ms: timerMs(tidalTimer),
        });
      }
      if (artifacts.recommendations) {
        await emitJson(output, artifacts.recommendations);
      }
      if (recommendationsConfig.format === "import" && artifacts.importPayload && artifacts.importPayload !== artifacts.recommendations) {
        await emitJson(output, artifacts.importPayload);
      }
      steps.push({
        step: "recommendations",
        action: artifacts.recommendations ? "wrote" : "ran",
        output: artifacts.recommendations,
        count: results.length,
      });
      progress("recommendations", "recommendations complete", { count: results.length, output: artifacts.recommendations ?? null });
    }
  } else {
    steps.push({ step: "recommendations", action: "skipped" });
  }

  progress("run", "complete", { steps: steps.length });
  return {
    ok: true,
    config_path: options.configPath,
    dry_run: options.dryRun,
    force: options.force,
    artifacts,
    steps,
  };
}

async function buildEmbeddingIndex(options: EmbedOptions) {
  progress("embeddings", "loading embedding inputs", {
    collection: options.collectionPath,
    external: options.externalPath ?? null,
    database: options.databasePath ?? null,
    dry_run: options.dryRun,
  });
  const collection = await loadAlbums(options.collectionPath, true);
  const externalRaw = options.externalPath ? await loadAlbums(options.externalPath, false) : [];
  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
  const external = externalRaw.filter((album) => !albumIdentityKeys(album).some((key) => ownedIdentities.has(key)));
  const albums = [...collection, ...external];
  progress("embeddings", "embedding inputs ready", {
    collection: collection.length,
    external: external.length,
    total: albums.length,
    batch_size: options.batchSize,
  });

  if (options.dryRun) {
    return {
      embedding_schema_version: "album_embeddings_v1",
      text_schema_version: TEXT_SCHEMA_VERSION,
      embedding_model: options.model,
      embedding_base_url: safeBaseUrl(options.baseUrl),
      album_count: albums.length,
      batch_size: options.batchSize,
      dimensions: options.dimensions ?? null,
      database_path: options.databasePath ?? null,
      estimated_requests: Math.ceil(albums.length / Math.max(1, options.batchSize)),
      sample_inputs: albums.slice(0, 3).map((album) => ({
        album_id: album.albumId,
        artist: album.artist,
        title: album.title,
        text_hash: stableKey(album.normalizedText),
        normalized_text: album.normalizedText,
      })),
    };
  }

  if (options.databasePath) {
    return buildEmbeddingIndexInDatabase(options, albums);
  }

  const embeddings: AlbumEmbedding[] = [];
  for (let index = 0; index < albums.length; index += options.batchSize) {
    const batch = albums.slice(index, index + options.batchSize);
    const batchContext = {
      batch: Math.floor(index / options.batchSize) + 1,
      batches: Math.ceil(albums.length / Math.max(1, options.batchSize)),
      start: index + 1,
      count: batch.length,
    };
    progress("embeddings", "requesting batch", batchContext);
    const vectors = await createEmbeddings(batch.map((album) => album.normalizedText), options, batchContext);
    if (vectors.length !== batch.length) {
      progress("embeddings", "provider vector count mismatch", {
        ...batchContext,
        expected: batch.length,
        received: vectors.length,
      });
      throw new Error(
        `Embedding provider returned ${vectors.length} vector(s) for ${batch.length} input(s) in batch ${batchContext.batch}/${batchContext.batches}`,
      );
    }
    for (let batchIndex = 0; batchIndex < batch.length; batchIndex += 1) {
      const album = batch[batchIndex];
      const vector = applyRequestedDimensions(vectors[batchIndex], options, batchContext, album);
      if (!isFiniteVector(vector)) {
        progress("embeddings", "invalid embedding vector", {
          ...batchContext,
          item: batchIndex + 1,
          album_id: album.albumId,
          artist: album.artist,
          title: album.title,
          dimensions: Array.isArray(vector) ? vector.length : null,
        });
        throw new Error(`Embedding vector for ${album.artist} - ${album.title} is missing or contains non-finite values`);
      }
      embeddings.push({
        album_id: album.albumId,
        artist: album.artist,
        title: album.title,
        owned: album.owned,
        text_hash: stableKey(album.normalizedText),
        dimensions: vector.length,
        embedding: normalizeArrayVector(vector),
        musicbrainz_release_id: album.musicbrainzReleaseId,
        musicbrainz_release_group_id: album.musicbrainzReleaseGroupId,
      });
    }
    progress("embeddings", "batch complete", { embedded: embeddings.length, total: albums.length });
  }

  return {
    embedding_schema_version: "album_embeddings_v1",
    text_schema_version: TEXT_SCHEMA_VERSION,
    embedding_model: options.model,
    embedding_base_url: safeBaseUrl(options.baseUrl),
    generated_at: new Date().toISOString(),
    album_count: embeddings.length,
    embeddings,
  } satisfies EmbeddingIndex;
}

async function buildEmbeddingIndexInDatabase(options: EmbedOptions, albums: AlbumRecord[]): Promise<EmbeddingBuildSummary> {
  if (!options.databasePath) throw new Error("databasePath is required for database embedding builds");
  const store = await openRecommenderDatabase(options.databasePath);
  try {
    const embeddingBaseUrl = safeBaseUrl(options.baseUrl);
    const pending: AlbumRecord[] = [];
    let reused = 0;
    let stale = 0;
    let storedDimensions: number | null = null;

    progress("embeddings", "checking database embeddings", {
      database: options.databasePath,
      albums: albums.length,
      model: options.model,
      base_url: embeddingBaseUrl,
      force: options.force,
    });
    for (const album of albums) {
      const row = getStoredEmbeddingRow(store, album.albumId, options.model, embeddingBaseUrl);
      if (!options.force && row && row.text_hash === stableKey(album.normalizedText) && (!options.dimensions || Number(row.dimensions) === options.dimensions)) {
        reused += 1;
        storedDimensions = storedDimensions ?? Number(row.dimensions);
        continue;
      }
      if (row && (row.text_hash !== stableKey(album.normalizedText) || (options.dimensions && Number(row.dimensions) !== options.dimensions))) stale += 1;
      pending.push(album);
    }

    progress("embeddings", "database embedding check complete", {
      database: options.databasePath,
      reused,
      stale,
      pending: pending.length,
    });

    let embedded = 0;
    for (let index = 0; index < pending.length; index += options.batchSize) {
      const batch = pending.slice(index, index + options.batchSize);
      const batchContext = {
        batch: Math.floor(index / options.batchSize) + 1,
        batches: Math.ceil(pending.length / Math.max(1, options.batchSize)),
        start: index + 1,
        count: batch.length,
      };
      progress("embeddings", "requesting database batch", batchContext);
      const vectors = await createEmbeddings(batch.map((album) => album.normalizedText), options, batchContext);
      if (vectors.length !== batch.length) {
        progress("embeddings", "provider vector count mismatch", {
          ...batchContext,
          expected: batch.length,
          received: vectors.length,
        });
        throw new Error(
          `Embedding provider returned ${vectors.length} vector(s) for ${batch.length} input(s) in batch ${batchContext.batch}/${batchContext.batches}`,
        );
      }

      const rows = batch.map((album, batchIndex) => {
        const vector = applyRequestedDimensions(vectors[batchIndex], options, batchContext, album);
        if (!isFiniteVector(vector)) {
          progress("embeddings", "invalid embedding vector", {
            ...batchContext,
            item: batchIndex + 1,
            album_id: album.albumId,
            artist: album.artist,
            title: album.title,
            dimensions: Array.isArray(vector) ? vector.length : null,
          });
          throw new Error(`Embedding vector for ${album.artist} - ${album.title} is missing or contains non-finite values`);
        }
        storedDimensions = storedDimensions ?? vector.length;
        return {
          album,
          embedding: normalizeArrayVector(vector),
          dimensions: vector.length,
          textHash: stableKey(album.normalizedText),
        };
      });
      upsertStoredEmbeddings(store, rows, options.model, embeddingBaseUrl);
      embedded += rows.length;
      progress("embeddings", "database batch stored", {
        embedded,
        pending: pending.length,
        reused,
        database: options.databasePath,
      });
    }

    return {
      embedding_schema_version: "album_embeddings_v1",
      text_schema_version: TEXT_SCHEMA_VERSION,
      embedding_model: options.model,
      embedding_base_url: embeddingBaseUrl,
      generated_at: new Date().toISOString(),
      album_count: albums.length,
      database_path: options.databasePath,
      requested_dimensions: options.dimensions ?? null,
      stored_dimensions: storedDimensions,
      reused_embeddings: reused,
      stale_embeddings: stale,
      embedded_embeddings: embedded,
    };
  } finally {
    store.close();
  }
}

async function createEmbeddings(
  inputs: string[],
  options: EmbedOptions,
  batchContext: EmbeddingBatchLogContext,
): Promise<number[][]> {
  const url = new URL(`${options.baseUrl.replace(/\/$/, "")}/embeddings`);
  const headers: Record<string, string> = {
    "accept": "application/json",
    "content-type": "application/json",
  };
  if (options.apiKey) {
    headers.authorization = `Bearer ${options.apiKey}`;
  }

  const body: Record<string, unknown> = {
    model: options.model,
    input: inputs,
    encoding_format: "float",
  };
  if (options.dimensions) {
    body.dimensions = options.dimensions;
  }

  const response = await fetch(url, {
    method: "POST",
    headers,
    body: JSON.stringify(body),
  }).catch((error) => {
    progressError("embeddings", "embedding provider request failed", error, {
      ...batchContext,
      url: safeEmbeddingUrl(url),
      model: options.model,
    });
    throw error;
  });
  const responseText = await response.text();
  progress("embeddings", "provider response received", {
    ...batchContext,
    status: response.status,
    ok: response.ok,
    content_type: response.headers.get("content-type"),
    content_length: response.headers.get("content-length"),
    body_bytes: Buffer.byteLength(responseText, "utf8"),
    body_hash: stableKey(responseText),
  });
  if (!response.ok) {
    progress("embeddings", "provider error body", {
      ...batchContext,
      status: response.status,
      preview: previewText(responseText, 400),
    });
    throw new Error(`${response.status} ${response.statusText}: ${responseText}`);
  }

  let json: any;
  try {
    json = JSON.parse(responseText);
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    progress("embeddings", "provider JSON parse failed", {
      ...batchContext,
      reason,
      body_bytes: Buffer.byteLength(responseText, "utf8"),
      body_hash: stableKey(responseText),
      preview: previewText(responseText, 800),
    });
    throw new Error(`Embedding provider returned invalid JSON in batch ${batchContext.batch}/${batchContext.batches}: ${reason}`);
  }
  const data = Array.isArray(json.data) ? json.data : [];
  if (!Array.isArray(json.data)) {
    progress("embeddings", "provider response missing data array", {
      ...batchContext,
      top_level_keys: json && typeof json === "object" ? Object.keys(json).slice(0, 20) : [],
    });
    return [];
  }

  const sorted = [...data].sort((a: any, b: any) => Number(a.index ?? 0) - Number(b.index ?? 0));
  const invalidRows: Record<string, unknown>[] = [];
  const vectors: number[][] = [];
  for (const [rowNumber, item] of sorted.entries()) {
    const embedding = item?.embedding;
    if (!isFiniteVector(embedding)) {
      invalidRows.push({
        row: rowNumber + 1,
        provider_index: item?.index ?? null,
        embedding_type: Array.isArray(embedding) ? "array" : typeof embedding,
        dimensions: Array.isArray(embedding) ? embedding.length : null,
      });
      continue;
    }
    vectors.push(embedding);
  }
  if (invalidRows.length) {
    progress("embeddings", "provider returned malformed embedding rows", {
      ...batchContext,
      invalid_rows: invalidRows.slice(0, 10),
      invalid_count: invalidRows.length,
      rows: sorted.length,
    });
    throw new Error(`Embedding provider returned ${invalidRows.length} malformed embedding row(s) in batch ${batchContext.batch}/${batchContext.batches}`);
  }
  progress("embeddings", "provider embeddings parsed", {
    ...batchContext,
    rows: sorted.length,
    vectors: vectors.length,
    dimensions: vectors[0]?.length ?? null,
  });
  return vectors;
}

async function buildExternalCatalog(options: CatalogOptions) {
  const collection = await loadAlbums(options.collectionPath, true);
  applyLocalVectorIndex(collection);
  const profile = buildCollectionProfile(collection);
  const plan = buildCatalogPlan(profile, options);

  if (options.dryRun) {
    return {
      catalog_schema_version: "external_catalog_v1",
      generated_at: new Date().toISOString(),
      dry_run: true,
      data_license_notes: [
        "Catalog artifacts are local operator data and are not intended to be committed.",
        "MusicBrainz core data is CC0; MusicBrainz supplementary tag/rating/annotation data is CC BY-NC-SA 3.0.",
        "Last.fm enrichment requires an operator-provided API key and is subject to Last.fm API terms.",
      ],
      collection_profile: profile,
      plan,
      notes: [
        options.candidateCatalog
          ? `Catalog will sample local candidate catalog: ${options.candidateCatalog}.`
          : "MusicBrainz queries are rate-limited by --musicbrainz-delay-ms.",
        options.lastfmApiKey
          ? "Last.fm expansion is enabled."
          : "Last.fm expansion is disabled because no API key was provided.",
      ],
    };
  }

  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
  if (options.candidateCatalog) {
    return buildExternalCatalogFromCandidateCatalog(options, profile, plan, ownedIdentities);
  }

  const candidates = new Map<string, CatalogCandidate>();
  const musicBrainzState: RateLimitState = { lastRequestAt: 0 };
  const lastfmState: RateLimitState = { lastRequestAt: 0 };
  const warnings: string[] = [];

  for (const query of [...plan.same_genre_queries, ...plan.adjacent_genre_queries, ...plan.exploratory_queries, ...plan.recent_queries]) {
    const releaseGroups = await searchMusicBrainzReleaseGroups(query, options, musicBrainzState).catch((error) => {
      warnings.push(`MusicBrainz query failed for ${query.tag}: ${String(error)}`);
      return [];
    });
    for (const releaseGroup of releaseGroups) {
      addCatalogCandidate(candidates, releaseGroupToAlbum(releaseGroup, query), ownedIdentities);
    }
    if (candidates.size >= options.limit * 1.6) break;
  }

  if (options.lastfmApiKey) {
    for (const tag of plan.same_genre_queries.slice(0, options.topGenres).map((query) => query.tag)) {
      const albums = await lastfmTopAlbumsByTag(tag, options, lastfmState).catch((error) => {
        warnings.push(`Last.fm tag query failed for ${tag}: ${String(error)}`);
        return [];
      });
      for (const album of albums) {
        addCatalogCandidate(
          candidates,
          lastfmAlbumToCatalogCandidate(album, "lastfm_tag", `Last.fm top albums for ${tag}`, [tag]),
          ownedIdentities,
        );
      }
    }

    for (const artist of plan.lastfm_artist_queries) {
      const similarArtists = await lastfmSimilarArtists(artist, options, lastfmState).catch((error) => {
        warnings.push(`Last.fm similar artist query failed for ${artist}: ${String(error)}`);
        return [];
      });
      for (const similarArtist of similarArtists.slice(0, 4)) {
        const albums = await lastfmTopAlbumsByArtist(similarArtist, options, lastfmState).catch((error) => {
          warnings.push(`Last.fm top albums query failed for ${similarArtist}: ${String(error)}`);
          return [];
        });
        for (const album of albums.slice(0, 3)) {
          addCatalogCandidate(
            candidates,
            lastfmAlbumToCatalogCandidate(album, "lastfm_artist", `Last.fm similar artist path: ${artist} -> ${similarArtist}`),
            ownedIdentities,
          );
        }
      }
      if (candidates.size >= options.limit * 1.8) break;
    }
  } else {
    warnings.push("Skipped Last.fm expansion because --lastfm-api-key or LASTFM_API_KEY was not provided.");
  }

  const albums = [...candidates.values()]
    .sort((a, b) => b.score - a.score || a.album.artist!.localeCompare(b.album.artist!) || a.album.title!.localeCompare(b.album.title!))
    .slice(0, options.limit)
    .map((candidate) => candidate.album);

  return {
    catalog_schema_version: "external_catalog_v1",
    generated_at: new Date().toISOString(),
    text_schema_version: TEXT_SCHEMA_VERSION,
    collection_profile: profile,
    sources: {
      musicbrainz: {
        root_url: MUSICBRAINZ_API_ROOT,
        user_agent: options.musicBrainzUserAgent,
        data_license_url: "https://musicbrainz.org/doc/About/Data_License",
      },
      lastfm: {
        enabled: Boolean(options.lastfmApiKey),
        api_url: "https://www.last.fm/api",
      },
    },
    data_license_notes: [
      "Catalog artifacts are local operator data and are not intended to be committed.",
      "MusicBrainz core data is CC0; MusicBrainz supplementary tag/rating/annotation data is CC BY-NC-SA 3.0.",
      "Last.fm enrichment requires an operator-provided API key and is subject to Last.fm API terms.",
    ],
    plan,
    warnings,
    albums,
  };
}

async function buildExternalCatalogFromCandidateCatalog(
  options: CatalogOptions,
  profile: CollectionProfile,
  plan: CatalogPlan,
  ownedIdentities: Set<string>,
) {
  if (!options.candidateCatalog) throw new Error("candidateCatalog is required");
  progress("catalog", "sampling candidate catalog", {
    candidate_catalog: options.candidateCatalog,
    limit: options.limit,
  });
  const candidates = new Map<string, CatalogCandidate>();
  const warnings: string[] = [];
  const pruneLimit = Math.max(options.limit * 6, 1000);
  let scanned = 0;
  for await (const input of readAlbumInputs(options.candidateCatalog)) {
    addCatalogCandidate(candidates, candidateCatalogAlbumToCatalogCandidate(input, plan, options), ownedIdentities);
    scanned += 1;
    if (scanned % 5000 === 0 && candidates.size > pruneLimit) {
      pruneCatalogCandidates(candidates, pruneLimit);
    }
    if (scanned % 25000 === 0) {
      progress("catalog", "candidate scan progress", { scanned, retained: candidates.size });
    }
  }
  pruneCatalogCandidates(candidates, pruneLimit);
  progress("catalog", "candidate scan complete", { scanned, retained: candidates.size });

  if (options.lastfmApiKey) {
    const lastfmState: RateLimitState = { lastRequestAt: 0 };
    for (const tag of plan.same_genre_queries.slice(0, options.topGenres).map((query) => query.tag)) {
      const albums = await lastfmTopAlbumsByTag(tag, options, lastfmState).catch((error) => {
        warnings.push(`Last.fm tag query failed for ${tag}: ${String(error)}`);
        return [];
      });
      for (const album of albums) {
        addCatalogCandidate(
          candidates,
          lastfmAlbumToCatalogCandidate(album, "lastfm_tag", `Last.fm top albums for ${tag}`, [tag]),
          ownedIdentities,
        );
      }
    }
  } else {
    warnings.push("Skipped Last.fm expansion because --lastfm-api-key or LASTFM_API_KEY was not provided.");
  }

  const albums = [...candidates.values()]
    .sort((a, b) => b.score - a.score || a.album.artist!.localeCompare(b.album.artist!) || a.album.title!.localeCompare(b.album.title!))
    .slice(0, options.limit)
    .map((candidate) => candidate.album);
  progress("catalog", "candidate catalog sampled", { albums: albums.length });

  return {
    catalog_schema_version: "external_catalog_v1",
    generated_at: new Date().toISOString(),
    text_schema_version: TEXT_SCHEMA_VERSION,
    collection_profile: profile,
    sources: {
      candidate_catalog: {
        path: options.candidateCatalog,
        scanned_albums: scanned,
      },
      lastfm: {
        enabled: Boolean(options.lastfmApiKey),
        api_url: "https://www.last.fm/api",
      },
    },
    data_license_notes: [
      "Catalog artifacts are local operator data and are not intended to be committed.",
      "Candidate catalog provenance and data license obligations depend on the local dataset used to build it.",
    ],
    plan,
    warnings,
    albums,
  };
}

function candidateCatalogAlbumToCatalogCandidate(input: AlbumInput, plan: CatalogPlan, options: CatalogOptions): CatalogCandidate {
  const artist = normalizeOptionalText(input.artist) ?? "Unknown Artist";
  const title = normalizeOptionalText(input.title) ?? "Untitled";
  const tags = unique([...(input.tags ?? []), ...(input.moods ?? [])].map((tag) => cleanExternalTag(tag)).filter(Boolean) as string[]);
  const genres = unique((input.genres ?? []).map(normalizeGenre).filter(Boolean));
  const signals = [...genres, ...tags].map(normalizeSignal);
  let bucket: CatalogCandidate["bucket"] = "exploratory";
  let bestScore = 0.2;

  for (const query of [...plan.same_genre_queries, ...plan.adjacent_genre_queries, ...plan.exploratory_queries]) {
    const querySignals = [normalizeSignal(query.tag), normalizeSignal(genreFamily(query.tag))];
    const matched = signals.some((signal) => querySignals.includes(signal) || querySignals.includes(normalizeSignal(genreFamily(signal))));
    if (!matched) continue;
    const score = bucketWeight(query.bucket);
    if (score > bestScore) {
      bestScore = score;
      bucket = query.bucket;
    }
  }

  const year = parseYear(input.year ?? input.release_date);
  if (year !== null && options.recentYears.includes(year)) {
    bestScore = Math.max(bestScore, bucketWeight("recent"));
    bucket = "recent";
  }

  const popularity = typeof (input as any).popularity_score === "number" ? clamp((input as any).popularity_score, 0, 1) : 0;
  const tagDepth = Math.min(0.08, tags.length * 0.01);
  const score = bestScore + popularity * 0.08 + tagDepth + stableScore(`${artist}:${title}`) * 0.015;

  return {
    bucket,
    score,
    album: {
      ...(input as ExternalCatalogAlbum),
      catalog_id: (input as ExternalCatalogAlbum).catalog_id ?? input.album_id ?? `candidate:${stableKey(`${artist}:${title}`)}`,
      album_id: input.album_id ?? `candidate:${stableKey(`${artist}:${title}`)}`,
      artist,
      title,
      genres,
      tags,
      source: input.source ?? "candidate-catalog",
      catalog_sources: unique([...(input as ExternalCatalogAlbum).catalog_sources ?? [], input.source ?? "candidate-catalog"]),
      source_evidence: unique([...(input as ExternalCatalogAlbum).source_evidence ?? [], `Local candidate catalog: ${options.candidateCatalog}`]),
      popularity_score: round(score),
    },
  };
}

function buildCatalogPlan(profile: CollectionProfile, options: CatalogOptions): CatalogPlan {
  const sameGenreTarget = Math.max(1, Math.round(options.limit * 0.65));
  const adjacentTarget = Math.max(1, Math.round(options.limit * 0.25));
  const exploratoryTarget = Math.max(1, options.limit - sameGenreTarget - adjacentTarget);
  const topGenres = profile.top_genres.slice(0, options.topGenres).map(([genre]) => genre);
  const adjacentTags = unique(topGenres.flatMap((genre) => ADJACENT_TAGS.get(genre) ?? [])).slice(
    0,
    Math.max(6, options.topGenres * 2),
  );
  const exploratoryTags = EXPLORATORY_TAGS.filter(
    (tag) => !topGenres.some((genre) => sameTag(genre, tag)) && !adjacentTags.some((adjacent) => sameTag(adjacent, tag)),
  ).slice(0, 6);

  return {
    same_genre_queries: topGenres.map((tag) => catalogQuery("same_genre", tag, sameGenreTarget, topGenres.length)),
    adjacent_genre_queries: adjacentTags.map((tag) => catalogQuery("adjacent", tag, adjacentTarget, adjacentTags.length)),
    exploratory_queries: exploratoryTags.map((tag) => catalogQuery("exploratory", tag, exploratoryTarget, exploratoryTags.length)),
    recent_queries: topGenres.map((tag) => recentCatalogQuery(tag, options)),
    lastfm_artist_queries: profile.top_artists.slice(0, options.topArtists).map(([artist]) => artist),
  };
}

function catalogQuery(bucket: CatalogQuery["bucket"], tag: string, bucketTarget: number, queryCount: number): CatalogQuery {
  return {
    bucket,
    tag,
    query: `tag:${quoteSearchTerm(tag)} AND primarytype:album`,
    target_count: Math.max(3, Math.ceil(bucketTarget / Math.max(1, queryCount))),
  };
}

function recentCatalogQuery(tag: string, options: CatalogOptions): CatalogQuery {
  const sortedYears = [...options.recentYears].sort((a, b) => a - b);
  const start = sortedYears[0];
  const end = sortedYears.at(-1) ?? start;
  return {
    bucket: "recent",
    tag,
    query: `tag:${quoteSearchTerm(tag)} AND primarytype:album AND firstreleasedate:[${start} TO ${end}]`,
    target_count: options.recentPerQuery,
  };
}

async function searchMusicBrainzReleaseGroups(
  query: CatalogQuery,
  options: CatalogOptions,
  state: RateLimitState,
): Promise<any[]> {
  const url = new URL(`${MUSICBRAINZ_API_ROOT}/release-group/`);
  url.searchParams.set("query", query.query);
  url.searchParams.set("fmt", "json");
  url.searchParams.set("limit", String(Math.min(options.perQuery, query.target_count * 2)));
  const body = await fetchJson(url, {
    "accept": "application/json",
    "user-agent": options.musicBrainzUserAgent,
  }, options.musicBrainzDelayMs, state);
  return Array.isArray(body["release-groups"]) ? body["release-groups"].filter(usableReleaseGroup) : [];
}

function releaseGroupToAlbum(releaseGroup: any, query: CatalogQuery): CatalogCandidate {
  const artist = artistCreditName(releaseGroup["artist-credit"]) ?? "Unknown Artist";
  const title = normalizeOptionalText(releaseGroup.title) ?? "Untitled";
  const releaseDate = normalizeOptionalText(releaseGroup["first-release-date"]);
  const tags = unique([
    query.tag,
    ...(Array.isArray(releaseGroup.tags) ? releaseGroup.tags.map((tag: any) => cleanExternalTag(tag.name)).filter(Boolean) : []),
  ]);
  const genres = unique([
    normalizeGenre(query.tag),
    ...(Array.isArray(releaseGroup.genres) ? releaseGroup.genres.map((genre: any) => normalizeGenre(genre.name)).filter(Boolean) : []),
  ]);
  const score = Number(releaseGroup.score ?? 0) / 100;

  return {
    bucket: query.bucket,
    score: bucketWeight(query.bucket) + score,
    album: {
      catalog_id: `mb-rg:${releaseGroup.id}`,
      album_id: `mb-rg:${releaseGroup.id}`,
      artist,
      title,
      release_date: releaseDate,
      genres,
      tags,
      source: "musicbrainz",
      catalog_sources: ["musicbrainz"],
      source_evidence: [`MusicBrainz release-group search: ${query.query}`],
      musicbrainz_release_group_id: normalizeOptionalText(releaseGroup.id),
      musicbrainz_release_id: null,
      external_url: releaseGroup.id ? `https://musicbrainz.org/release-group/${releaseGroup.id}` : null,
      popularity_score: score,
    },
  };
}

function usableReleaseGroup(releaseGroup: any): boolean {
  const primaryType = normalizeOptionalText(releaseGroup["primary-type"] ?? releaseGroup.type);
  if (primaryType && primaryType !== "Album") return false;

  const secondaryTypes = Array.isArray(releaseGroup["secondary-types"]) ? releaseGroup["secondary-types"] : [];
  const blockedSecondaryTypes = new Set(["Compilation", "Single", "EP", "Live", "Remix", "Soundtrack", "Interview", "DJ-mix"]);
  if (secondaryTypes.some((type: string) => blockedSecondaryTypes.has(type))) return false;

  const artist = artistCreditName(releaseGroup["artist-credit"]);
  if (!artist || sameArtist(artist, "Various Artists")) return false;

  const title = normalizeOptionalText(releaseGroup.title);
  if (!title) return false;
  return true;
}

async function lastfmTopAlbumsByTag(tag: string, options: CatalogOptions, state: RateLimitState): Promise<any[]> {
  const body = await fetchLastfm(
    {
      method: "tag.gettopalbums",
      tag,
      limit: String(Math.max(5, Math.min(options.perQuery, 50))),
    },
    options,
    state,
  );
  const albums = body.albums?.album;
  return Array.isArray(albums) ? albums : [];
}

async function lastfmSimilarArtists(artist: string, options: CatalogOptions, state: RateLimitState): Promise<string[]> {
  const body = await fetchLastfm(
    {
      method: "artist.getsimilar",
      artist,
      limit: "6",
    },
    options,
    state,
  );
  const artists = body.similarartists?.artist;
  return Array.isArray(artists) ? artists.map((item: any) => item.name).filter(Boolean) : [];
}

async function lastfmTopAlbumsByArtist(artist: string, options: CatalogOptions, state: RateLimitState): Promise<any[]> {
  const body = await fetchLastfm(
    {
      method: "artist.gettopalbums",
      artist,
      limit: "5",
    },
    options,
    state,
  );
  const albums = body.topalbums?.album;
  return Array.isArray(albums) ? albums : [];
}

async function fetchLastfm(params: Record<string, string>, options: CatalogOptions, state: RateLimitState): Promise<any> {
  if (!options.lastfmApiKey) throw new Error("Last.fm API key is required");
  const url = new URL(LASTFM_API_ROOT);
  for (const [key, value] of Object.entries(params)) {
    url.searchParams.set(key, value);
  }
  url.searchParams.set("api_key", options.lastfmApiKey);
  url.searchParams.set("format", "json");
  return fetchJson(url, {
    "accept": "application/json",
    "user-agent": DEFAULT_USER_AGENT,
  }, options.lastfmDelayMs, state);
}

function lastfmAlbumToCatalogCandidate(
  album: any,
  bucket: CatalogCandidate["bucket"],
  evidence: string,
  seedTags: string[] = [],
): CatalogCandidate {
  const artist = normalizeOptionalText(album.artist?.name ?? album.artist) ?? "Unknown Artist";
  const title = normalizeOptionalText(album.name) ?? "Untitled";
  const listeners = Number(album.listeners ?? 0);
  const playcount = Number(album.playcount ?? 0);
  const mbid = normalizeOptionalText(album.mbid);
  const externalUrl = normalizeOptionalText(album.url);
  const score = bucketWeight(bucket) + Math.log10(Math.max(1, listeners + playcount)) / 10;
  const genres = unique(seedTags.map(normalizeGenre).filter(Boolean));
  const tags = unique(seedTags.map((tag) => cleanExternalTag(tag)).filter((tag): tag is string => tag !== null));

  return {
    bucket,
    score,
    album: {
      catalog_id: mbid ? `lastfm-mbid:${mbid}` : `lastfm:${stableKey(`${artist}:${title}`)}`,
      album_id: mbid ? `lastfm-mbid:${mbid}` : `lastfm:${stableKey(`${artist}:${title}`)}`,
      artist,
      title,
      genres,
      tags,
      source: "lastfm",
      catalog_sources: ["lastfm"],
      source_evidence: [evidence],
      external_url: externalUrl,
      musicbrainz_release_group_id: null,
      musicbrainz_release_id: null,
      popularity_score: round(score),
    },
  };
}

function addCatalogCandidate(
  candidates: Map<string, CatalogCandidate>,
  candidate: CatalogCandidate,
  ownedIdentities: Set<string>,
) {
  const identityKeys = albumInputIdentityKeys(candidate.album);
  if (identityKeys.some((key) => ownedIdentities.has(key))) return;
  const key = identityKeys.find((value) => value.startsWith("rg:") || value.startsWith("release:")) ?? identityKeys.at(-1);
  if (!key) return;

  const existing = candidates.get(key);
  if (!existing || candidate.score > existing.score) {
    candidates.set(key, candidate);
    return;
  }

  existing.album.catalog_sources = unique([...existing.album.catalog_sources, ...candidate.album.catalog_sources]);
  existing.album.source_evidence = unique([...existing.album.source_evidence, ...candidate.album.source_evidence]);
  existing.album.tags = unique([...(existing.album.tags ?? []), ...(candidate.album.tags ?? [])]);
  existing.album.genres = unique([...(existing.album.genres ?? []), ...(candidate.album.genres ?? [])].map(normalizeGenre));
}

function pruneCatalogCandidates(candidates: Map<string, CatalogCandidate>, limit: number) {
  if (candidates.size <= limit) return;
  const keep = new Set(
    [...candidates.entries()]
      .sort((a, b) => b[1].score - a[1].score || a[1].album.artist!.localeCompare(b[1].album.artist!))
      .slice(0, limit)
      .map(([key]) => key),
  );
  for (const key of candidates.keys()) {
    if (!keep.has(key)) candidates.delete(key);
  }
}

async function buildMusicBrainzDumpCandidateCatalog(options: MusicBrainzDumpOptions) {
  const startedAt = new Date().toISOString();
  progress("musicbrainz-dump", "loading lookup tables", {
    core: options.corePath,
    derived: options.derivedPath ?? null,
    include_derived_tags: options.includeDerivedTags,
  });
  const primaryTypes = await loadDumpIdNameMap(options.corePath, "release_group_primary_type");
  progress("musicbrainz-dump", "loaded primary types", { count: primaryTypes.size });
  const secondaryTypes = await loadDumpIdNameMap(options.corePath, "release_group_secondary_type");
  progress("musicbrainz-dump", "loaded secondary types", { count: secondaryTypes.size });
  const artistCredits = await loadDumpIdNameMap(options.corePath, "artist_credit");
  progress("musicbrainz-dump", "loaded artist credits", { count: artistCredits.size });
  const firstReleaseDates = await loadReleaseGroupFirstReleaseDates(options.corePath);
  progress("musicbrainz-dump", "loaded first release dates", { count: firstReleaseDates.size });
  const secondaryTypesByReleaseGroup = await loadReleaseGroupSecondaryTypes(options.corePath, secondaryTypes);
  progress("musicbrainz-dump", "loaded release-group secondary type joins", { count: secondaryTypesByReleaseGroup.size });
  const tagsByReleaseGroup =
    options.derivedPath && options.includeDerivedTags
      ? await loadReleaseGroupTags(options.derivedPath, options.minTagCount)
      : new Map<string, { name: string; count: number }[]>();
  progress("musicbrainz-dump", "loaded release-group tags", { count: tagsByReleaseGroup.size });

  const output = options.output ?? path.join(SCRIPT_DIR, "mb-candidate-catalog.jsonl");
  const writer = createWriteStream(output, { encoding: "utf8" });
  let scanned = 0;
  let written = 0;
  const blockedSecondaryTypes = new Set(["Compilation", "Single", "EP", "Live", "Remix", "Soundtrack", "Interview", "DJ-mix"]);

  progress("musicbrainz-dump", "streaming release groups", { output });
  try {
    for await (const line of dumpTableLines(options.corePath, "release_group")) {
      const row = splitDumpLine(line);
      const releaseGroupId = row[0];
      const mbid = row[1];
      const title = row[2];
      const artistCreditId = row[3];
      const primaryTypeId = row[4];
      scanned += 1;

      const primaryType = primaryTypeId ? primaryTypes.get(primaryTypeId) : null;
      if (primaryType !== "Album") continue;

      const secondary = secondaryTypesByReleaseGroup.get(releaseGroupId ?? "") ?? [];
      if (secondary.some((type) => blockedSecondaryTypes.has(type))) continue;

      const artist = artistCreditId ? artistCredits.get(artistCreditId) : null;
      if (!releaseGroupId || !mbid || !title || !artist || sameArtist(artist, "Various Artists")) continue;

      const tagRows = tagsByReleaseGroup.get(releaseGroupId) ?? [];
      const tags = unique(tagRows.map((tag) => cleanExternalTag(tag.name)).filter((tag): tag is string => tag !== null)).slice(0, 16);
      const genres = unique(tags.map(normalizeGenre)).slice(0, 8);
      const releaseDate = firstReleaseDates.get(releaseGroupId) ?? null;
      const popularityScore = tagRows.length
        ? clamp(Math.log10(tagRows.reduce((sum, tag) => sum + tag.count, 0) + 1) / 3, 0, 1)
        : undefined;
      const album: ExternalCatalogAlbum = {
        catalog_id: `mb-rg:${mbid}`,
        album_id: `mb-rg:${mbid}`,
        artist,
        title,
        release_date: releaseDate,
        genres,
        tags,
        source: "musicbrainz-dump",
        catalog_sources: options.derivedPath && options.includeDerivedTags ? ["musicbrainz-core", "musicbrainz-derived"] : ["musicbrainz-core"],
        source_evidence: [
          `MusicBrainz release_group ${releaseGroupId}`,
          options.derivedPath && options.includeDerivedTags ? "MusicBrainz release_group_tag" : "MusicBrainz core dump only",
        ],
        musicbrainz_release_group_id: mbid,
        musicbrainz_release_id: null,
        external_url: `https://musicbrainz.org/release-group/${mbid}`,
        popularity_score: popularityScore,
      };
      writer.write(`${JSON.stringify(album)}\n`);
      written += 1;
      if (scanned % 100000 === 0) {
        progress("musicbrainz-dump", "release-group progress", { scanned, written });
      }
      if (options.maxRows > 0 && written >= options.maxRows) break;
    }
  } finally {
    await closeWriter(writer);
  }
  progress("musicbrainz-dump", "candidate catalog written", { scanned, written, output });

  return {
    ok: true,
    command: "musicbrainz-dump",
    generated_at: new Date().toISOString(),
    started_at: startedAt,
    output,
    scanned_release_groups: scanned,
    written_albums: written,
    core: options.corePath,
    derived: options.derivedPath ?? null,
    include_derived_tags: Boolean(options.derivedPath && options.includeDerivedTags),
    min_tag_count: options.minTagCount,
    max_rows: options.maxRows || null,
    data_license_notes: [
      "This JSONL file is a local operator artifact and should not be committed.",
      "MusicBrainz core data is CC0; MusicBrainz supplementary tag data is CC BY-NC-SA 3.0.",
    ],
  };
}

function albumInputIdentityKeys(album: AlbumInput): string[] {
  const artist = normalizeOptionalText(album.artist);
  const title = normalizeOptionalText(album.title);
  return [
    album.album_id ? `album:${album.album_id}` : null,
    album.musicbrainz_release_group_id ? `rg:${album.musicbrainz_release_group_id}` : null,
    album.musicbrainz_release_id ? `release:${album.musicbrainz_release_id}` : null,
    artist && title ? `text:${normalizeSearchText(artist)}:${normalizeSearchText(title)}` : null,
  ].filter((value): value is string => value !== null);
}

function uniqueAlbums(albums: AlbumRecord[]): AlbumRecord[] {
  const seen = new Set<string>();
  const uniqueRecords: AlbumRecord[] = [];
  for (const album of albums) {
    const keys = albumIdentityKeys(album);
    if (keys.some((key) => seen.has(key))) continue;
    uniqueRecords.push(album);
    for (const key of keys) seen.add(key);
  }
  return uniqueRecords;
}

async function fetchJson(
  url: URL,
  headers: Record<string, string>,
  minDelayMs: number,
  state: RateLimitState,
  attempt = 1,
): Promise<any> {
  const elapsed = Date.now() - state.lastRequestAt;
  if (elapsed < minDelayMs) {
    await sleep(minDelayMs - elapsed);
  }
  state.lastRequestAt = Date.now();

  const response = await fetch(url, { headers });
  if ((response.status === 429 || response.status === 503) && attempt <= 3) {
    const retryAfter = Number(response.headers.get("retry-after") ?? 0);
    await sleep(Math.max(minDelayMs, retryAfter * 1000 || minDelayMs * attempt));
    return fetchJson(url, headers, minDelayMs, state, attempt + 1);
  }
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}: ${await response.text()}`);
  }
  return response.json();
}

async function enrichMusicdImportPayloadWithTidal(payload: any, options: TidalOptions) {
  const recommendations = Array.isArray(payload.recommendations) ? payload.recommendations : [];
  const token = await getTidalAccessToken(options);
  const state: RateLimitState = { lastRequestAt: 0 };
  const matches: unknown[] = [];
  const warnings: string[] = [];
  let updated = 0;
  let skippedExisting = 0;
  let unmatched = 0;

  const enriched = [];
  for (const item of recommendations) {
    if (item.tidal_url) {
      skippedExisting += 1;
      enriched.push(item);
      continue;
    }
    const match = await resolveTidalAlbumMatch(
      {
        artist: item.suggested_artist,
        title: item.suggested_title,
        releaseDate: null,
      },
      token,
      options,
      state,
    ).catch((error) => {
      warnings.push(`${item.suggested_artist} - ${item.suggested_title}: ${String(error)}`);
      return null;
    });

    if (match && match.confidence >= options.minConfidence) {
      updated += 1;
      enriched.push({
        ...item,
        external_url: options.overwriteExternalUrl ? match.url : item.external_url ?? null,
        tidal_url: match.url,
      });
      matches.push({
        artist: item.suggested_artist,
        title: item.suggested_title,
        tidal_album_id: match.id,
        tidal_url: match.url,
        confidence: round(match.confidence),
        matched_artist: match.artist,
        matched_title: match.title,
        reason: match.reason,
      });
    } else {
      unmatched += 1;
      enriched.push(item);
      if (match) {
        matches.push({
          artist: item.suggested_artist,
          title: item.suggested_title,
          tidal_album_id: match.id,
          tidal_url: match.url,
          confidence: round(match.confidence),
          matched_artist: match.artist,
          matched_title: match.title,
          reason: `below threshold: ${match.reason}`,
        });
      }
    }
  }

  return {
    ...payload,
    recommendations: enriched,
    tidal_enrichment: {
      enabled: true,
      generated_at: new Date().toISOString(),
      country_code: options.countryCode,
      min_confidence: options.minConfidence,
      overwrite_external_url: options.overwriteExternalUrl,
      updated,
      skipped_existing: skippedExisting,
      unmatched,
      warnings,
      matches,
    },
  };
}

async function enrichRecommendationResultWithTidal(result: any, options: TidalOptions) {
  const token = await getTidalAccessToken(options);
  const state: RateLimitState = { lastRequestAt: 0 };
  const warnings: string[] = [];

  async function enrichList(items: any[] | undefined): Promise<any[]> {
    const list = Array.isArray(items) ? items : [];
    const enriched = [];
    for (const item of list) {
      if (item.tidal_url) {
        enriched.push(item);
        continue;
      }
      const match = await resolveTidalAlbumMatch(
        {
          artist: item.artist,
          title: item.title,
          releaseDate: item.year ? String(item.year) : null,
        },
        token,
        options,
        state,
      ).catch((error) => {
        warnings.push(`${item.artist} - ${item.title}: ${String(error)}`);
        return null;
      });
      enriched.push(
        match && match.confidence >= options.minConfidence
          ? {
              ...item,
              external_url: options.overwriteExternalUrl ? match.url : item.external_url ?? null,
              tidal_url: match.url,
              tidal_match: {
                album_id: match.id,
                confidence: round(match.confidence),
                artist: match.artist,
                title: match.title,
                release_date: match.releaseDate,
                reason: match.reason,
              },
            }
          : item,
      );
    }
    return enriched;
  }

  return {
    ...result,
    owned_recommendations: await enrichList(result.owned_recommendations),
    discovery_recommendations: await enrichList(result.discovery_recommendations),
    tidal_enrichment: {
      enabled: true,
      country_code: options.countryCode,
      min_confidence: options.minConfidence,
      warnings,
    },
  };
}

async function getTidalAccessToken(options: TidalOptions): Promise<TidalToken> {
  if (!options.clientId || !options.clientSecret) {
    throw new Error("TIDAL enrichment requires TIDAL_CLIENT_ID and TIDAL_CLIENT_SECRET");
  }
  const credentials = Buffer.from(`${options.clientId}:${options.clientSecret}`, "utf8").toString("base64");
  const response = await fetch(options.tokenUrl, {
    method: "POST",
    headers: {
      "authorization": `Basic ${credentials}`,
      "content-type": "application/x-www-form-urlencoded",
      "accept": "application/json",
    },
    body: new URLSearchParams({ grant_type: "client_credentials" }).toString(),
  });
  if (!response.ok) {
    throw new Error(`TIDAL token request failed: ${response.status} ${response.statusText}: ${await response.text()}`);
  }
  const body: any = await response.json();
  const accessToken = normalizeOptionalText(body.access_token);
  if (!accessToken) throw new Error("TIDAL token response did not include access_token");
  const expiresIn = typeof body.expires_in === "number" ? body.expires_in : 3600;
  return {
    accessToken,
    expiresAt: Date.now() + expiresIn * 1000,
  };
}

async function resolveTidalAlbumMatch(
  target: { artist: string; title: string; releaseDate: string | null },
  token: TidalToken,
  options: TidalOptions,
  state: RateLimitState,
): Promise<TidalAlbumMatch | null> {
  const cacheKey = tidalMatchCacheKey(target, options);
  const cached = TIDAL_MATCH_CACHE.get(cacheKey);
  if (cached) return cached;
  const promise = resolveTidalAlbumMatchUncached(target, token, options, state).catch((error) => {
    TIDAL_MATCH_CACHE.delete(cacheKey);
    throw error;
  });
  TIDAL_MATCH_CACHE.set(cacheKey, promise);
  return promise;
}

async function resolveTidalAlbumMatchUncached(
  target: { artist: string; title: string; releaseDate: string | null },
  token: TidalToken,
  options: TidalOptions,
  state: RateLimitState,
): Promise<TidalAlbumMatch | null> {
  if (token.expiresAt <= Date.now() + 30_000) {
    throw new Error("TIDAL token expired during enrichment; rerun the command to refresh it");
  }
  const query = `${target.artist} ${target.title}`;
  const searchUrl = new URL(`${options.apiBaseUrl.replace(/\/$/, "")}/searchResults/${encodeURIComponent(query)}`);
  searchUrl.searchParams.set("include", "albums");
  searchUrl.searchParams.set("explicitFilter", "INCLUDE");
  searchUrl.searchParams.set("countryCode", options.countryCode);

  const search = await fetchTidalJson(searchUrl, token, options, state);
  const albumIds = tidalSearchAlbumIds(search).slice(0, options.maxCandidates);
  const includedAlbums = tidalIncludedResources(search, "albums");
  const candidates = [];
  for (const albumId of albumIds) {
    const detail = await fetchTidalAlbum(albumId, token, options, state).catch(() => includedAlbums.get(albumId));
    if (detail) candidates.push(detail);
  }
  for (const album of includedAlbums.values()) {
    if (candidates.length >= options.maxCandidates) break;
    if (!candidates.some((candidate) => candidate.id === album.id)) candidates.push(album);
  }

  const scored = candidates
    .map((album) => scoreTidalAlbumMatch(target, album))
    .sort((a, b) => b.confidence - a.confidence);
  return scored[0] ?? null;
}

function tidalMatchCacheKey(
  target: { artist: string; title: string; releaseDate: string | null },
  options: TidalOptions,
): string {
  return [
    options.apiBaseUrl.replace(/\/$/, ""),
    options.countryCode,
    options.maxCandidates,
    normalizeSearchText(target.artist),
    normalizeSearchText(target.title),
    target.releaseDate ?? "",
  ].join("|");
}

async function fetchTidalAlbum(albumId: string, token: TidalToken, options: TidalOptions, state: RateLimitState): Promise<any | null> {
  const url = new URL(`${options.apiBaseUrl.replace(/\/$/, "")}/albums/${encodeURIComponent(albumId)}`);
  url.searchParams.set("include", "artists");
  url.searchParams.set("countryCode", options.countryCode);
  const body = await fetchTidalJson(url, token, options, state);
  const album = body.data?.type === "albums" ? body.data : null;
  if (!album) return null;
  const artists = tidalIncludedResources(body, "artists");
  return {
    ...album,
    tidal_artist_names: tidalRelationshipIds(album, "artists")
      .map((id) => normalizeOptionalText(artists.get(id)?.attributes?.name))
      .filter((name): name is string => name !== null),
  };
}

async function fetchTidalJson(url: URL, token: TidalToken, options: TidalOptions, state: RateLimitState): Promise<any> {
  return fetchJson(
    url,
    {
      "accept": "application/vnd.api+json",
      "authorization": `Bearer ${token.accessToken}`,
    },
    options.delayMs,
    state,
  );
}

function tidalSearchAlbumIds(body: any): string[] {
  const relationshipIds = tidalRelationshipIds(body.data, "albums");
  if (relationshipIds.length) return unique(relationshipIds);
  return [...tidalIncludedResources(body, "albums").keys()];
}

function tidalRelationshipIds(resource: any, name: string): string[] {
  const data = resource?.relationships?.[name]?.data;
  if (Array.isArray(data)) {
    return data.map((item) => normalizeOptionalText(item?.id)).filter((id): id is string => id !== null);
  }
  const id = normalizeOptionalText(data?.id);
  return id ? [id] : [];
}

function tidalIncludedResources(body: any, type: string): Map<string, any> {
  const resources = new Map<string, any>();
  const included = Array.isArray(body.included) ? body.included : [];
  for (const resource of included) {
    if (resource?.type !== type) continue;
    const id = normalizeOptionalText(resource.id);
    if (id) resources.set(id, resource);
  }
  return resources;
}

function scoreTidalAlbumMatch(target: { artist: string; title: string; releaseDate: string | null }, album: any): TidalAlbumMatch {
  const attributes = album.attributes ?? {};
  const title = normalizeOptionalText(attributes.title) ?? "";
  const version = normalizeOptionalText(attributes.version);
  const fullTitle = version ? `${title} ${version}` : title;
  const artistNames = Array.isArray(album.tidal_artist_names) ? album.tidal_artist_names : [];
  const artist = artistNames.length ? artistNames.join(" & ") : null;
  const titleScore = textMatchScore(target.title, fullTitle);
  const artistScore = artist ? Math.max(...artistNames.map((name: string) => textMatchScore(target.artist, name))) : 0.65;
  const targetYear = parseYear(target.releaseDate);
  const releaseDate = normalizeOptionalText(attributes.releaseDate);
  const candidateYear = parseYear(releaseDate);
  const yearScore =
    targetYear && candidateYear
      ? Math.max(0, 1 - Math.abs(targetYear - candidateYear) / 5)
      : targetYear || candidateYear
        ? 0.45
        : 0.6;
  const albumType = normalizeOptionalText(attributes.albumType ?? attributes.type);
  const albumTypeScore = albumType === "ALBUM" ? 1 : albumType === "EP" ? 0.45 : 0.2;
  const popularity = typeof attributes.popularity === "number" ? clamp(attributes.popularity, 0, 1) : 0.5;
  const confidence = clamp(
    titleScore * 0.5 + artistScore * 0.32 + yearScore * 0.08 + albumTypeScore * 0.06 + popularity * 0.04,
    0,
    1,
  );
  return {
    id: String(album.id),
    url: tidalAlbumUrl(String(album.id)),
    confidence,
    title,
    artist,
    releaseDate,
    albumType,
    reason: `title ${round(titleScore)}, artist ${round(artistScore)}, year ${round(yearScore)}, type ${round(albumTypeScore)}`,
  };
}

function textMatchScore(left: string, right: string): number {
  const normalizedLeft = normalizeSearchText(left);
  const normalizedRight = normalizeSearchText(right);
  if (!normalizedLeft || !normalizedRight) return 0;
  if (normalizedLeft === normalizedRight) return 1;
  if (normalizedLeft.includes(normalizedRight) || normalizedRight.includes(normalizedLeft)) return 0.9;
  return jaccard(tokenize(normalizedLeft), tokenize(normalizedRight));
}

function tidalAlbumUrl(albumId: string): string {
  return `https://tidal.com/browse/album/${encodeURIComponent(albumId)}`;
}

async function loadAlbums(path: string, owned: boolean): Promise<AlbumRecord[]> {
  const inputs = await loadAlbumInputs(path);
  return inputs.map((input, index) => normalizeAlbum(input, owned, basename(path), index));
}

async function loadAlbumInputs(path: string): Promise<AlbumInput[]> {
  if (path.endsWith(".jsonl") || path.endsWith(".ndjson")) {
    const inputs: AlbumInput[] = [];
    for await (const input of readAlbumInputs(path)) {
      inputs.push(input);
    }
    return inputs;
  }

  const raw = JSON.parse(await readFile(path, "utf8"));
  const inputs: AlbumInput[] = Array.isArray(raw) ? raw : raw.seeds ?? raw.albums ?? [];
  if (!Array.isArray(inputs)) {
    throw new Error(`${path} must contain an array, { "seeds": [...] }, or { "albums": [...] }`);
  }
  return inputs;
}

async function* readAlbumInputs(path: string): AsyncIterable<AlbumInput> {
  if (path.endsWith(".jsonl") || path.endsWith(".ndjson")) {
    for await (const line of readLines(path)) {
      const trimmed = line.trim();
      if (!trimmed) continue;
      yield JSON.parse(trimmed) as AlbumInput;
    }
    return;
  }
  for (const input of await loadAlbumInputs(path)) {
    yield input;
  }
}

function normalizeAlbum(input: AlbumInput, owned: boolean, sourceName: string, index: number): AlbumRecord {
  const artist = requiredText(input.artist, `album ${index + 1} artist`);
  const title = requiredText(input.title, `album ${index + 1} title`);
  const releaseDate = normalizeOptionalText(input.release_date ?? input.year);
  const year = parseYear(input.year ?? input.release_date);
  const genres = unique((input.genres ?? []).map(normalizeGenre).filter(Boolean));
  const tags = unique([...(input.tags ?? []), ...(input.moods ?? [])].map(cleanPhrase).filter(Boolean));
  const descriptors = unique([
    ...(input.style_descriptors ?? []),
    ...genres.flatMap(inferDescriptorsForGenre),
    ...tags.flatMap(inferDescriptorsForTag),
  ]);
  const description = normalizeOptionalText(input.description);
  const decade = year === null ? null : `${Math.floor(year / 10) * 10}s`;
  const albumId =
    normalizeOptionalText(input.album_id ?? input.id) ??
    stableKey(`${owned ? "owned" : "external"}:${artist}:${title}:${releaseDate ?? index}`);

  const record: AlbumRecord = {
    albumId,
    artist,
    title,
    releaseDate,
    year,
    decade,
    genres,
    tags,
    descriptors,
    description,
    owned,
    source: normalizeOptionalText(input.source) ?? sourceName,
    musicbrainzReleaseId: normalizeOptionalText(input.musicbrainz_release_id),
    musicbrainzReleaseGroupId: normalizeOptionalText(input.musicbrainz_release_group_id),
    artworkUrl: normalizeOptionalText(input.artwork_url),
    externalUrl: normalizeOptionalText(input.external_url),
    tidalUrl: normalizeOptionalText(input.tidal_url),
    trackCount: typeof input.track_count === "number" ? input.track_count : null,
    normalizedText: "",
    tokens: [],
    vector: new Map(),
  };
  record.normalizedText = normalizedAlbumText(record);
  record.tokens = tokenize(record.normalizedText);
  return record;
}

function normalizedAlbumText(album: AlbumRecord): string {
  const parts = [
    `Schema: ${TEXT_SCHEMA_VERSION}.`,
    `Album: ${album.title}.`,
    `Artist: ${album.artist}.`,
    album.year ? `Year: ${album.year}.` : null,
    album.decade ? `Era: ${album.decade}.` : null,
    album.genres.length ? `Genres: ${album.genres.join(", ")}.` : null,
    album.tags.length ? `Tags: ${album.tags.join(", ")}.` : null,
    album.descriptors.length ? `Style: ${album.descriptors.join(", ")}.` : null,
    album.trackCount ? `Album length cue: ${album.trackCount} tracks.` : null,
    album.description ? `Description: ${album.description}.` : null,
  ];
  return parts.filter(Boolean).join(" ");
}

function applyLocalVectorIndex(albums: AlbumRecord[]): string {
  buildVectorIndex(albums);
  return LOCAL_EMBEDDING_MODEL;
}

async function applyEmbeddingIndex(albums: AlbumRecord[], embeddingPath: string): Promise<string> {
  progress("embeddings", "loading embedding index", { path: embeddingPath, albums: albums.length });
  const index = await readEmbeddingIndex(embeddingPath);
  if (index.embedding_schema_version !== "album_embeddings_v1") {
    throw new Error(`${embeddingPath} is not an album_embeddings_v1 file`);
  }
  if (index.text_schema_version !== TEXT_SCHEMA_VERSION) {
    throw new Error(
      `${embeddingPath} was built for text schema ${index.text_schema_version}, but this script uses ${TEXT_SCHEMA_VERSION}`,
    );
  }

  const embeddingsByAlbumId = new Map(index.embeddings.map((embedding) => [embedding.album_id, embedding]));
  const missing: string[] = [];
  const stale: string[] = [];
  for (const album of albums) {
    const embedding = embeddingsByAlbumId.get(album.albumId);
    if (!embedding) {
      missing.push(`${album.artist} - ${album.title} (${album.albumId})`);
      continue;
    }
    const textHash = stableKey(album.normalizedText);
    if (embedding.text_hash !== textHash) {
      stale.push(`${album.artist} - ${album.title} (${album.albumId})`);
      continue;
    }
    album.denseVector = Float32Array.from(embedding.embedding);
  }

  if (missing.length || stale.length) {
    const details = [
      missing.length ? `missing ${missing.length} album embedding(s): ${missing.slice(0, 5).join("; ")}` : null,
      stale.length ? `stale ${stale.length} album embedding(s): ${stale.slice(0, 5).join("; ")}` : null,
    ]
      .filter(Boolean)
      .join(" / ");
    throw new Error(`Embedding index ${embeddingPath} is incomplete for this recommendation run: ${details}`);
  }

  progress("embeddings", "embedding index applied", { model: index.embedding_model, embeddings: index.embeddings.length });
  return index.embedding_model;
}

async function readEmbeddingIndex(embeddingPath: string): Promise<EmbeddingIndex> {
  const fileStat = await stat(embeddingPath);
  progress("embeddings", "embedding index file read started", { path: embeddingPath, bytes: fileStat.size });
  const raw = await readFile(embeddingPath, "utf8");
  progress("embeddings", "embedding index file read complete", {
    path: embeddingPath,
    bytes: Buffer.byteLength(raw, "utf8"),
    hash: stableKey(raw),
  });
  try {
    const index = JSON.parse(raw) as EmbeddingIndex;
    if (!Array.isArray(index.embeddings)) {
      throw new Error("missing embeddings array");
    }
    return index;
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    progress("embeddings", "embedding index parse failed", {
      path: embeddingPath,
      reason,
      ...embeddingTextDiagnostics(raw),
    });
    throw new Error(
      `Embedding index ${embeddingPath} is not valid JSON. The previous embedding build may have been interrupted; rerun the embed/run step with --force or delete the file so it can be rebuilt. Parser error: ${reason}`,
    );
  }
}

async function applyEmbeddingDatabaseIndex(
  albums: AlbumRecord[],
  databasePath: string,
  embeddingModel?: string,
  embeddingBaseUrl?: string,
  embeddingDimensions?: number,
): Promise<string> {
  const model = embeddingModel ?? "text-embedding-3-small";
  const baseUrl = safeBaseUrl(embeddingBaseUrl ?? process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1");
  progress("embeddings", "loading embedding database", {
    database: databasePath,
    albums: albums.length,
    model,
    base_url: baseUrl,
    dimensions: embeddingDimensions ?? null,
  });
  const totalTimer = timerStart();
  const openTimer = timerStart();
  const store = await openRecommenderDatabase(databasePath);
  progress("embeddings", "embedding database opened", { database: databasePath, duration_ms: timerMs(openTimer) });
  try {
    const missing: string[] = [];
    const stale: string[] = [];
    let dimensions: number | null = null;
    let lookupMs = 0;
    let hashMs = 0;
    let decodeMs = 0;
    let vectorMs = 0;
    for (const album of albums) {
      const lookupTimer = timerStart();
      const row = getStoredEmbeddingRow(store, album.albumId, model, baseUrl);
      lookupMs += timerMs(lookupTimer);
      if (!row) {
        missing.push(`${album.artist} - ${album.title} (${album.albumId})`);
        continue;
      }
      const hashTimer = timerStart();
      const textHash = stableKey(album.normalizedText);
      hashMs += timerMs(hashTimer);
      if (row.text_hash !== textHash) {
        stale.push(`${album.artist} - ${album.title} (${album.albumId})`);
        continue;
      }
      const decodeTimer = timerStart();
      const vector = blobToVector(row.embedding);
      decodeMs += timerMs(decodeTimer);
      if (embeddingDimensions && vector.length !== embeddingDimensions) {
        stale.push(`${album.artist} - ${album.title} (${album.albumId}, stored dimensions ${vector.length})`);
        continue;
      }
      dimensions = dimensions ?? vector.length;
      const vectorTimer = timerStart();
      album.denseVector = Float32Array.from(vector);
      vectorMs += timerMs(vectorTimer);
    }

    if (missing.length || stale.length) {
      const details = [
        missing.length ? `missing ${missing.length} album embedding(s): ${missing.slice(0, 5).join("; ")}` : null,
        stale.length ? `stale ${stale.length} album embedding(s): ${stale.slice(0, 5).join("; ")}` : null,
      ]
        .filter(Boolean)
        .join(" / ");
      throw new Error(`Embedding database ${databasePath} is incomplete for this recommendation run: ${details}`);
    }

    progress("embeddings", "embedding database applied", {
      database: databasePath,
      model,
      base_url: baseUrl,
      embeddings: albums.length,
      dimensions,
      duration_ms: timerMs(totalTimer),
      lookup_ms: round(lookupMs),
      hash_ms: round(hashMs),
      decode_ms: round(decodeMs),
      vector_convert_ms: round(vectorMs),
    });
    return model;
  } finally {
    store.close();
  }
}

function buildVectorIndex(albums: AlbumRecord[]) {
  const documentFrequency = new Map<string, number>();
  for (const album of albums) {
    for (const token of new Set(album.tokens)) {
      documentFrequency.set(token, (documentFrequency.get(token) ?? 0) + 1);
    }
  }

  const vocabulary = new Map<string, number>();
  for (const token of documentFrequency.keys()) {
    vocabulary.set(token, vocabulary.size);
  }

  for (const album of albums) {
    const termFrequency = new Map<string, number>();
    for (const token of album.tokens) {
      termFrequency.set(token, (termFrequency.get(token) ?? 0) + 1);
    }

    const vector = new Map<number, number>();
    for (const [token, count] of termFrequency) {
      const index = vocabulary.get(token);
      if (index === undefined) continue;
      const idf = Math.log((albums.length + 1) / ((documentFrequency.get(token) ?? 0) + 1)) + 1;
      vector.set(index, (count / album.tokens.length) * idf);
    }
    normalizeVector(vector);
    album.vector = vector;
  }
}

function buildCollectionProfile(collection: AlbumRecord[], embeddingModel = LOCAL_EMBEDDING_MODEL): CollectionProfile {
  const topGenres = topCounts(collection.flatMap((album) => album.genres), 12);
  const topArtists = topCounts(collection.map((album) => album.artist), 12);
  const eraDistribution = topCounts(
    collection.map((album) => album.decade ?? "unknown era"),
    12,
    (a, b) => eraSortKey(a[0]) - eraSortKey(b[0]) || b[1] - a[1],
  );
  const styleDescriptors = topCounts(collection.flatMap((album) => album.descriptors), 16);
  const representativeAlbums = representativeAlbumsFor(collection, 10).map(publicAlbum);

  return {
    text_schema_version: TEXT_SCHEMA_VERSION,
    embedding_model: embeddingModel,
    album_count: collection.length,
    top_genres: topGenres,
    top_artists: topArtists,
    era_distribution: eraDistribution,
    style_descriptors: styleDescriptors,
    popularity_obscurity_tendency: popularityTendency(collection, topArtists),
    representative_albums: representativeAlbums,
  };
}

function representativeAlbumsFor(collection: AlbumRecord[], count: number): AlbumRecord[] {
  const centroid = new Map<number, number>();
  for (const album of collection) {
    for (const [index, value] of album.vector) {
      centroid.set(index, (centroid.get(index) ?? 0) + value);
    }
  }
  normalizeVector(centroid);
  return collection
    .map((album) => ({ album, score: cosine(album.vector, centroid) }))
    .sort((a, b) => b.score - a.score)
    .filter((item, index, items) => items.findIndex((other) => other.album.artist === item.album.artist) === index)
    .slice(0, count)
    .map((item) => item.album);
}

function retrieve(
  seed: AlbumRecord,
  albums: AlbumRecord[],
  profile: CollectionProfile,
  limit: number,
  ownedPool: boolean,
): Candidate[] {
  const timer = timerStart();
  const scoringContext = buildRetrievalScoringContext(seed, profile);
  const candidates = shortlistRetrievalAlbums(seed, albums, limit, ownedPool);
  const scored: Candidate[] = [];
  for (const album of candidates.albums) {
    if (album.albumId === seed.albumId) continue;
    pushTopCandidate(scored, scoreCandidate(seed, album, scoringContext, ownedPool), limit);
  }
  const scoreMs = timerMs(timer);
  const finalizeTimer = timerStart();
  const result = scored.sort((a, b) => b.score - a.score);
  for (const candidate of result) {
    candidate.rationale = explainCandidate(
      seed,
      candidate.album,
      candidate.embeddingSimilarity,
      candidate.genreAffinity,
      candidate.eraCompatibility,
    );
  }
  progress("recommendations", "retrieval scoring details", {
    seed_album_id: seed.albumId,
    pool: ownedPool ? "owned" : "discovery",
    candidates: albums.length,
    scored: Math.max(0, candidates.albums.length - (candidates.albums.some((album) => album.albumId === seed.albumId) ? 1 : 0)),
    shortlisted: candidates.shortlisted ? candidates.albums.length : null,
    shortlist_ms: candidates.shortlistMs,
    shortlist_dimensions: candidates.shortlisted ? candidates.dimensions : null,
    returned: result.length,
    score_ms: scoreMs,
    finalize_ms: timerMs(finalizeTimer),
  });
  return result;
}

function shortlistRetrievalAlbums(
  seed: AlbumRecord,
  albums: AlbumRecord[],
  limit: number,
  ownedPool: boolean,
): { albums: AlbumRecord[]; shortlisted: boolean; shortlistMs: number; dimensions: number | null } {
  if (
    ownedPool ||
    albums.length < RETRIEVAL_SHORTLIST_THRESHOLD ||
    !seed.denseVector ||
    seed.denseVector.length <= RETRIEVAL_SHORTLIST_DIMENSIONS
  ) {
    return { albums, shortlisted: false, shortlistMs: 0, dimensions: null };
  }
  const shortlistTimer = timerStart();
  const shortlistLimit = Math.min(
    albums.length,
    Math.max(limit * RETRIEVAL_SHORTLIST_MULTIPLIER, RETRIEVAL_SHORTLIST_MIN),
  );
  const shortlist: { album: AlbumRecord; score: number }[] = [];
  for (const album of albums) {
    if (album.albumId === seed.albumId || !album.denseVector) continue;
    shortlist.push({
      album,
      score: partialDenseCosine(seed.denseVector, album.denseVector, RETRIEVAL_SHORTLIST_DIMENSIONS),
    });
  }
  return {
    albums: shortlist
      .sort((a, b) => b.score - a.score)
      .slice(0, shortlistLimit)
      .map((item) => item.album),
    shortlisted: true,
    shortlistMs: timerMs(shortlistTimer),
    dimensions: RETRIEVAL_SHORTLIST_DIMENSIONS,
  };
}

function buildRetrievalScoringContext(seed: AlbumRecord, profile: CollectionProfile): RetrievalScoringContext {
  return {
    profileGenreSet: new Set(profile.top_genres.slice(0, 8).map(([genre]) => genre)),
    seedGenreSignals: genreSignalSet(seed),
    seedTagSignals: [...seed.tags, ...seed.descriptors].map(normalizeSignal),
    seedDescriptorSet: new Set(seed.descriptors),
  };
}

function pushTopCandidate(candidates: Candidate[], candidate: Candidate, limit: number) {
  if (limit <= 0) return;
  if (candidates.length < limit) {
    candidates.push(candidate);
    return;
  }
  let weakestIndex = 0;
  let weakestScore = candidates[0].score;
  for (let index = 1; index < candidates.length; index += 1) {
    if (candidates[index].score < weakestScore) {
      weakestScore = candidates[index].score;
      weakestIndex = index;
    }
  }
  if (candidate.score > weakestScore) {
    candidates[weakestIndex] = candidate;
  }
}

function scoreCandidate(
  seed: AlbumRecord,
  album: AlbumRecord,
  context: RetrievalScoringContext,
  ownedPool: boolean,
): Candidate {
  const embeddingSimilarity = albumSimilarity(seed, album);
  const genreAffinity = affinityWithContext(context, album);
  const eraCompatibility = seed.year && album.year ? Math.max(0, 1 - Math.abs(seed.year - album.year) / 45) : 0.35;
  const profileFit = album.genres.some((genre) => context.profileGenreSet.has(genre)) ? 0.04 : 0;
  const exploratoryBonus = !ownedPool && genreAffinity < 0.2 ? 0.03 : 0;
  const artistPenalty = sameArtist(seed.artist, album.artist) ? 0.25 : 0;
  const diversityBonus = album.descriptors.some((descriptor) => context.seedDescriptorSet.has(descriptor)) ? 0.04 : 0;
  const seedTieBreak = ownedPool ? 0 : stableScore(`${seed.albumId}:${album.albumId}`) * 0.025;
  const score =
    embeddingSimilarity * 0.72 +
    genreAffinity * 0.2 +
    eraCompatibility * 0.06 +
    profileFit +
    exploratoryBonus +
    diversityBonus +
    seedTieBreak -
    artistPenalty;

  return {
    album,
    score,
    embeddingSimilarity,
    genreAffinity,
    eraCompatibility,
    diversityBonus: profileFit + exploratoryBonus + diversityBonus,
    artistPenalty,
    rationale: "",
  };
}

function affinity(seed: AlbumRecord, album: AlbumRecord): number {
  return affinityWithContext(
    {
      profileGenreSet: new Set(),
      seedGenreSignals: genreSignalSet(seed),
      seedTagSignals: [...seed.tags, ...seed.descriptors].map(normalizeSignal),
      seedDescriptorSet: new Set(seed.descriptors),
    },
    album,
  );
}

function affinityWithContext(context: RetrievalScoringContext, album: AlbumRecord): number {
  const albumGenres = genreSignalSet(album);
  const genreScore = jaccard([...context.seedGenreSignals], [...albumGenres]);
  const tagScore = jaccard(
    context.seedTagSignals,
    [...album.tags, ...album.descriptors].map(normalizeSignal),
  );
  return Math.max(genreScore, tagScore * 0.75);
}

function selectDiscoveryBatch(
  seed: AlbumRecord,
  discoveryPool: Candidate[],
  owned: Candidate[],
  options: RecommendOptions,
): Candidate[] {
  const blockedArtists = new Set([seed.artist, ...owned.map((candidate) => candidate.album.artist)]);
  const recentTarget = Math.min(options.recentDiscoveryCount, options.discoveryCount);
  const recentCandidates = discoveryPool.filter((candidate) => isRecentAlbum(candidate.album));
  const recent = diversify(recentCandidates, recentTarget, blockedArtists);
  const recentIdentities = new Set(recent.flatMap((candidate) => albumIdentityKeys(candidate.album)));
  const updatedBlockedArtists = new Set([...blockedArtists, ...recent.map((candidate) => candidate.album.artist)]);
  const remainingPool = discoveryPool.filter(
    (candidate) => !albumIdentityKeys(candidate.album).some((identity) => recentIdentities.has(identity)),
  );
  const remaining = diversify(remainingPool, options.discoveryCount - recent.length, updatedBlockedArtists);
  return [...recent, ...remaining].slice(0, options.discoveryCount);
}

function isRecentAlbum(album: AlbumRecord, years = recentYears()): boolean {
  return album.year !== null && years.includes(album.year);
}

function recentYears(): number[] {
  const currentYear = new Date().getFullYear();
  return [currentYear, currentYear - 1];
}

function genreSignalSet(album: AlbumRecord): Set<string> {
  return new Set(
    [...album.genres, ...album.tags]
      .flatMap((value) => [normalizeGenre(value), genreFamily(value)])
      .map(normalizeSignal)
      .filter(Boolean),
  );
}

function diversify(pool: Candidate[], count: number, blockedArtists: Set<string>): Candidate[] {
  if (count <= 0) return [];

  const selected: Candidate[] = [];
  const artistCounts = new Map<string, number>();
  const descriptorCounts = new Map<string, number>();
  const selectedIdentities = new Set<string>();

  for (const candidate of pool) {
    const candidateIdentities = albumIdentityKeys(candidate.album);
    if (candidateIdentities.some((key) => selectedIdentities.has(key))) continue;

    const artistKey = candidate.album.artist.toLocaleLowerCase();
    const artistAlreadyBlocked = blockedArtists.has(candidate.album.artist);
    const artistCount = artistCounts.get(artistKey) ?? 0;
    if (artistAlreadyBlocked && selected.length >= Math.max(1, count - 1)) continue;
    if (artistCount >= 1) continue;

    const descriptorOverlap = candidate.album.descriptors.filter(
      (descriptor) => (descriptorCounts.get(descriptor) ?? 0) > 0,
    ).length;
    if (descriptorOverlap >= 3 && selected.length < pool.length - 1) continue;

    selected.push(candidate);
    for (const key of candidateIdentities) selectedIdentities.add(key);
    artistCounts.set(artistKey, artistCount + 1);
    for (const descriptor of candidate.album.descriptors) {
      descriptorCounts.set(descriptor, (descriptorCounts.get(descriptor) ?? 0) + 1);
    }
    if (selected.length >= count) return selected;
  }

  for (const candidate of pool) {
    const candidateIdentities = albumIdentityKeys(candidate.album);
    if (candidateIdentities.some((key) => selectedIdentities.has(key))) continue;
    if (!selected.includes(candidate)) selected.push(candidate);
    for (const key of candidateIdentities) selectedIdentities.add(key);
    if (selected.length >= count) break;
  }
  return selected;
}

async function rerankWithLmStudio(result: any, baseUrl: string, model: string) {
  const candidates = [...result.owned_recommendations, ...result.discovery_recommendations].map((candidate) => ({
    artist: candidate.artist,
    title: candidate.title,
    owned: candidate.owned,
    score: candidate.score,
    genres: candidate.genres,
    rationale: candidate.rationale,
  }));
  const response = await fetch(`${baseUrl.replace(/\/$/, "")}/chat/completions`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({
      model,
      temperature: 0.2,
      response_format: { type: "json_schema", json_schema: {schema:
  {
    "$schema": "http://json-schema.org/draft-07/schema#",
      "title": "MusicRecommendationResponse",
      "description": "A response containing two lists of music recommendations: discovery and owned.",
      "type": "object",
      "properties": {
    "discovery_recommendations": {
      "type": "array",
          "description": "List of recommended albums the user does not own yet.",
          "items": {
        "$ref": "#/definitions/recommendation"
      }
    },
    "owned_recommendations": {
      "type": "array",
          "description": "List of albums the user already owns (for context or re-recommendation).",
          "items": {
        "$ref": "#/definitions/recommendation"
      }
    }
  },
    "required": [
    "discovery_recommendations",
    "owned_recommendations"
  ],
      "definitions": {
    "recommendation": {
      "type": "object",
          "properties": {
        "album_id": {
          "type": ["string", "null"],
              "description": "Internal ID for the album (e.g., UUID or hash)."
        },
        "artist": {
          "type": "string"
        },
        "title": {
          "type": "string"
        },
        "year": {
          "type": "integer",
              "minimum": 0,
              "maximum": 9999
        },
        "genres": {
          "type": "array",
              "items": {
            "type": "string"
          }
        },
        "owned": {
          "type": "boolean"
        },
        "score": {
          "type": "number",
              "minimum": 0,
              "maximum": 1
        },
        "signals": {
          "type": "object",
              "properties": {
            "embedding_similarity": {
              "type": "number"
            },
            "genre_affinity": {
              "type": "integer"
            },
            "era_compatibility": {
              "type": "integer"
            },
            "diversity_bonus": {
              "type": "number"
            },
            "artist_penalty": {
              "type": "integer"
            }
          },
          "required": [
            "embedding_similarity",
            "genre_affinity",
            "era_compatibility",
            "diversity_bonus",
            "artist_penalty"
          ]
        },
        "rationale": {
          "type": "string"
        },
        "musicbrainz_release_id": {
          "type": ["string", "null"]
        },
        "musicbrainz_release_group_id": {
          "type": ["string", "null"]
        },
        "artwork_url": {
          "type": ["string", "null"]
        },
        "external_url": {
          "type": ["string", "null"]
        },
        "tidal_url": {
          "type": ["string", "null"]
        }
      },
      "required": [
        "album_id",
        "artist",
        "title",
        "year",
        "genres",
        "owned",
        "score",
        "signals",
        "rationale"
      ]
    }
  }
  }}

 },
      messages: [
        {
          role: "system",
          content:
            "You rerank music album recommendations. Return compact JSON with owned_recommendations and discovery_recommendations arrays. Preserve artist/title/owned fields and improve the rationale.",
        },
        {
          role: "user",
          content: JSON.stringify({
            seed_album: result.seed_album,
            collection_profile: result.collection_profile,
            candidates,
          }),
        },
      ],
    }),
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}: ${await response.text()}`);
  }
  const body: any = await response.json();
  const content = body.choices?.[0]?.message?.content;
  if (!content) throw new Error("LM Studio returned no message content");
  return {
    ...result,
    llm_rerank_model: model,
    llm_rerank: JSON.parse(content),
  };
}

function toMusicdImportPayload(seed: AlbumRecord, result: any) {
  const recommendationsByKey = new Map<string, any>();
  for (const item of [
    ...(result.owned_recommendations ?? []),
    ...(result.discovery_recommendations ?? []),
    ...((result.llm_rerank?.owned_recommendations ?? []) as any[]),
    ...((result.llm_rerank?.discovery_recommendations ?? []) as any[]),
  ]) {
    const key =
      item.musicbrainz_release_group_id ??
      item.musicbrainz_release_id ??
      `${normalizeSearchText(item.artist ?? "")}:${normalizeSearchText(item.title ?? "")}`;
    if (!recommendationsByKey.has(key)) {
      recommendationsByKey.set(key, item);
    }
  }

  return {
    source: result.llm_rerank_model ? `recommender:${result.llm_rerank_model}` : "recommender:local-tfidf",
    batch_id: `seed-${seed.albumId}-${new Date().toISOString().slice(0, 10)}`,
    recommendations: [...recommendationsByKey.values()].map((item: any) => ({
      seed_album_id: seed.albumId,
      seed_musicbrainz_release_id: seed.musicbrainzReleaseId,
      suggested_artist: item.artist,
      suggested_title: item.title,
      suggested_musicbrainz_release_id: item.musicbrainz_release_id ?? null,
      suggested_musicbrainz_release_group_id: item.musicbrainz_release_group_id ?? null,
      confidence: clamp(Number(item.score ?? item.confidence ?? 0.75), 0, 1),
      rationale: item.rationale ?? null,
      external_url: item.external_url ?? null,
      tidal_url: item.tidal_url ?? null,
      artwork_url: item.artwork_url ?? null,
      status: "suggested",
    })),
  };
}

function combinedMusicdImportPayload(results: any[], configPath: string) {
  const recommendationsByKey = new Map<string, any>();
  for (const result of results) {
    for (const item of result.recommendations ?? []) {
      const key =
        item.recommendation_key ??
        item.suggested_musicbrainz_release_group_id ??
        item.suggested_musicbrainz_release_id ??
        `${item.seed_album_id}:${normalizeSearchText(item.suggested_artist ?? "")}:${normalizeSearchText(item.suggested_title ?? "")}`;
      if (!recommendationsByKey.has(key)) {
        recommendationsByKey.set(key, item);
      }
    }
  }

  const sources = unique(results.map((result) => result.source).filter(Boolean));
  return {
    source: sources.length === 1 ? sources[0] : "recommender:run",
    batch_id: `run-${stableKey(configPath)}-${new Date().toISOString().slice(0, 10)}`,
    recommendations: [...recommendationsByKey.values()],
  };
}

function candidateToJson(candidate: Candidate) {
  return {
    ...publicAlbum(candidate.album),
    owned: candidate.album.owned,
    score: round(candidate.score),
    signals: {
      embedding_similarity: round(candidate.embeddingSimilarity),
      genre_affinity: round(candidate.genreAffinity),
      era_compatibility: round(candidate.eraCompatibility),
      diversity_bonus: round(candidate.diversityBonus),
      artist_penalty: round(candidate.artistPenalty),
    },
    rationale: candidate.rationale,
    musicbrainz_release_id: candidate.album.musicbrainzReleaseId,
    musicbrainz_release_group_id: candidate.album.musicbrainzReleaseGroupId,
    artwork_url: candidate.album.artworkUrl,
    external_url: candidate.album.externalUrl,
    tidal_url: candidate.album.tidalUrl,
  };
}

function publicAlbum(album: AlbumRecord) {
  return {
    album_id: album.albumId,
    artist: album.artist,
    title: album.title,
    year: album.year,
    genres: album.genres,
  };
}

function albumIdentityKeys(album: AlbumRecord): string[] {
  return [
    album.albumId ? `album:${album.albumId}` : null,
    album.musicbrainzReleaseGroupId ? `rg:${album.musicbrainzReleaseGroupId}` : null,
    album.musicbrainzReleaseId ? `release:${album.musicbrainzReleaseId}` : null,
    `text:${normalizeSearchText(album.artist)}:${normalizeSearchText(album.title)}`,
  ].filter((value): value is string => value !== null);
}

function bucketWeight(bucket: CatalogCandidate["bucket"] | CatalogQuery["bucket"]): number {
  if (bucket === "same_genre") return 1;
  if (bucket === "recent") return 0.98;
  if (bucket === "lastfm_artist") return 0.92;
  if (bucket === "lastfm_tag") return 0.88;
  if (bucket === "adjacent") return 0.78;
  return 0.6;
}

function artistCreditName(artistCredit: unknown): string | null {
  if (!Array.isArray(artistCredit)) return null;
  return normalizeOptionalText(
    artistCredit
      .map((credit) => credit?.artist?.name ?? credit?.name)
      .filter(Boolean)
      .join(""),
  );
}

function quoteSearchTerm(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function sameTag(left: string, right: string): boolean {
  return normalizeSearchText(left) === normalizeSearchText(right);
}

function safeBaseUrl(value: string): string {
  try {
    const url = new URL(value);
    return `${url.origin}${url.pathname.replace(/\/$/, "")}`;
  } catch {
    return value.replace(/\/$/, "");
  }
}

function safeEmbeddingUrl(url: URL): string {
  return `${url.origin}${url.pathname}`;
}

function previewText(value: string, maxLength: number): string {
  return value
    .slice(0, maxLength)
    .replace(/\s+/g, " ")
    .trim();
}

function embeddingTextDiagnostics(raw: string): Record<string, unknown> {
  const trimmed = raw.trimEnd();
  return {
    bytes: Buffer.byteLength(raw, "utf8"),
    chars: raw.length,
    hash: stableKey(raw),
    album_id_markers: countOccurrences(raw, "\"album_id\""),
    has_json_object_start: raw.trimStart().startsWith("{"),
    has_embedding_array_key: raw.includes("\"embeddings\""),
    has_final_object_close: /]\s*}\s*$/.test(trimmed),
    tail_preview: previewText(trimmed.slice(-800), 800),
  };
}

function countOccurrences(value: string, needle: string): number {
  let count = 0;
  let start = 0;
  while (true) {
    const index = value.indexOf(needle, start);
    if (index === -1) return count;
    count += 1;
    start = index + needle.length;
  }
}

function explainCandidate(
  seed: AlbumRecord,
  album: AlbumRecord,
  embeddingSimilarity: number,
  genreAffinity: number,
  eraCompatibility: number,
): string {
  const sharedGenres = seed.genres.filter((genre) => album.genres.includes(genre));
  const sharedDescriptors = seed.descriptors.filter((descriptor) => album.descriptors.includes(descriptor)).slice(0, 4);
  const reasons = [];
  if (sharedGenres.length) reasons.push(`shares ${sharedGenres.join(", ")}`);
  if (sharedDescriptors.length) reasons.push(`matches ${sharedDescriptors.join(", ")} traits`);
  if (seed.decade && album.decade && seed.decade === album.decade) reasons.push(`sits in the same ${seed.decade} era`);
  if (!reasons.length && eraCompatibility > 0.5) reasons.push("has compatible era and metadata signals");
  if (!reasons.length) reasons.push("broadens the seed's neighborhood without repeating the same artist lane");
  return `${album.artist} - ${album.title} ${reasons.join("; ")}. Similarity ${round(embeddingSimilarity)}, genre affinity ${round(genreAffinity)}.`;
}

function findSeed(collection: AlbumRecord[], options: RecommendOptions): AlbumRecord {
  if (options.seedAlbumId) {
    const exact = collection.find((album) => album.albumId === options.seedAlbumId);
    if (exact) return exact;
    throw new Error(`No collection album found with album_id ${options.seedAlbumId}`);
  }

  if (options.seed) {
    const rawQuery = options.seed.trim();
    const [artistQuery, titleQuery] = rawQuery.match(/\s+-\s+/)
      ? rawQuery.split(/\s+-\s+/, 2).map(normalizeSearchText)
      : ["", normalizeSearchText(rawQuery)];
    const query = normalizeSearchText(rawQuery);
    const scored = collection
      .map((album) => {
        const artist = normalizeSearchText(album.artist);
        const title = normalizeSearchText(album.title);
        const exactArtist = artistQuery && artist.includes(artistQuery) ? 1 : 0;
        const exactTitle = title.includes(titleQuery) ? 1 : 0;
        return { album, score: exactArtist + exactTitle + jaccard(tokenize(query), tokenize(`${artist} ${title}`)) };
      })
      .sort((a, b) => b.score - a.score);
    if (scored[0]?.score > 0) return scored[0].album;
    throw new Error(`No collection album matched seed "${options.seed}"`);
  }

  throw new Error("recommend requires --seed-album-id or --seed");
}

function parseArgs(args: string[]): Record<string, string | boolean> {
  const parsed: Record<string, string | boolean> = {};
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (!arg.startsWith("--")) continue;
    const key = arg.slice(2);
    const next = args[index + 1];
    if (!next || next.startsWith("--")) {
      parsed[key] = true;
    } else {
      parsed[key] = next;
      index += 1;
    }
  }
  return parsed;
}

function stringArg(args: Record<string, string | boolean>, key: string): string | undefined {
  const value = args[key];
  return typeof value === "string" ? value : undefined;
}

function numberArg(args: Record<string, string | boolean>, key: string, fallback: number): number {
  const value = stringArg(args, key);
  if (!value) return fallback;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed < 0) throw new Error(`--${key} must be a positive number`);
  return parsed;
}

function configNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function configNumberArray(value: unknown, fallback: number[]): number[] {
  if (!Array.isArray(value)) return fallback;
  const numbers = value.filter((item): item is number => typeof item === "number" && Number.isFinite(item));
  return numbers.length ? numbers : fallback;
}

function optionalNumberArg(args: Record<string, string | boolean>, key: string): number | undefined {
  const value = stringArg(args, key);
  if (!value) return undefined;
  const parsed = Number(value);
  if (!Number.isFinite(parsed) || parsed <= 0) throw new Error(`--${key} must be a positive number`);
  return parsed;
}

function optionalBooleanArg(args: Record<string, string | boolean>, key: string): boolean | undefined {
  const value = args[key];
  if (value === undefined) return undefined;
  if (typeof value === "boolean") return value;
  if (/^(true|1|yes|on)$/i.test(value)) return true;
  if (/^(false|0|no|off)$/i.test(value)) return false;
  throw new Error(`--${key} must be true or false`);
}

async function emitJson(value: unknown, output?: string) {
  if (output && isEmbeddingIndex(value)) {
    await emitEmbeddingIndexJson(value, output);
    return;
  }
  const json = `${JSON.stringify(value, null, 2)}\n`;
  if (output) {
    await writeFile(output, json, "utf8");
  } else {
    process.stdout.write(json);
  }
}

function isEmbeddingIndex(value: unknown): value is EmbeddingIndex {
  return Boolean(
    value &&
      typeof value === "object" &&
      (value as any).embedding_schema_version === "album_embeddings_v1" &&
      Array.isArray((value as any).embeddings),
  );
}

async function emitEmbeddingIndexJson(index: EmbeddingIndex, output: string) {
  const tempOutput = `${output}.tmp-${process.pid}-${Date.now()}`;
  const writer = createWriteStream(tempOutput, { encoding: "utf8" });
  progress("embeddings", "writing embedding index", {
    output,
    temp_output: tempOutput,
    embeddings: index.embeddings.length,
    first_album_id: index.embeddings[0]?.album_id ?? null,
    last_album_id: index.embeddings[index.embeddings.length - 1]?.album_id ?? null,
  });
  try {
    await writeStreamChunk(writer, "{\n");
    await writeStreamChunk(writer, `  "embedding_schema_version": ${JSON.stringify(index.embedding_schema_version)},\n`);
    await writeStreamChunk(writer, `  "text_schema_version": ${JSON.stringify(index.text_schema_version)},\n`);
    await writeStreamChunk(writer, `  "embedding_model": ${JSON.stringify(index.embedding_model)},\n`);
    await writeStreamChunk(writer, `  "embedding_base_url": ${JSON.stringify(index.embedding_base_url)},\n`);
    await writeStreamChunk(writer, `  "generated_at": ${JSON.stringify(index.generated_at)},\n`);
    await writeStreamChunk(writer, `  "album_count": ${JSON.stringify(index.album_count)},\n`);
    await writeStreamChunk(writer, '  "embeddings": [\n');
    for (let indexNumber = 0; indexNumber < index.embeddings.length; indexNumber += 1) {
      const comma = indexNumber === index.embeddings.length - 1 ? "" : ",";
      await writeStreamChunk(writer, `    ${JSON.stringify(index.embeddings[indexNumber])}${comma}\n`);
      if ((indexNumber + 1) % 10000 === 0 || indexNumber === index.embeddings.length - 1) {
        progress("embeddings", "embedding index write progress", {
          output,
          written: indexNumber + 1,
          total: index.embeddings.length,
        });
      }
    }
    await writeStreamChunk(writer, "  ]\n");
    await writeStreamChunk(writer, "}\n");
    await closeWriter(writer);
    const tempStat = await stat(tempOutput);
    progress("embeddings", "embedding index temp file closed", { output, temp_output: tempOutput, bytes: tempStat.size });
    await rename(tempOutput, output);
    const outputStat = await stat(output);
    progress("embeddings", "embedding index written", { output, bytes: outputStat.size, embeddings: index.embeddings.length });
  } catch (error) {
    writer.destroy();
    await unlink(tempOutput).catch(() => undefined);
    progressError("embeddings", "embedding index write failed", error, { output, temp_output: tempOutput });
    throw error;
  }
}

async function writeStreamChunk(writer: ReturnType<typeof createWriteStream>, chunk: string) {
  if (writer.write(chunk)) return;
  await new Promise<void>((resolve, reject) => {
    writer.once("drain", resolve);
    writer.once("error", reject);
  });
}

function catalogOptionsFromConfig(config: RunConfig, collectionPath: string, output: string, configDir: string, dryRun: boolean): CatalogOptions {
  const catalog = config.catalog ?? {};
  return {
    collectionPath,
    output,
    candidateCatalog: resolveOptionalConfigPath(catalog.candidateCatalog, configDir),
    limit: configNumber(catalog.limit, 250),
    topGenres: configNumber(catalog.topGenres, 8),
    topArtists: configNumber(catalog.topArtists, 10),
    perQuery: configNumber(catalog.perQuery, 25),
    recentYears: configNumberArray(catalog.recentYears, recentYears()),
    recentPerQuery: configNumber(catalog.recentPerQuery, 10),
    dryRun: dryRun || Boolean(catalog.dryRun),
    musicBrainzUserAgent: catalog.musicBrainzUserAgent ?? DEFAULT_USER_AGENT,
    musicBrainzDelayMs: configNumber(catalog.musicBrainzDelayMs, 1100),
    lastfmApiKey: catalog.lastfmApiKey ?? process.env.LASTFM_API_KEY,
    lastfmDelayMs: configNumber(catalog.lastfmDelayMs, 250),
  };
}

function databasePathFromConfig(config: RunConfig, configDir: string): string | undefined {
  if (typeof config.database === "string") {
    return resolveConfigPath(config.database, configDir);
  }
  if (config.database && config.database.enabled !== false) {
    return resolveConfigPath(config.database.path ?? "recommender.sqlite", configDir);
  }
  return undefined;
}

function embedOptionsFromConfig(
  config: RunConfig,
  collectionPath: string,
  externalPath: string | undefined,
  output: string | undefined,
  databasePath: string | undefined,
  dryRun: boolean,
  force: boolean,
): EmbedOptions {
  const embeddings = config.embeddings ?? {};
  return {
    collectionPath,
    externalPath: dryRun && externalPath && !existsSyncLoose(externalPath) ? undefined : externalPath,
    output,
    databasePath,
    baseUrl: embeddings.baseUrl ?? process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
    apiKey: embeddings.apiKey ?? process.env.OPENAI_API_KEY,
    model: embeddings.model ?? "text-embedding-3-small",
    batchSize: configNumber(embeddings.batchSize, 64),
    dimensions: typeof embeddings.dimensions === "number" ? embeddings.dimensions : undefined,
    dryRun: dryRun || Boolean(embeddings.dryRun),
    force,
  };
}

function recommendationOptionsFromConfig(
  config: RunConfig,
  collectionPath: string,
  externalPath: string | undefined,
  embeddingsPath: string | undefined,
  databasePath: string | undefined,
  output: string | undefined,
): RecommendOptions[] {
  const recommendations = config.recommendations ?? {};
  const seedRuns = [
    ...(recommendations.seeds ?? []).map((seed) => ({ seed })),
    ...(recommendations.seedAlbumIds ?? []).map((seedAlbumId) => ({ seedAlbumId })),
  ];
  if (seedRuns.length === 0) {
    throw new Error("run config recommendations must include at least one seed or seedAlbumId");
  }
  return seedRuns.map((seedRun) => ({
    collectionPath,
    externalPath,
    embeddingsPath,
    databasePath,
    embeddingModel: config.embeddings?.model,
    embeddingBaseUrl: config.embeddings?.baseUrl,
    embeddingDimensions: config.embeddings?.dimensions,
    output: seedRuns.length === 1 ? output : undefined,
    seed: "seed" in seedRun ? seedRun.seed : undefined,
    seedAlbumId: "seedAlbumId" in seedRun ? seedRun.seedAlbumId : undefined,
    ownedCount: configNumber(recommendations.ownedCount, 6),
    discoveryCount: configNumber(recommendations.discoveryCount, 6),
    recentDiscoveryCount: configNumber(recommendations.recentDiscoveryCount, 2),
    poolSize: configNumber(recommendations.poolSize, 60),
    format: recommendations.format ?? "recommendations",
    lmStudioUrl: recommendations.lmStudioUrl,
    rerankModel: recommendations.rerankModel,
    tidal: tidalOptionsFromConfig(config),
  }));
}

function tidalOptionsFromArgs(args: Record<string, string | boolean>): TidalOptions | undefined {
  if (!Boolean(args.tidal)) return undefined;
  return buildTidalOptions({
    enabled: true,
    clientId: stringArg(args, "tidal-client-id"),
    clientSecret: stringArg(args, "tidal-client-secret"),
    countryCode: stringArg(args, "tidal-country-code"),
    minConfidence: optionalNumberArg(args, "tidal-min-confidence"),
    maxCandidates: optionalNumberArg(args, "tidal-max-candidates"),
    delayMs: optionalNumberArg(args, "tidal-delay-ms"),
    overwriteExternalUrl: optionalBooleanArg(args, "tidal-overwrite-external-url"),
    apiBaseUrl: stringArg(args, "tidal-api-base-url"),
    tokenUrl: stringArg(args, "tidal-token-url"),
  });
}

function tidalOptionsFromConfig(config: RunConfig): TidalOptions | undefined {
  const tidal = config.tidal;
  if (!(tidal?.enabled ?? false)) return undefined;
  return buildTidalOptions({
    enabled: true,
    clientId: tidal.clientId,
    clientSecret: tidal.clientSecret,
    countryCode: tidal.countryCode,
    minConfidence: tidal.minConfidence,
    maxCandidates: tidal.maxCandidates,
    delayMs: tidal.delayMs,
    overwriteExternalUrl: tidal.overwriteExternalUrl,
    apiBaseUrl: tidal.apiBaseUrl,
    tokenUrl: tidal.tokenUrl,
  });
}

function buildTidalOptions(input: Partial<TidalOptions> & { enabled: boolean }): TidalOptions {
  return {
    enabled: input.enabled,
    clientId: input.clientId ?? process.env.TIDAL_CLIENT_ID,
    clientSecret: input.clientSecret ?? process.env.TIDAL_CLIENT_SECRET,
    countryCode: (input.countryCode ?? process.env.TIDAL_COUNTRY_CODE ?? "US").toUpperCase(),
    minConfidence: clamp(configNumber(input.minConfidence, 0.82), 0, 1),
    maxCandidates: Math.max(1, Math.round(configNumber(input.maxCandidates, 6))),
    delayMs: Math.max(0, Math.round(configNumber(input.delayMs, 150))),
    overwriteExternalUrl: input.overwriteExternalUrl ?? true,
    apiBaseUrl: input.apiBaseUrl ?? TIDAL_API_ROOT,
    tokenUrl: input.tokenUrl ?? TIDAL_TOKEN_URL,
  };
}

function resolveConfigPath(value: string, baseDir: string): string {
  return path.isAbsolute(value) ? value : path.resolve(baseDir, value);
}

function resolveOptionalConfigPath(value: string | undefined, baseDir: string): string | undefined {
  return value ? resolveConfigPath(value, baseDir) : undefined;
}

async function fileExists(filePath: string): Promise<boolean> {
  try {
    await readFile(filePath);
    return true;
  } catch {
    return false;
  }
}

function existsSyncLoose(filePath: string): boolean {
  try {
    return existsSync(filePath);
  } catch {
    return false;
  }
}

async function openRecommenderDatabase(databasePath: string): Promise<any> {
  const sqlite = await import("bun:sqlite");
  const database = new sqlite.Database(databasePath);
  database.exec("PRAGMA busy_timeout = 5000;");
  database.exec("PRAGMA journal_mode = WAL;");
  database.exec("PRAGMA synchronous = NORMAL;");
  database.exec(`
    CREATE TABLE IF NOT EXISTS recommender_embeddings (
      album_id TEXT NOT NULL,
      embedding_model TEXT NOT NULL,
      embedding_base_url TEXT NOT NULL,
      text_schema_version TEXT NOT NULL,
      text_hash TEXT NOT NULL,
      dimensions INTEGER NOT NULL,
      embedding BLOB NOT NULL,
      artist TEXT NOT NULL,
      title TEXT NOT NULL,
      owned INTEGER NOT NULL,
      musicbrainz_release_id TEXT,
      musicbrainz_release_group_id TEXT,
      updated_unix INTEGER NOT NULL,
      PRIMARY KEY(album_id, embedding_model, embedding_base_url, text_schema_version)
    );

    CREATE INDEX IF NOT EXISTS idx_recommender_embeddings_model
    ON recommender_embeddings(embedding_model, embedding_base_url, text_schema_version);
  `);
  return database;
}

function getStoredEmbeddingRow(store: any, albumId: string, model: string, baseUrl: string): any | null {
  return (
    store
      .prepare(
        `SELECT album_id, text_hash, dimensions, embedding
         FROM recommender_embeddings
         WHERE album_id = ?
           AND embedding_model = ?
           AND embedding_base_url = ?
           AND text_schema_version = ?`,
      )
      .get(albumId, model, baseUrl, TEXT_SCHEMA_VERSION) ?? null
  );
}

function upsertStoredEmbeddings(
  store: any,
  rows: { album: AlbumRecord; embedding: number[]; dimensions: number; textHash: string }[],
  model: string,
  baseUrl: string,
) {
  const now = Math.floor(Date.now() / 1000);
  const insert = store.prepare(
    `INSERT INTO recommender_embeddings
     (album_id, embedding_model, embedding_base_url, text_schema_version, text_hash,
      dimensions, embedding, artist, title, owned, musicbrainz_release_id, musicbrainz_release_group_id, updated_unix)
     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(album_id, embedding_model, embedding_base_url, text_schema_version) DO UPDATE SET
       text_hash = excluded.text_hash,
       dimensions = excluded.dimensions,
       embedding = excluded.embedding,
       artist = excluded.artist,
       title = excluded.title,
       owned = excluded.owned,
       musicbrainz_release_id = excluded.musicbrainz_release_id,
       musicbrainz_release_group_id = excluded.musicbrainz_release_group_id,
       updated_unix = excluded.updated_unix`,
  );
  const transaction = store.transaction((items: typeof rows) => {
    for (const item of items) {
      insert.run(
        item.album.albumId,
        model,
        baseUrl,
        TEXT_SCHEMA_VERSION,
        item.textHash,
        item.dimensions,
        vectorToBlob(item.embedding),
        item.album.artist,
        item.album.title,
        item.album.owned ? 1 : 0,
        item.album.musicbrainzReleaseId,
        item.album.musicbrainzReleaseGroupId,
        now,
      );
    }
  });
  transaction(rows);
}

function vectorToBlob(vector: number[]): Buffer {
  const buffer = Buffer.allocUnsafe(vector.length * 4);
  for (let index = 0; index < vector.length; index += 1) {
    buffer.writeFloatLE(vector[index], index * 4);
  }
  return buffer;
}

function blobToVector(blob: Uint8Array): number[] {
  const bytes = Buffer.from(blob);
  if (bytes.length % 4 !== 0) {
    throw new Error(`Stored embedding blob has invalid byte length ${bytes.length}`);
  }
  const vector: number[] = [];
  for (let offset = 0; offset < bytes.length; offset += 4) {
    vector.push(bytes.readFloatLE(offset));
  }
  return vector;
}

async function* readLines(filePath: string): AsyncIterable<string> {
  yield* streamLines(createReadStream(filePath, { encoding: "utf8" }));
}

async function* dumpTableLines(dumpPath: string, table: string): AsyncIterable<string> {
  const tablePath = await resolveDumpTablePath(dumpPath, table);
  if (isLikelyDirectory(dumpPath)) {
    yield* readLines(tablePath);
    return;
  }

  const child = spawn("tar", ["-xOf", dumpPath, tablePath], { stdio: ["ignore", "pipe", "pipe"] });
  const stderr: Buffer[] = [];
  child.stderr.on("data", (chunk) => stderr.push(Buffer.from(chunk)));
  for await (const line of streamLines(child.stdout)) {
    yield line;
  }
  const exitCode = await waitForChild(child);
  if (exitCode !== 0) {
    throw new Error(`tar failed reading ${table} from ${dumpPath}: ${Buffer.concat(stderr).toString("utf8").trim()}`);
  }
}

async function* streamLines(stream: AsyncIterable<Buffer | string>): AsyncIterable<string> {
  let pending = "";
  for await (const chunk of stream) {
    pending += chunk.toString();
    let newlineIndex = pending.indexOf("\n");
    while (newlineIndex !== -1) {
      const line = pending.slice(0, newlineIndex).replace(/\r$/, "");
      pending = pending.slice(newlineIndex + 1);
      yield line;
      newlineIndex = pending.indexOf("\n");
    }
  }
  if (pending.length) yield pending.replace(/\r$/, "");
}

async function resolveDumpTablePath(dumpPath: string, table: string): Promise<string> {
  if (isLikelyDirectory(dumpPath)) {
    for (const candidate of [path.join(dumpPath, "mbdump", table), path.join(dumpPath, table)]) {
      if (existsSyncLoose(candidate)) return candidate;
    }
    throw new Error(`Could not find MusicBrainz table ${table} in ${dumpPath}`);
  }

  const entries = await listArchiveEntries(dumpPath);
  const wanted = [`mbdump/${table}`, table];
  const found = entries.find((entry) => wanted.includes(entry)) ?? entries.find((entry) => entry.endsWith(`/${table}`));
  if (!found) throw new Error(`Could not find MusicBrainz table ${table} in ${dumpPath}`);
  return found;
}

async function listArchiveEntries(archivePath: string): Promise<string[]> {
  const cached = ARCHIVE_ENTRIES_CACHE.get(archivePath);
  if (cached) return cached;
  const promise = listArchiveEntriesUncached(archivePath);
  ARCHIVE_ENTRIES_CACHE.set(archivePath, promise);
  return promise;
}

async function listArchiveEntriesUncached(archivePath: string): Promise<string[]> {
  const child = spawn("tar", ["-tf", archivePath], { stdio: ["ignore", "pipe", "pipe"] });
  const stdout: Buffer[] = [];
  const stderr: Buffer[] = [];
  child.stdout.on("data", (chunk) => stdout.push(Buffer.from(chunk)));
  child.stderr.on("data", (chunk) => stderr.push(Buffer.from(chunk)));
  const exitCode = await waitForChild(child);
  if (exitCode !== 0) {
    throw new Error(`tar failed listing ${archivePath}: ${Buffer.concat(stderr).toString("utf8").trim()}`);
  }
  return Buffer.concat(stdout).toString("utf8").split(/\r?\n/).filter(Boolean);
}

async function dumpHasTable(dumpPath: string, table: string): Promise<boolean> {
  try {
    await resolveDumpTablePath(dumpPath, table);
    return true;
  } catch {
    return false;
  }
}

async function waitForChild(child: ReturnType<typeof spawn>): Promise<number | null> {
  return new Promise((resolve, reject) => {
    child.on("error", reject);
    child.on("close", resolve);
  });
}

function isLikelyDirectory(value: string): boolean {
  return !/\.(tar|tbz|tbz2|bz2|gz|xz)$/i.test(value);
}

function splitDumpLine(line: string): (string | null)[] {
  return line.split("\t").map((value) => (value === "\\N" ? null : value));
}

async function loadDumpIdNameMap(dumpPath: string, table: string): Promise<Map<string, string>> {
  const map = new Map<string, string>();
  for await (const line of dumpTableLines(dumpPath, table)) {
    const row = splitDumpLine(line);
    if (row[0] && row[1]) map.set(row[0], row[1]);
  }
  return map;
}

async function loadReleaseGroupFirstReleaseDates(dumpPath: string): Promise<Map<string, string>> {
  const map = new Map<string, string>();
  if (!(await dumpHasTable(dumpPath, "release_group_meta"))) {
    return map;
  }
  for await (const line of dumpTableLines(dumpPath, "release_group_meta")) {
    const row = splitDumpLine(line);
    const releaseGroup = row[0];
    const year = row[3];
    if (!releaseGroup || !year) continue;
    const month = row[4]?.padStart(2, "0");
    const day = row[5]?.padStart(2, "0");
    map.set(releaseGroup, [year, month, day].filter(Boolean).join("-"));
  }
  return map;
}

async function loadReleaseGroupSecondaryTypes(dumpPath: string, secondaryTypes: Map<string, string>): Promise<Map<string, string[]>> {
  const map = new Map<string, string[]>();
  for await (const line of dumpTableLines(dumpPath, "release_group_secondary_type_join")) {
    const row = splitDumpLine(line);
    const releaseGroup = row[0];
    const typeId = row[1];
    const type = typeId ? secondaryTypes.get(typeId) : undefined;
    if (!releaseGroup || !type) continue;
    map.set(releaseGroup, [...(map.get(releaseGroup) ?? []), type]);
  }
  return map;
}

async function loadReleaseGroupTags(dumpPath: string, minCount: number): Promise<Map<string, { name: string; count: number }[]>> {
  const tags = await loadDumpIdNameMap(dumpPath, "tag");
  const map = new Map<string, { name: string; count: number }[]>();
  for await (const line of dumpTableLines(dumpPath, "release_group_tag")) {
    const row = splitDumpLine(line);
    const releaseGroup = row[0];
    const tagId = row[1];
    const count = Number(row[2] ?? 0);
    const name = tagId ? tags.get(tagId) : undefined;
    if (!releaseGroup || !name || !Number.isFinite(count) || count < minCount) continue;
    map.set(releaseGroup, [...(map.get(releaseGroup) ?? []), { name, count }]);
  }
  for (const [releaseGroup, tagRows] of map) {
    map.set(
      releaseGroup,
      tagRows.sort((a, b) => b.count - a.count || a.name.localeCompare(b.name)).slice(0, 24),
    );
  }
  return map;
}

async function closeWriter(writer: ReturnType<typeof createWriteStream>): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    writer.on("error", reject);
    writer.end(resolve);
  });
}

function tokenize(text: string): string[] {
  return normalizeSearchText(text)
    .split(/\s+/)
    .filter((token) => token.length > 1 && !STOP_WORDS.has(token));
}

function normalizeSearchText(text: string): string {
  return text
    .toLocaleLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/&/g, " and ")
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

function normalizeGenre(value: string): string {
  const cleaned = cleanPhrase(value);
  const alias = GENRE_ALIASES.get(cleaned.toLocaleLowerCase());
  return alias ?? cleaned;
}

function genreFamily(value: string): string {
  const normalized = normalizeSearchText(normalizeGenre(value));
  if (/\b(hip hop|hiphop|rap|boom bap|trap)\b/.test(normalized)) return "hip-hop";
  if (/\b(electronic|electronica|idm|techno|ambient|downtempo|drum and bass|trip hop)\b/.test(normalized)) return "electronic";
  if (/\b(indie|alternative|experimental|noise|college rock|noise pop|post punk|dream pop)\b/.test(normalized)) return "alternative";
  if (/\b(rock|punk|metal|krautrock|post rock|psychedelic|no wave)\b/.test(normalized)) return "rock";
  if (/\b(jazz|fusion|spiritual jazz|soul jazz|free jazz)\b/.test(normalized)) return "jazz";
  if (/\b(pop|synthpop|power pop|art pop)\b/.test(normalized)) return "pop";
  if (/\b(soul|funk|r b|rnb)\b/.test(normalized)) return "soul-funk";
  return normalized;
}

function normalizeSignal(value: string): string {
  return normalizeSearchText(value).replace(/\s+/g, "-");
}

function cleanExternalTag(value: unknown): string | null {
  const tag = normalizeOptionalText(value);
  if (!tag || tag.includes("_") || tag.length > 40) return null;
  return cleanPhrase(tag).toLocaleLowerCase();
}

function cleanPhrase(value: string): string {
  return value.trim().replace(/\s+/g, " ");
}

function requiredText(value: unknown, label: string): string {
  const text = normalizeOptionalText(value);
  if (!text) throw new Error(`${label} is required`);
  return text;
}

function normalizeOptionalText(value: unknown): string | null {
  if (value === null || value === undefined) return null;
  const text = String(value).trim();
  return text.length ? text : null;
}

function parseYear(value: unknown): number | null {
  const match = normalizeOptionalText(value)?.match(/\b(19|20)\d{2}\b/);
  return match ? Number(match[0]) : null;
}

function inferDescriptorsForGenre(genre: string): string[] {
  const key = normalizeSearchText(genre).replace(/\s+/g, "");
  return STYLE_LEXICON[key] ?? [];
}

function inferDescriptorsForTag(tag: string): string[] {
  const key = normalizeSearchText(tag).replace(/\s+/g, "");
  return STYLE_LEXICON[key] ?? [cleanPhrase(tag).toLocaleLowerCase()];
}

function topCounts(
  values: string[],
  limit: number,
  sorter: (a: [string, number], b: [string, number]) => number = (a, b) => b[1] - a[1] || a[0].localeCompare(b[0]),
): [string, number][] {
  const counts = new Map<string, number>();
  for (const value of values.filter(Boolean)) {
    counts.set(value, (counts.get(value) ?? 0) + 1);
  }
  return [...counts.entries()].sort(sorter).slice(0, limit);
}

function popularityTendency(collection: AlbumRecord[], topArtists: [string, number][]): string {
  const repeatedArtistShare =
    topArtists.filter(([, count]) => count >= 3).reduce((sum, [, count]) => sum + count, 0) / Math.max(1, collection.length);
  const lowMetadataShare =
    collection.filter((album) => album.genres.length === 0 || album.musicbrainzReleaseGroupId === null).length /
    Math.max(1, collection.length);
  if (lowMetadataShare > 0.25) return "metadata-sparse and likely tilted toward deeper catalogue material";
  if (repeatedArtistShare > 0.35) return "artist-deep listening with repeat catalogue exploration";
  if (collection.length > 750) return "broad collection with a mix of canonical and obscure records";
  return "balanced collection breadth";
}

function cosine(a: Map<number, number>, b: Map<number, number>): number {
  let dot = 0;
  const [small, large] = a.size < b.size ? [a, b] : [b, a];
  for (const [index, value] of small) {
    dot += value * (large.get(index) ?? 0);
  }
  return dot;
}

function albumSimilarity(seed: AlbumRecord, album: AlbumRecord): number {
  if (seed.denseVector && album.denseVector) {
    return denseCosine(seed.denseVector, album.denseVector);
  }
  return cosine(seed.vector, album.vector);
}

function denseCosine(a: Float32Array, b: Float32Array): number {
  const length = Math.min(a.length, b.length);
  let dot = 0;
  for (let index = 0; index < length; index += 1) {
    dot += a[index] * b[index];
  }
  return dot;
}

function partialDenseCosine(a: Float32Array, b: Float32Array, dimensions: number): number {
  const length = Math.min(a.length, b.length, dimensions);
  let dot = 0;
  for (let index = 0; index < length; index += 1) {
    dot += a[index] * b[index];
  }
  return dot;
}

function normalizeVector(vector: Map<number, number>) {
  const magnitude = Math.sqrt([...vector.values()].reduce((sum, value) => sum + value * value, 0));
  if (magnitude === 0) return;
  for (const [index, value] of vector) {
    vector.set(index, value / magnitude);
  }
}

function normalizeArrayVector(vector: number[]): number[] {
  const magnitude = Math.sqrt(vector.reduce((sum, value) => sum + value * value, 0));
  if (magnitude === 0) return vector;
  return vector.map((value) => value / magnitude);
}

function applyRequestedDimensions(
  vector: number[],
  options: Pick<EmbedOptions, "dimensions">,
  batchContext: EmbeddingBatchLogContext,
  album: AlbumRecord,
): number[] {
  if (!options.dimensions) return vector;
  if (vector.length === options.dimensions) return vector;
  if (vector.length > options.dimensions) {
    const truncationKey = `${vector.length}:${options.dimensions}`;
    if (!DIMENSION_TRUNCATION_LOGGED.has(truncationKey)) {
      DIMENSION_TRUNCATION_LOGGED.add(truncationKey);
      progress("embeddings", "truncating embedding vectors to requested dimensions", {
        ...batchContext,
        first_album_id: album.albumId,
        provider_dimensions: vector.length,
        stored_dimensions: options.dimensions,
      });
    }
    return vector.slice(0, options.dimensions);
  }
  progress("embeddings", "embedding vector shorter than requested dimensions", {
    ...batchContext,
    album_id: album.albumId,
    artist: album.artist,
    title: album.title,
    provider_dimensions: vector.length,
    requested_dimensions: options.dimensions,
  });
  throw new Error(
    `Embedding vector for ${album.artist} - ${album.title} has ${vector.length} dimensions, below requested ${options.dimensions}`,
  );
}

function isFiniteVector(value: unknown): value is number[] {
  return Array.isArray(value) && value.every((item) => typeof item === "number" && Number.isFinite(item));
}

function vectorFromArray(vector: number[]): Map<number, number> {
  const sparse = new Map<number, number>();
  for (let index = 0; index < vector.length; index += 1) {
    const value = vector[index];
    if (value !== 0) sparse.set(index, value);
  }
  normalizeVector(sparse);
  return sparse;
}

function jaccard(a: string[], b: string[]): number {
  const left = new Set(a);
  const right = new Set(b);
  if (!left.size && !right.size) return 0;
  let intersection = 0;
  for (const value of left) {
    if (right.has(value)) intersection += 1;
  }
  return intersection / new Set([...left, ...right]).size;
}

function sameArtist(a: string, b: string): boolean {
  return normalizeSearchText(a) === normalizeSearchText(b);
}

function unique(values: string[]): string[] {
  return [...new Set(values.map(cleanPhrase).filter(Boolean))];
}

function stableKey(input: string): string {
  let hash = 0x811c9dc5;
  for (let index = 0; index < input.length; index += 1) {
    hash ^= input.charCodeAt(index);
    hash = Math.imul(hash, 0x01000193);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}

function stableScore(input: string): number {
  return parseInt(stableKey(input), 16) / 0xffffffff;
}

function eraSortKey(value: string): number {
  const year = Number(value.match(/\d{4}/)?.[0]);
  return Number.isFinite(year) ? year : 9999;
}

function round(value: number): number {
  return Math.round(value * 1000) / 1000;
}

function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

main().catch((error: unknown) => {
  progressError("error", "command failed", error, { argv: process.argv.slice(2).join(" ") });
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
