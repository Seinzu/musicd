function escapeHtml(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

function formatDuration(seconds) {
  if (seconds === null || seconds === undefined || Number.isNaN(Number(seconds))) {
    return 'Unknown';
  }
  const total = Number(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, '0')}:${String(secs).padStart(2, '0')}`;
  }
  return `${minutes}:${String(secs).padStart(2, '0')}`;
}

/* --------------------------------------------------------------- Likes */
const MUSICD_CLIENT_ID_KEY = 'musicd.clientId';
const MUSICD_LIKED_ITEMS_KEY = 'musicd.likedItems';

function getMusicdClientId() {
  try {
    const existing = window.localStorage.getItem(MUSICD_CLIENT_ID_KEY);
    if (existing) {
      return existing;
    }
    const generated = window.crypto?.randomUUID
      ? window.crypto.randomUUID()
      : `web-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
    window.localStorage.setItem(MUSICD_CLIENT_ID_KEY, generated);
    return generated;
  } catch (_error) {
    return `web-volatile-${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  }
}

function loadLikedItems() {
  try {
    return new Set(JSON.parse(window.localStorage.getItem(MUSICD_LIKED_ITEMS_KEY) || '[]'));
  } catch (_error) {
    return new Set();
  }
}

function saveLikedItems(items) {
  try {
    window.localStorage.setItem(MUSICD_LIKED_ITEMS_KEY, JSON.stringify([...items]));
  } catch (_error) {
    /* localStorage can be unavailable in private contexts */
  }
}

function likedItemKey(kind, id) {
  return `${kind}:${id}`;
}

function updateLikeButtons(kind, id, count, liked) {
  const cssEscape = window.CSS?.escape || ((value) => String(value).replaceAll('"', '\\"'));
  const selector = `.like-button[data-like-kind="${cssEscape(kind)}"][data-like-id="${cssEscape(id)}"]`;
  for (const button of document.querySelectorAll(selector)) {
    button.dataset.likeCount = String(count);
    button.classList.toggle('liked', liked);
    button.disabled = liked;
    button.setAttribute('aria-pressed', liked ? 'true' : 'false');
    const countNode = button.querySelector('.like-count');
    if (countNode) {
      countNode.textContent = String(count);
    }
  }
}

async function likeItem(button) {
  const kind = button.dataset.likeKind || '';
  const id = button.dataset.likeId || '';
  if (!kind || !id || button.dataset.loading === 'true') {
    return;
  }

  button.dataset.loading = 'true';
  const form = new URLSearchParams();
  form.set('item_kind', kind);
  form.set('item_id', id);
  form.set('client_id', getMusicdClientId());

  try {
    const response = await fetch('/api/like', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: form.toString(),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || 'Like failed');
    }
    const likedItems = loadLikedItems();
    likedItems.add(likedItemKey(kind, id));
    saveLikedItems(likedItems);
    updateLikeButtons(kind, id, payload.like_count ?? button.dataset.likeCount ?? 0, true);
  } catch (_error) {
    button.classList.add('like-error');
    window.setTimeout(() => button.classList.remove('like-error'), 1200);
  } finally {
    button.dataset.loading = 'false';
  }
}

function setupLikeButtons() {
  const buttons = document.querySelectorAll('.like-button');
  if (!buttons.length) {
    return;
  }
  const likedItems = loadLikedItems();
  for (const button of buttons) {
    const kind = button.dataset.likeKind || '';
    const id = button.dataset.likeId || '';
    const liked = likedItems.has(likedItemKey(kind, id));
    button.classList.toggle('liked', liked);
    button.disabled = liked;
    button.setAttribute('aria-pressed', liked ? 'true' : 'false');
    if (button.dataset.likeReady === 'true') {
      continue;
    }
    button.dataset.likeReady = 'true';
    button.addEventListener('click', (event) => {
      event.preventDefault();
      event.stopPropagation();
      likeItem(button);
    });
  }
}

/* -------------------------------------------------------------- Renderer */
function syncRendererFields(value) {
  const proxies = document.querySelectorAll('.renderer-location-proxy');
  for (const proxy of proxies) {
    proxy.value = value;
  }
}

async function discoverRenderers() {
  const select = document.getElementById('renderer_discovery');
  if (!select) {
    return;
  }
  select.innerHTML = '<option value="">Discovering renderers...</option>';
  try {
    const response = await fetch('/api/renderers/discover');
    const items = await response.json();
    select.innerHTML = '';
    if (!items.length) {
      select.innerHTML = '<option value="">No renderers discovered</option>';
      return;
    }
    for (const item of items) {
      const option = document.createElement('option');
      option.value = item.location;
      option.textContent = item.name || item.location;
      select.appendChild(option);
    }
  } catch (_error) {
    select.innerHTML = '<option value="">Discovery failed</option>';
  }
}

function applySelectedRenderer() {
  const select = document.getElementById('renderer_discovery');
  const input = document.getElementById('renderer_location');
  if (!select || !input) {
    return;
  }
  if (select.value) {
    input.value = select.value;
    syncRendererFields(select.value);
  }
}

/* ---------------------------------------------------- Track inspector */
function renderTrackDetailPanel(track) {
  const host = document.getElementById('track_detail_panel');
  if (!host) {
    return;
  }
  if (!track || track.error) {
    host.innerHTML = `<h3>Track Tags</h3><p class="meta">${escapeHtml(track?.error || 'Track details are unavailable.')}</p>`;
    return;
  }

  const artworkHtml = track.artwork
    ? `<img class="sidebar-artwork" src="${escapeHtml(track.artwork.url)}" alt="Artwork for ${escapeHtml(track.album)}">`
    : '<div class="sidebar-artwork placeholder">No Art</div>';

  const metaRows = [
    { label: 'Artist', value: track.artist || 'Unknown' },
    { label: 'Album', value: track.album || 'Unknown' },
    { label: 'Disc / Track', value: `${track.disc_number ?? '?'} / ${track.track_number ?? '?'}` },
    { label: 'Duration', value: formatDuration(track.duration_seconds) },
    { label: 'Format', value: track.mime_type || 'Unknown' },
    { label: 'Parser', value: track.embedded_metadata?.parser || 'Unknown' },
    { label: 'Path', value: `<code>${escapeHtml(track.relative_path || track.absolute_path || '')}</code>`, isHtml: true },
  ]
    .map((item) => `
      <div class="track-sidebar-meta-row">
        <div class="track-sidebar-label">${escapeHtml(item.label)}</div>
        <div class="track-sidebar-value">${item.isHtml ? item.value : escapeHtml(item.value)}</div>
      </div>
    `)
    .join('');

  const tagRows = (track.embedded_metadata?.fields || []).length
    ? track.embedded_metadata.fields
        .map((field) => `
          <div class="track-sidebar-tag">
            <div class="track-sidebar-label">${escapeHtml(field.key)}</div>
            <div class="track-sidebar-tag-value"><code>${escapeHtml(field.value)}</code></div>
          </div>
        `)
        .join('')
    : '<div class="track-sidebar-tag"><div class="track-sidebar-value">No embedded tag fields were parsed for this file.</div></div>';

  const notesHtml = (track.embedded_metadata?.notes || []).length
    ? `<ul class="track-sidebar-note-list">${track.embedded_metadata.notes.map((note) => `<li>${escapeHtml(note)}</li>`).join('')}</ul>`
    : '';

  host.innerHTML = `
    <h3>${escapeHtml(track.title || 'Track Tags')}</h3>
    <p class="meta">${escapeHtml(track.artist || 'Unknown artist')} · ${escapeHtml(track.album || 'Unknown album')}</p>
    ${artworkHtml}
    <div class="track-sidebar-actions">
      <a class="button-link secondary" href="/track/${encodeURIComponent(track.id)}" target="_blank" rel="noreferrer">Inspect</a>
      <a class="button-link secondary" href="/stream/track/${encodeURIComponent(track.id)}" target="_blank" rel="noreferrer">Preview</a>
    </div>
    <div class="track-sidebar-meta">${metaRows}</div>
    <div class="track-sidebar-tags">${tagRows}</div>
    ${notesHtml}
  `;
}

async function loadTrackDetails(trackId) {
  if (!trackId) {
    return;
  }
  try {
    const response = await fetch(`/api/tracks/${encodeURIComponent(trackId)}`);
    const payload = await response.json();
    renderTrackDetailPanel(payload);
  } catch (_error) {
    renderTrackDetailPanel({ error: 'Failed to load track details.' });
  }
}

function syncSelectedTrackSidebar() {
  const selected = document.querySelector('input[name="track_id"]:checked');
  if (selected) {
    loadTrackDetails(selected.value);
  }
}

/* ------------------------------------------------------- Library filter */
let libraryFilterTimer = null;

function getActiveFacet() {
  const active = document.querySelector('.filter-chips .chip.active');
  return active?.dataset?.facet || 'all';
}

function applyFacet(facet) {
  const sections = document.querySelectorAll('.library-section');
  for (const section of sections) {
    const sectionFacet = section.dataset.section;
    section.classList.toggle('facet-hidden', facet !== 'all' && sectionFacet !== facet);
  }
}

function filterLibrary() {
  const input = document.getElementById('library_filter');
  if (!input) {
    return;
  }
  if (libraryFilterTimer !== null) {
    clearTimeout(libraryFilterTimer);
  }
  libraryFilterTimer = setTimeout(() => {
    const nextUrl = new URL('/library', window.location.origin);
    const facet = getActiveFacet();
    const query = input.value.trim();
    const rendererLocation = document.body.dataset.rendererLocation || '';
    if (facet && facet !== 'all') {
      nextUrl.searchParams.set('facet', facet);
    }
    if (query) {
      nextUrl.searchParams.set('q', query);
    }
    if (rendererLocation) {
      nextUrl.searchParams.set('renderer_location', rendererLocation);
    }
    window.location.href = `${nextUrl.pathname}${nextUrl.search}`;
  }, 300);
}

function setupLibraryChips() {
  const chips = document.querySelectorAll('.filter-chips button.chip');
  if (!chips.length) {
    return;
  }
  for (const chip of chips) {
    chip.addEventListener('click', () => {
      for (const other of chips) {
        other.classList.toggle('active', other === chip);
        other.setAttribute('aria-selected', other === chip ? 'true' : 'false');
      }
      applyFacet(chip.dataset.facet || 'all');
    });
  }
  applyFacet(getActiveFacet());
}

let libraryLoadObserver = null;

function setupLibraryInfiniteScroll() {
  const loaders = document.querySelectorAll('[data-library-loader]');
  if (!loaders.length) {
    return;
  }
  if (!('IntersectionObserver' in window)) {
    for (const loader of loaders) {
      loader.hidden = true;
    }
    return;
  }
  libraryLoadObserver = new IntersectionObserver((entries) => {
    for (const entry of entries) {
      if (entry.isIntersecting) {
        loadMoreLibraryRows(entry.target);
      }
    }
  }, { rootMargin: '600px 0px' });
  for (const loader of loaders) {
    libraryLoadObserver.observe(loader);
  }
}

async function loadMoreLibraryRows(loader) {
  if (loader.dataset.loading === 'true') {
    return;
  }
  const target = document.getElementById(loader.dataset.targetId || '');
  if (!target) {
    loader.remove();
    return;
  }
  loader.dataset.loading = 'true';
  const params = new URLSearchParams();
  params.set('facet', loader.dataset.facet || '');
  params.set('offset', loader.dataset.offset || '0');
  if (loader.dataset.q) {
    params.set('q', loader.dataset.q);
  }
  if (loader.dataset.rendererLocation) {
    params.set('renderer_location', loader.dataset.rendererLocation);
  }

  try {
    const response = await fetch(`/library/rows?${params.toString()}`, {
      headers: { 'X-Requested-With': 'musicd-library-scroll' },
    });
    if (!response.ok) {
      throw new Error('request failed');
    }
    const payload = await response.json();
    if (!payload.ok) {
      throw new Error(payload.error || 'request failed');
    }
    if (payload.rows) {
      target.insertAdjacentHTML('beforeend', payload.rows);
      setupLikeButtons();
    }
    if (payload.has_more) {
      loader.dataset.offset = String(payload.next_offset);
      loader.dataset.loading = 'false';
    } else {
      libraryLoadObserver?.unobserve(loader);
      loader.remove();
    }
  } catch (_error) {
    loader.dataset.loading = 'false';
    loader.textContent = 'Could not load more items. Scroll to retry.';
  }
}

/* ---------------------------------------------------------- Queue panel */
let queueRefreshTimer = null;
let queueRefreshInFlight = false;

async function refreshQueuePanel() {
  const host = document.getElementById('queue_panel_host');
  if (!host) {
    return;
  }
  if (queueRefreshInFlight) {
    return;
  }
  const rendererLocation = host.dataset.rendererLocation || document.body.dataset.rendererLocation || '';
  queueRefreshInFlight = true;
  try {
    const url = rendererLocation
      ? `/queue/panel?renderer_location=${encodeURIComponent(rendererLocation)}&return_to=/queue`
      : '/queue/panel?return_to=/queue';
    const response = await fetch(url, {
      headers: { 'X-Requested-With': 'musicd-live-refresh' },
    });
    if (!response.ok) {
      return;
    }
    host.innerHTML = await response.text();
    syncRendererFields(rendererLocation);
  } catch (_error) {
    /* swallow transient network errors and try again on next tick */
  } finally {
    queueRefreshInFlight = false;
  }
}

function startQueueRefresh() {
  if (!document.getElementById('queue_panel_host')) {
    return;
  }
  if (queueRefreshTimer !== null) {
    clearInterval(queueRefreshTimer);
  }
  document.addEventListener('visibilitychange', () => {
    if (!document.hidden) {
      refreshQueuePanel();
    }
  });
  queueRefreshTimer = setInterval(() => {
    if (!document.hidden) {
      refreshQueuePanel();
    }
  }, 2500);
}

/* ------------------------------------------------------ Library rescan */
function parseEventData(event) {
  try {
    return JSON.parse(event.data || '{}');
  } catch (_error) {
    return {};
  }
}

function setRescanControls(scanning, message) {
  const button = document.getElementById('rescan_button');
  const progressContainer = document.getElementById('progress_bar_container');
  const status = document.getElementById('rescan_status');

  if (button) {
    button.disabled = scanning;
  }
  if (progressContainer) {
    progressContainer.style.display = scanning ? 'block' : 'none';
  }
  if (status && message) {
    status.textContent = message;
    status.classList.add('visually-hidden');
  }
}

function showRescanStatus(message) {
  const status = document.getElementById('rescan_status');
  if (status) {
    status.textContent = message;
    status.classList.remove('visually-hidden');
  }
}

function updateRescanProgress(data) {
  const progressBar = document.getElementById('rescan_progress_bar');
  const status = document.getElementById('rescan_status');

  if (progressBar) {
    let percent = Number(data.percent);
    if (!Number.isFinite(percent) && typeof data.total === 'number' && data.total > 0) {
      percent = Math.round((Number(data.current || 0) / data.total) * 100);
    }
    if (Number.isFinite(percent)) {
      progressBar.value = Math.max(0, Math.min(100, percent));
    }
  }

  if (data.message && status) {
    status.textContent = data.message;
  }
}

function setupRescanProgress() {
  const form = document.getElementById('rescan_form');
  if (!form) {
    return;
  }

  form.addEventListener('submit', (event) => {
    event.preventDefault();
    const progressBar = document.getElementById('rescan_progress_bar');
    if (progressBar) {
      progressBar.value = 0;
    }
    setRescanControls(true, 'Starting library scan...');

    const params = new URLSearchParams(new FormData(form));
    const progressUrl = form.dataset.progressUrl || form.action;
    const url = `${progressUrl}?${params.toString()}`;
    const source = new EventSource(url);
    let closed = false;

    const closeSource = () => {
      closed = true;
      source.close();
    };

    source.addEventListener('scan_start', () => {
      if (progressBar) {
        progressBar.value = 1;
      }
      setRescanControls(true, 'Scanning library...');
    });

    source.addEventListener('scan_progress', (scanEvent) => {
      updateRescanProgress(parseEventData(scanEvent));
    });

    source.addEventListener('scan_complete', (scanEvent) => {
      const data = parseEventData(scanEvent);
      updateRescanProgress(data);
      closeSource();
      window.setTimeout(() => window.location.reload(), 800);
    });

    source.addEventListener('scan_error', (scanEvent) => {
      const data = parseEventData(scanEvent);
      closeSource();
      setRescanControls(false, '');
      showRescanStatus(data.message || 'Library rescan failed.');
    });

    source.onerror = () => {
      if (closed) {
        return;
      }
      closeSource();
      setRescanControls(false, '');
      showRescanStatus('Connection lost. Please refresh the page.');
    };
  });
}

/* ------------------------------------------------------------ Radio */
function radioRendererLocation(form) {
  return new FormData(form).get('renderer_location') || document.getElementById('renderer_location')?.value || '';
}

function setRadioStatus(message, isError = false) {
  const status = document.getElementById('radio_status');
  if (!status) {
    return;
  }
  status.textContent = message;
  status.classList.toggle('error-text', isError);
}

function renderRadioStations(stations) {
  const host = document.getElementById('radio_results');
  if (!host) {
    return;
  }
  if (!stations.length) {
    host.innerHTML = '<p class="empty">No stations matched that search.</p>';
    return;
  }
  host.innerHTML = stations.map((station) => {
    const tags = (station.tags || []).slice(0, 3).map((tag) => `<span class="chip mini">${escapeHtml(tag)}</span>`).join('');
    const meta = [
      station.country_code,
      station.language,
      station.codec,
      station.bitrate ? `${station.bitrate} kbps` : '',
    ].filter(Boolean).join(' · ');
    const art = station.artwork_url
      ? `<img class="radio-art" src="${escapeHtml(station.artwork_url)}" alt="">`
      : '<div class="radio-art placeholder">Radio</div>';
    return `
      <article class="radio-result">
        ${art}
        <div class="radio-result-main">
          <h3>${escapeHtml(station.name || 'Untitled station')}</h3>
          <p class="meta">${escapeHtml(meta || station.stream_url || '')}</p>
          <div class="radio-tags">${tags}</div>
        </div>
        <button type="button" onclick='playRadioStation(${escapeHtml(JSON.stringify(station))})'>Play</button>
      </article>
    `;
  }).join('');
}

async function searchRadioStations(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const params = new URLSearchParams(new FormData(form));
  params.set('limit', '12');
  setRadioStatus('Searching stations...');
  try {
    const response = await fetch(`/api/radio/stations?${params.toString()}`);
    const payload = await response.json();
    if (!response.ok) {
      throw new Error(payload.error || 'Station search failed');
    }
    renderRadioStations(payload);
    setRadioStatus(`${payload.length} station${payload.length === 1 ? '' : 's'} found.`);
  } catch (error) {
    renderRadioStations([]);
    setRadioStatus(error.message || 'Station search failed.', true);
  }
}

async function playRadioStation(station) {
  const rendererLocation = document.querySelector('.renderer-location-proxy')?.value || document.getElementById('renderer_location')?.value || '';
  await submitRadioPlay({
    renderer_location: rendererLocation,
    stream_url: station.stream_url,
    station_name: station.name,
    station_id: station.id,
    artwork_url: station.artwork_url || '',
    codec: station.codec || '',
  });
}

async function playDirectRadioStream(event) {
  event.preventDefault();
  const form = event.currentTarget;
  const data = Object.fromEntries(new FormData(form).entries());
  data.renderer_location = radioRendererLocation(form);
  await submitRadioPlay(data);
}

async function submitRadioPlay(data) {
  if (!data.renderer_location) {
    setRadioStatus('Pick a renderer before starting radio.', true);
    return;
  }
  if (!data.stream_url) {
    setRadioStatus('Enter a stream URL first.', true);
    return;
  }
  const form = new URLSearchParams();
  for (const [key, value] of Object.entries(data)) {
    if (value !== undefined && value !== null && String(value).trim() !== '') {
      form.set(key, value);
    }
  }
  setRadioStatus('Starting radio...');
  try {
    const response = await fetch('/api/radio/play', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
      body: form.toString(),
    });
    const payload = await response.json();
    if (!response.ok || !payload.ok) {
      throw new Error(payload.error || 'Radio playback failed');
    }
    setRadioStatus(payload.message || 'Radio started.');
    startQueueRefresh();
  } catch (error) {
    setRadioStatus(error.message || 'Radio playback failed.', true);
  }
}

/* --------------------------------------------------------------- Boot */
document.addEventListener('change', (event) => {
  if (event.target instanceof HTMLInputElement && event.target.name === 'track_id') {
    loadTrackDetails(event.target.value);
  }
});

setupLibraryChips();
setupLibraryInfiniteScroll();
setupLikeButtons();
syncSelectedTrackSidebar();
startQueueRefresh();
setupRescanProgress();
