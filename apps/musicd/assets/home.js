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

    function renderTrackDetailPanel(track) {
      const host = document.getElementById('track_detail_panel');
      if (!host) {
        return;
      }
      if (!track || track.error) {
        host.innerHTML = `<h3>Track Tags</h3><p>${escapeHtml(track?.error || 'Track details are unavailable.')}</p>`;
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
        <p>${escapeHtml(track.artist || 'Unknown artist')} • ${escapeHtml(track.album || 'Unknown album')}</p>
        ${artworkHtml}
        <div class="track-sidebar-actions">
          <a class="button-link secondary" href="/track/${encodeURIComponent(track.id)}" target="_blank" rel="noreferrer">Full Inspector</a>
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

    async function discoverRenderers() {
      const select = document.getElementById('renderer_discovery');
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
      } catch (error) {
        select.innerHTML = '<option value="">Discovery failed</option>';
      }
    }

    function applySelectedRenderer() {
      const select = document.getElementById('renderer_discovery');
      const input = document.getElementById('renderer_location');
      if (select.value) {
        input.value = select.value;
        syncRendererFields(select.value);
        refreshQueuePanel();
      }
    }

    function syncRendererFields(value) {
      const hidden = document.getElementById('rescan_renderer_location');
      if (hidden) {
        hidden.value = value;
      }
      const proxies = document.querySelectorAll('.renderer-location-proxy');
      for (const proxy of proxies) {
        proxy.value = value;
      }
    }

    let queueRefreshTimer = null;
    let queueRefreshInFlight = false;

    async function refreshQueuePanel() {
      const rendererInput = document.getElementById('renderer_location');
      const host = document.getElementById('queue_panel_host');
      if (!rendererInput || !host) {
        return;
      }
      const rendererLocation = rendererInput.value.trim();
      if (queueRefreshInFlight) {
        return;
      }
      queueRefreshInFlight = true;
      try {
        const url = rendererLocation
          ? `/queue/panel?renderer_location=${encodeURIComponent(rendererLocation)}`
          : '/queue/panel';
        const response = await fetch(url, {
          headers: {
            'X-Requested-With': 'musicd-live-refresh'
          }
        });
        if (!response.ok) {
          return;
        }
        host.innerHTML = await response.text();
        syncRendererFields(rendererLocation);
      } catch (_error) {
      } finally {
        queueRefreshInFlight = false;
      }
    }

    function startQueueRefresh() {
      if (queueRefreshTimer !== null) {
        clearInterval(queueRefreshTimer);
      }
      document.addEventListener('visibilitychange', () => {
        if (!document.hidden) {
          refreshQueuePanel();
        }
      });
      queueRefreshTimer = setInterval(() => {
        if (document.hidden) {
          return;
        }
        refreshQueuePanel();
      }, 2500);
    }

    function filterTracks() {
      const needle = document.getElementById('track_filter').value.trim().toLowerCase();
      const rows = document.querySelectorAll('#track_table tr');
      for (const row of rows) {
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }
      const albumRows = document.querySelectorAll('#album_table tr');
      for (const row of albumRows) {
        row.style.display = !needle || row.dataset.search.includes(needle) ? '' : 'none';
      }
    }

    document.addEventListener('change', (event) => {
      if (event.target instanceof HTMLInputElement && event.target.name === 'track_id') {
        loadTrackDetails(event.target.value);
      }
    });

    refreshQueuePanel();
    startQueueRefresh();
    syncSelectedTrackSidebar();
