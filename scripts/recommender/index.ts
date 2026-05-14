#!/usr/bin/env node

import { existsSync } from "node:fs";
import { readFile, writeFile } from "node:fs/promises";
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
  trackCount: number | null;
  normalizedText: string;
  tokens: string[];
  vector: Map<number, number>;
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

type RecommendOptions = {
  collectionPath: string;
  externalPath?: string;
  embeddingsPath?: string;
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
};

type EmbedOptions = {
  collectionPath: string;
  externalPath?: string;
  output?: string;
  baseUrl: string;
  apiKey?: string;
  model: string;
  batchSize: number;
  dimensions?: number;
  dryRun: boolean;
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

type CatalogOptions = {
  collectionPath: string;
  output?: string;
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

type RunConfig = {
  collection?: string;
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
const DEFAULT_USER_AGENT = "musicd-recommender/0.1 (local catalog builder)";

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
    const options: EmbedOptions = {
      collectionPath: stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json"),
      externalPath: stringArg(args, "external-catalog"),
      output: stringArg(args, "output") ?? (dryRun ? undefined : path.join(SCRIPT_DIR, "embeddings.json")),
      baseUrl: stringArg(args, "embedding-base-url") ?? process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
      apiKey: stringArg(args, "embedding-api-key") ?? process.env.OPENAI_API_KEY,
      model: stringArg(args, "embedding-model") ?? "text-embedding-3-small",
      batchSize: numberArg(args, "embedding-batch-size", 64),
      dimensions: optionalNumberArg(args, "embedding-dimensions"),
      dryRun,
    };
    const result = await buildEmbeddingIndex(options);
    await emitJson(result, options.output);
    return;
  }

  if (command === "catalog") {
    const options: CatalogOptions = {
      collectionPath: stringArg(args, "collection") ?? path.join(SCRIPT_DIR, "seed.json"),
      output: stringArg(args, "output"),
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
    };
    const result = await recommend(options);
    await emitJson(result, options.output);
    return;
  }

  usage();
}

async function recommend(options: RecommendOptions) {
  const collection = await loadAlbums(options.collectionPath, true);
  const externalRaw = options.externalPath ? await loadAlbums(options.externalPath, false) : [];
  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
  const external = externalRaw.filter((album) => !albumIdentityKeys(album).some((key) => ownedIdentities.has(key)));
  const allAlbums = [...collection, ...external];
  const embeddingModel = options.embeddingsPath
    ? await applyEmbeddingIndex(allAlbums, options.embeddingsPath)
    : applyLocalVectorIndex(allAlbums);

  const seed = findSeed(collection, options);
  const profile = buildCollectionProfile(collection, embeddingModel);
  const ownedPool = retrieve(seed, collection, profile, options.poolSize, true);
  const discoveryPool = retrieve(seed, external, profile, options.poolSize, false);

  const owned = diversify(ownedPool, options.ownedCount, new Set([seed.artist]));
  const discovery = selectDiscoveryBatch(seed, discoveryPool, owned, options);

  const baseResult = {
    text_schema_version: TEXT_SCHEMA_VERSION,
    embedding_model: embeddingModel,
    seed_album: publicAlbum(seed),
    collection_profile: profile,
    owned_recommendations: owned.map(candidateToJson),
    discovery_recommendations: discovery.map(candidateToJson),
  };

  const maybeReranked =
    options.lmStudioUrl && options.rerankModel
      ? await rerankWithLmStudio(baseResult, options.lmStudioUrl, options.rerankModel).catch(
          (error: unknown) => ({
            ...baseResult,
            rerank_warning: `LM Studio rerank failed: ${String(error)}`,
          }),
        )
      : baseResult;

  if (options.format === "import") {
    return toMusicdImportPayload(seed, maybeReranked);
  }

  return maybeReranked;
}

async function runPipeline(options: RunOptions) {
  const config = JSON.parse(await readFile(options.configPath, "utf8")) as RunConfig;
  const configDir = path.dirname(options.configPath);
  const collectionPath = resolveConfigPath(config.collection ?? "seed.json", configDir);
  const artifacts = {
    profile: resolveOptionalConfigPath(config.artifacts?.profile, configDir),
    catalog: resolveOptionalConfigPath(config.artifacts?.catalog ?? "external-catalog.json", configDir),
    embeddings: resolveOptionalConfigPath(config.artifacts?.embeddings ?? "embeddings.json", configDir),
    recommendations: resolveOptionalConfigPath(config.artifacts?.recommendations ?? "recommendations.json", configDir),
    importPayload: resolveOptionalConfigPath(config.artifacts?.importPayload, configDir),
  };
  const steps: unknown[] = [];

  const collection = await loadAlbums(collectionPath, true);
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
      steps.push({ step: "catalog", action: "reused", output: catalogPath });
    } else {
      const catalogOptions = catalogOptionsFromConfig(config, collectionPath, catalogPath, options.dryRun);
      if (options.dryRun || catalogOptions.dryRun) {
        const plan = await buildExternalCatalog({ ...catalogOptions, dryRun: true });
        steps.push({ step: "catalog", action: "would_build", output: catalogPath, plan });
      } else {
        const catalog = await buildExternalCatalog(catalogOptions);
        await emitJson(catalog, catalogPath);
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
  if (embeddingsEnabled && embeddingsPath) {
    const reuseEmbeddings = !options.dryRun && !options.force && (config.embeddings?.reuseExisting ?? true) && (await fileExists(embeddingsPath));
    if (reuseEmbeddings) {
      steps.push({ step: "embeddings", action: "reused", output: embeddingsPath });
    } else {
      const embedOptions = embedOptionsFromConfig(config, collectionPath, catalogPath, embeddingsPath, options.dryRun);
      if (options.dryRun || embedOptions.dryRun) {
        const preview = await buildEmbeddingIndex({ ...embedOptions, dryRun: true });
        steps.push({
          step: "embeddings",
          action: "would_build",
          output: embeddingsPath,
          preview,
          note:
            catalogPath && embedOptions.externalPath === undefined
              ? "External catalog does not exist yet; dry-run embedding preview uses collection albums only."
              : undefined,
        });
      } else {
        const index = await buildEmbeddingIndex(embedOptions);
        await emitJson(index, embeddingsPath);
        steps.push({ step: "embeddings", action: "wrote", output: embeddingsPath, album_count: (index as EmbeddingIndex).album_count });
      }
    }
  } else {
    embeddingsPath = undefined;
    steps.push({ step: "embeddings", action: "skipped" });
  }

  const recommendationsConfig = config.recommendations ?? {};
  const recommendationsEnabled = recommendationsConfig.enabled ?? true;
  if (recommendationsEnabled) {
    const recommendationRuns = recommendationOptionsFromConfig(
      config,
      collectionPath,
      catalogPath,
      embeddingsPath,
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
      for (const recommendationOptions of recommendationRuns) {
        results.push(await recommend(recommendationOptions));
      }
      const output =
        recommendationsConfig.format === "import"
          ? combinedMusicdImportPayload(results, options.configPath)
          : {
              generated_at: new Date().toISOString(),
              config_path: options.configPath,
              result_count: results.length,
              results,
            };
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
    }
  } else {
    steps.push({ step: "recommendations", action: "skipped" });
  }

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
  const collection = await loadAlbums(options.collectionPath, true);
  const externalRaw = options.externalPath ? await loadAlbums(options.externalPath, false) : [];
  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
  const external = externalRaw.filter((album) => !albumIdentityKeys(album).some((key) => ownedIdentities.has(key)));
  const albums = [...collection, ...external];

  if (options.dryRun) {
    return {
      embedding_schema_version: "album_embeddings_v1",
      text_schema_version: TEXT_SCHEMA_VERSION,
      embedding_model: options.model,
      embedding_base_url: safeBaseUrl(options.baseUrl),
      album_count: albums.length,
      batch_size: options.batchSize,
      dimensions: options.dimensions ?? null,
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

  const embeddings: AlbumEmbedding[] = [];
  for (let index = 0; index < albums.length; index += options.batchSize) {
    const batch = albums.slice(index, index + options.batchSize);
    const vectors = await createEmbeddings(batch.map((album) => album.normalizedText), options);
    if (vectors.length !== batch.length) {
      throw new Error(`Embedding provider returned ${vectors.length} vector(s) for ${batch.length} input(s)`);
    }
    for (let batchIndex = 0; batchIndex < batch.length; batchIndex += 1) {
      const album = batch[batchIndex];
      embeddings.push({
        album_id: album.albumId,
        artist: album.artist,
        title: album.title,
        owned: album.owned,
        text_hash: stableKey(album.normalizedText),
        dimensions: vectors[batchIndex].length,
        embedding: normalizeArrayVector(vectors[batchIndex]),
        musicbrainz_release_id: album.musicbrainzReleaseId,
        musicbrainz_release_group_id: album.musicbrainzReleaseGroupId,
      });
    }
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

async function createEmbeddings(inputs: string[], options: EmbedOptions): Promise<number[][]> {
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
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText}: ${await response.text()}`);
  }
  const json: any = await response.json();
  const data = Array.isArray(json.data) ? json.data : [];
  return data
    .sort((a: any, b: any) => Number(a.index ?? 0) - Number(b.index ?? 0))
    .map((item: any) => item.embedding)
    .filter((embedding: unknown): embedding is number[] => Array.isArray(embedding));
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
      collection_profile: profile,
      plan,
      notes: [
        "MusicBrainz queries are rate-limited by --musicbrainz-delay-ms.",
        options.lastfmApiKey
          ? "Last.fm expansion is enabled."
          : "Last.fm expansion is disabled because no API key was provided.",
      ],
    };
  }

  const ownedIdentities = new Set(collection.flatMap(albumIdentityKeys));
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
      },
      lastfm: {
        enabled: Boolean(options.lastfmApiKey),
      },
    },
    plan,
    warnings,
    albums,
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

async function loadAlbums(path: string, owned: boolean): Promise<AlbumRecord[]> {
  const raw = JSON.parse(await readFile(path, "utf8"));
  const inputs: AlbumInput[] = Array.isArray(raw) ? raw : raw.seeds ?? raw.albums ?? [];
  if (!Array.isArray(inputs)) {
    throw new Error(`${path} must contain an array, { "seeds": [...] }, or { "albums": [...] }`);
  }
  return inputs.map((input, index) => normalizeAlbum(input, owned, basename(path), index));
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
  const index = JSON.parse(await readFile(embeddingPath, "utf8")) as EmbeddingIndex;
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
    album.vector = vectorFromArray(embedding.embedding);
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

  return index.embedding_model;
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
  return albums
    .filter((album) => album.albumId !== seed.albumId)
    .map((album) => scoreCandidate(seed, album, profile, ownedPool))
    .sort((a, b) => b.score - a.score)
    .slice(0, limit);
}

function scoreCandidate(
  seed: AlbumRecord,
  album: AlbumRecord,
  profile: CollectionProfile,
  ownedPool: boolean,
): Candidate {
  const embeddingSimilarity = cosine(seed.vector, album.vector);
  const genreAffinity = affinity(seed, album);
  const eraCompatibility = seed.year && album.year ? Math.max(0, 1 - Math.abs(seed.year - album.year) / 45) : 0.35;
  const profileGenreSet = new Set(profile.top_genres.slice(0, 8).map(([genre]) => genre));
  const profileFit = album.genres.some((genre) => profileGenreSet.has(genre)) ? 0.04 : 0;
  const exploratoryBonus = !ownedPool && genreAffinity < 0.2 ? 0.03 : 0;
  const artistPenalty = sameArtist(seed.artist, album.artist) ? 0.25 : 0;
  const diversityBonus = album.descriptors.some((descriptor) => seed.descriptors.includes(descriptor)) ? 0.04 : 0;
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
    rationale: explainCandidate(seed, album, embeddingSimilarity, genreAffinity, eraCompatibility),
  };
}

function affinity(seed: AlbumRecord, album: AlbumRecord): number {
  const seedGenres = genreSignalSet(seed);
  const albumGenres = genreSignalSet(album);
  const genreScore = jaccard([...seedGenres], [...albumGenres]);
  const tagScore = jaccard(
    [...seed.tags, ...seed.descriptors].map(normalizeSignal),
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

async function emitJson(value: unknown, output?: string) {
  const json = `${JSON.stringify(value, null, 2)}\n`;
  if (output) {
    await writeFile(output, json, "utf8");
  } else {
    process.stdout.write(json);
  }
}

function catalogOptionsFromConfig(config: RunConfig, collectionPath: string, output: string, dryRun: boolean): CatalogOptions {
  const catalog = config.catalog ?? {};
  return {
    collectionPath,
    output,
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

function embedOptionsFromConfig(
  config: RunConfig,
  collectionPath: string,
  externalPath: string | undefined,
  output: string,
  dryRun: boolean,
): EmbedOptions {
  const embeddings = config.embeddings ?? {};
  return {
    collectionPath,
    externalPath: dryRun && externalPath && !existsSyncLoose(externalPath) ? undefined : externalPath,
    output,
    baseUrl: embeddings.baseUrl ?? process.env.OPENAI_BASE_URL ?? "https://api.openai.com/v1",
    apiKey: embeddings.apiKey ?? process.env.OPENAI_API_KEY,
    model: embeddings.model ?? "text-embedding-3-small",
    batchSize: configNumber(embeddings.batchSize, 64),
    dimensions: typeof embeddings.dimensions === "number" ? embeddings.dimensions : undefined,
    dryRun: dryRun || Boolean(embeddings.dryRun),
  };
}

function recommendationOptionsFromConfig(
  config: RunConfig,
  collectionPath: string,
  externalPath: string | undefined,
  embeddingsPath: string | undefined,
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
  }));
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
  console.error(error instanceof Error ? error.message : String(error));
  process.exit(1);
});
