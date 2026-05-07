use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::Terminal;
use ratatui::backend::Backend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph, Tabs, Wrap};

use crate::api::{AlbumSummary, ApiClient, Queue, Renderer, ServerInfo, Session, TrackSummary};
use crate::config::{self, CliConfig};
use crate::local_audio::LocalAudioPlayer;

const TICK: Duration = Duration::from_millis(750);
const STATUS_TTL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Albums,
    Tracks,
    Queue,
    Renderers,
}

impl Tab {
    fn all() -> [Tab; 4] {
        [Tab::Albums, Tab::Tracks, Tab::Queue, Tab::Renderers]
    }

    fn title(self) -> &'static str {
        match self {
            Tab::Albums => "Albums",
            Tab::Tracks => "Tracks",
            Tab::Queue => "Queue",
            Tab::Renderers => "Renderers",
        }
    }

    fn index(self) -> usize {
        Tab::all().iter().position(|t| *t == self).unwrap_or(0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Normal,
    Search,
}

pub struct App {
    api: ApiClient,
    config: CliConfig,
    server: Option<ServerInfo>,

    tab: Tab,
    mode: Mode,

    albums: Vec<AlbumSummary>,
    tracks: Vec<TrackSummary>,
    renderers: Vec<Renderer>,
    queue: Queue,

    selected_renderer: Option<Renderer>,

    search: String,
    filtered_albums: Vec<usize>,
    filtered_tracks: Vec<usize>,

    list_state: ListState,

    status: Option<(StatusKind, String, Instant)>,
    should_quit: bool,
    last_queue_poll: Instant,
    local_audio: LocalAudioPlayer,
}

#[derive(Debug, Clone, Copy)]
enum StatusKind {
    Info,
    Error,
}

impl App {
    pub fn new(api: ApiClient, config: CliConfig) -> Self {
        let mut state = ListState::default();
        state.select(Some(0));
        Self {
            api,
            config,
            server: None,
            tab: Tab::Albums,
            mode: Mode::Normal,
            albums: Vec::new(),
            tracks: Vec::new(),
            renderers: Vec::new(),
            queue: Queue::default(),
            selected_renderer: None,
            search: String::new(),
            filtered_albums: Vec::new(),
            filtered_tracks: Vec::new(),
            list_state: state,
            status: None,
            should_quit: false,
            last_queue_poll: Instant::now() - TICK,
            local_audio: LocalAudioPlayer::default(),
        }
    }

    pub fn run<B: Backend>(&mut self, terminal: &mut Terminal<B>) -> Result<()> {
        self.bootstrap();
        while !self.should_quit {
            terminal.draw(|frame| self.draw(frame.area(), frame))?;

            if event::poll(TICK)? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == event::KeyEventKind::Press {
                        self.handle_key(key);
                    }
                }
            }

            if self.last_queue_poll.elapsed() >= TICK {
                self.poll_queue();
                self.last_queue_poll = Instant::now();
            }

            if let Some((_, _, at)) = &self.status {
                if at.elapsed() >= STATUS_TTL {
                    self.status = None;
                }
            }
        }
        Ok(())
    }

    fn bootstrap(&mut self) {
        match self.api.server_info() {
            Ok(info) => self.server = Some(info),
            Err(err) => self.set_error(format!("server: {err:#}")),
        }
        if let Err(err) = self.api.register_cli_local_renderer("This CLI") {
            self.set_error(format!("local renderer: {err:#}"));
        }
        self.refresh_renderers();
        self.resolve_selected_renderer();
        self.refresh_library();
        self.poll_queue();
        self.recompute_filter();
        self.fix_selection();
    }

    fn refresh_library(&mut self) {
        match self.api.list_albums() {
            Ok(list) => self.albums = list,
            Err(err) => self.set_error(format!("albums: {err:#}")),
        }
        match self.api.list_tracks() {
            Ok(list) => self.tracks = list,
            Err(err) => self.set_error(format!("tracks: {err:#}")),
        }
        self.recompute_filter();
    }

    fn refresh_renderers(&mut self) {
        match self.api.list_renderers() {
            Ok(list) => {
                self.renderers = list;
                self.resolve_selected_renderer();
            }
            Err(err) => self.set_error(format!("renderers: {err:#}")),
        }
    }

    fn resolve_selected_renderer(&mut self) {
        if let Some(loc) = &self.config.renderer_location {
            self.selected_renderer = self.renderers.iter().find(|r| &r.location == loc).cloned();
        }
        if self.selected_renderer.is_none() {
            self.selected_renderer = self.renderers.iter().find(|r| r.selected).cloned();
        }
    }

    fn poll_queue(&mut self) {
        let Some(renderer) = self.selected_renderer.clone() else {
            self.local_audio.sync(&self.api, None, &self.queue);
            return;
        };
        match self.api.queue(&renderer.location) {
            Ok(q) => {
                if self.local_audio.sync(&self.api, Some(&renderer), &q) {
                    match self.api.queue(&renderer.location) {
                        Ok(fresh_queue) => {
                            self.local_audio
                                .sync(&self.api, Some(&renderer), &fresh_queue);
                            self.queue = fresh_queue;
                        }
                        Err(err) => {
                            self.queue = q;
                            self.set_error(format!("queue refresh: {err:#}"));
                        }
                    }
                } else {
                    self.queue = q;
                }
            }
            Err(err) => self.set_error(format!("queue: {err:#}")),
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.mode == Mode::Search {
            self.handle_search_key(key);
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            KeyCode::Tab => self.cycle_tab(1),
            KeyCode::BackTab => self.cycle_tab(-1),
            KeyCode::Char('1') => self.set_tab(Tab::Albums),
            KeyCode::Char('2') => self.set_tab(Tab::Tracks),
            KeyCode::Char('3') => self.set_tab(Tab::Queue),
            KeyCode::Char('4') => self.set_tab(Tab::Renderers),
            KeyCode::Char('/') if matches!(self.tab, Tab::Albums | Tab::Tracks) => {
                self.mode = Mode::Search;
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::Home => self.set_selection(0),
            KeyCode::End => self.set_selection(isize::MAX),
            KeyCode::Enter => self.activate_selection(),
            KeyCode::Char('a') => self.append_selection(),
            KeyCode::Char(' ') => self.toggle_play_pause(),
            KeyCode::Char('n') => self.transport(|api, loc| api.transport_next(loc), "Next"),
            KeyCode::Char('p') => {
                self.transport(|api, loc| api.transport_previous(loc), "Previous")
            }
            KeyCode::Char('s') => self.transport(|api, loc| api.transport_stop(loc), "Stopped"),
            KeyCode::Char('C') => self.transport(|api, loc| api.queue_clear(loc), "Queue cleared"),
            KeyCode::Char('r') => {
                self.refresh_library();
                self.refresh_renderers();
                self.poll_queue();
                self.set_info("Refreshed");
            }
            KeyCode::Char('D') => self.discover(),
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::Normal;
                self.search.clear();
                self.recompute_filter();
                self.set_selection(0);
            }
            KeyCode::Enter => {
                self.mode = Mode::Normal;
                self.set_selection(0);
            }
            KeyCode::Backspace => {
                self.search.pop();
                self.recompute_filter();
                self.set_selection(0);
            }
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.search.push(c);
                self.recompute_filter();
                self.set_selection(0);
            }
            _ => {}
        }
    }

    fn cycle_tab(&mut self, delta: i32) {
        let tabs = Tab::all();
        let idx = self.tab.index() as i32;
        let next = (idx + delta).rem_euclid(tabs.len() as i32) as usize;
        self.set_tab(tabs[next]);
    }

    fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.set_selection(0);
    }

    fn current_len(&self) -> usize {
        match self.tab {
            Tab::Albums => self.filtered_albums.len(),
            Tab::Tracks => self.filtered_tracks.len(),
            Tab::Queue => self.queue.entries.len(),
            Tab::Renderers => self.renderers.len(),
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let len = self.current_len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        let cur = self.list_state.selected().unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, len as isize - 1) as usize;
        self.list_state.select(Some(next));
    }

    fn set_selection(&mut self, idx: isize) {
        let len = self.current_len();
        if len == 0 {
            self.list_state.select(None);
            return;
        }
        let next = idx.clamp(0, len as isize - 1) as usize;
        self.list_state.select(Some(next));
    }

    fn fix_selection(&mut self) {
        let len = self.current_len();
        match self.list_state.selected() {
            None if len > 0 => self.list_state.select(Some(0)),
            Some(idx) if idx >= len => self.set_selection(0),
            _ => {}
        }
    }

    fn activate_selection(&mut self) {
        let Some(idx) = self.list_state.selected() else {
            return;
        };
        match self.tab {
            Tab::Albums => {
                let Some(&album_idx) = self.filtered_albums.get(idx) else {
                    return;
                };
                let album = self.albums[album_idx].clone();
                let Some(loc) = self.renderer_location() else {
                    return;
                };
                match self.api.play_album(&loc, &album.id) {
                    Ok(()) => {
                        self.set_info(format!("Playing album: {}", album.title));
                        self.poll_queue();
                    }
                    Err(err) => self.set_error(format!("play album: {err:#}")),
                }
            }
            Tab::Tracks => {
                let Some(&track_idx) = self.filtered_tracks.get(idx) else {
                    return;
                };
                let track = self.tracks[track_idx].clone();
                let Some(loc) = self.renderer_location() else {
                    return;
                };
                match self.api.play_track(&loc, &track.id) {
                    Ok(()) => {
                        self.set_info(format!("Playing: {}", track.title));
                        self.poll_queue();
                    }
                    Err(err) => self.set_error(format!("play track: {err:#}")),
                }
            }
            Tab::Renderers => {
                let renderer = self.renderers[idx].clone();
                self.config.renderer_location = Some(renderer.location.clone());
                if let Err(err) = config::save(&self.config) {
                    self.set_error(format!("save config: {err:#}"));
                }
                self.selected_renderer = Some(renderer.clone());
                self.set_info(format!("Selected renderer: {}", renderer.name));
                self.poll_queue();
            }
            Tab::Queue => {}
        }
    }

    fn append_selection(&mut self) {
        let Some(idx) = self.list_state.selected() else {
            return;
        };
        let Some(loc) = self.renderer_location() else {
            return;
        };
        match self.tab {
            Tab::Albums => {
                let Some(&album_idx) = self.filtered_albums.get(idx) else {
                    return;
                };
                let album = self.albums[album_idx].clone();
                match self.api.append_album(&loc, &album.id) {
                    Ok(()) => {
                        self.set_info(format!("Queued album: {}", album.title));
                        self.poll_queue();
                    }
                    Err(err) => self.set_error(format!("append album: {err:#}")),
                }
            }
            Tab::Tracks => {
                let Some(&track_idx) = self.filtered_tracks.get(idx) else {
                    return;
                };
                let track = self.tracks[track_idx].clone();
                match self.api.append_track(&loc, &track.id) {
                    Ok(()) => {
                        self.set_info(format!("Queued: {}", track.title));
                        self.poll_queue();
                    }
                    Err(err) => self.set_error(format!("append track: {err:#}")),
                }
            }
            _ => {}
        }
    }

    fn toggle_play_pause(&mut self) {
        let Some(loc) = self.renderer_location() else {
            return;
        };
        let state = self
            .queue
            .session
            .as_ref()
            .map(|s| s.transport_state.to_ascii_uppercase())
            .unwrap_or_default();
        let result = if state == "PLAYING" {
            self.api.transport_pause(&loc).map(|_| "Paused")
        } else {
            self.api.transport_play(&loc).map(|_| "Playing")
        };
        match result {
            Ok(label) => {
                self.set_info(label);
                self.poll_queue();
            }
            Err(err) => self.set_error(format!("transport: {err:#}")),
        }
    }

    fn transport<F>(&mut self, f: F, label: &str)
    where
        F: FnOnce(&ApiClient, &str) -> Result<()>,
    {
        let Some(loc) = self.renderer_location() else {
            return;
        };
        match f(&self.api, &loc) {
            Ok(()) => {
                self.set_info(label.to_string());
                self.poll_queue();
            }
            Err(err) => self.set_error(format!("{label}: {err:#}")),
        }
    }

    fn discover(&mut self) {
        match self.api.discover_renderers() {
            Ok(list) => {
                self.renderers = list;
                self.resolve_selected_renderer();
                self.set_info(format!("Discovered {} renderer(s)", self.renderers.len()));
            }
            Err(err) => self.set_error(format!("discover: {err:#}")),
        }
    }

    fn renderer_location(&mut self) -> Option<String> {
        match self.selected_renderer.as_ref() {
            Some(r) => Some(r.location.clone()),
            None => {
                self.set_error("No renderer selected. Press 4 then Enter to pick one.");
                None
            }
        }
    }

    fn recompute_filter(&mut self) {
        let q = self.search.to_ascii_lowercase();
        self.filtered_albums = (0..self.albums.len())
            .filter(|&i| {
                let a = &self.albums[i];
                if q.is_empty() {
                    true
                } else {
                    a.title.to_ascii_lowercase().contains(&q)
                        || a.artist.to_ascii_lowercase().contains(&q)
                }
            })
            .collect();
        self.filtered_tracks = (0..self.tracks.len())
            .filter(|&i| {
                let t = &self.tracks[i];
                if q.is_empty() {
                    true
                } else {
                    t.title.to_ascii_lowercase().contains(&q)
                        || t.artist.to_ascii_lowercase().contains(&q)
                        || t.album.to_ascii_lowercase().contains(&q)
                }
            })
            .collect();
        self.fix_selection();
    }

    fn set_info(&mut self, msg: impl Into<String>) {
        self.status = Some((StatusKind::Info, msg.into(), Instant::now()));
    }

    fn set_error(&mut self, msg: impl Into<String>) {
        self.status = Some((StatusKind::Error, msg.into(), Instant::now()));
    }

    fn draw(&mut self, area: Rect, frame: &mut ratatui::Frame) {
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(3),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .split(area);

        self.draw_header(layout[0], frame);
        self.draw_tabs(layout[1], frame);
        self.draw_body(layout[2], frame);
        self.draw_now_playing(layout[3], frame);
        self.draw_footer(layout[4], frame);
    }

    fn draw_header(&self, area: Rect, frame: &mut ratatui::Frame) {
        let server = self
            .server
            .as_ref()
            .map(|s| format!("{} @ {}", s.name, s.base_url))
            .unwrap_or_else(|| format!("server: {}", self.api.base_url()));
        let renderer = match &self.selected_renderer {
            Some(r) => format!("Renderer: {}", r.name),
            None => "Renderer: (none — press 4 then Enter)".to_string(),
        };
        let line = Line::from(vec![
            Span::styled(server, Style::default().fg(Color::Cyan)),
            Span::raw("   "),
            Span::styled(
                renderer,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
        ]);
        let p = Paragraph::new(line).block(Block::default().borders(Borders::ALL).title("musicd"));
        frame.render_widget(p, area);
    }

    fn draw_tabs(&self, area: Rect, frame: &mut ratatui::Frame) {
        let titles: Vec<Line> = Tab::all()
            .iter()
            .enumerate()
            .map(|(i, t)| Line::from(format!("{} {}", i + 1, t.title())))
            .collect();
        let tabs = Tabs::new(titles)
            .select(self.tab.index())
            .block(Block::default().borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            );
        frame.render_widget(tabs, area);
    }

    fn draw_body(&mut self, area: Rect, frame: &mut ratatui::Frame) {
        match self.tab {
            Tab::Albums | Tab::Tracks => {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(3), Constraint::Min(0)])
                    .split(area);
                self.draw_search(chunks[0], frame);
                self.draw_list(chunks[1], frame);
            }
            _ => self.draw_list(area, frame),
        }
    }

    fn draw_search(&self, area: Rect, frame: &mut ratatui::Frame) {
        let active = self.mode == Mode::Search;
        let cursor = if active { "█" } else { "" };
        let body = format!("{}{}", self.search, cursor);
        let style = if active {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        let title = if active {
            "search (Esc to cancel)"
        } else {
            "search (/)"
        };
        let p = Paragraph::new(body)
            .style(style)
            .block(Block::default().borders(Borders::ALL).title(title));
        frame.render_widget(p, area);
    }

    fn draw_list(&mut self, area: Rect, frame: &mut ratatui::Frame) {
        let (items, title): (Vec<ListItem>, String) = match self.tab {
            Tab::Albums => (
                self.filtered_albums
                    .iter()
                    .map(|&i| {
                        let a = &self.albums[i];
                        ListItem::new(format!("{}  ·  {}", a.title, a.artist))
                    })
                    .collect(),
                format!(
                    "Albums ({}/{})",
                    self.filtered_albums.len(),
                    self.albums.len()
                ),
            ),
            Tab::Tracks => (
                self.filtered_tracks
                    .iter()
                    .map(|&i| {
                        let t = &self.tracks[i];
                        let dur = t
                            .duration_seconds
                            .map(format_secs)
                            .unwrap_or_else(|| "—".to_string());
                        ListItem::new(format!(
                            "{:>6}  {}  ·  {}  ·  {}",
                            dur, t.title, t.artist, t.album
                        ))
                    })
                    .collect(),
                format!(
                    "Tracks ({}/{})",
                    self.filtered_tracks.len(),
                    self.tracks.len()
                ),
            ),
            Tab::Queue => {
                let items = self
                    .queue
                    .entries
                    .iter()
                    .map(|e| {
                        let marker = if Some(e.id) == self.queue.current_entry_id {
                            "▶ "
                        } else {
                            "  "
                        };
                        let title = e.title.clone().unwrap_or_else(|| "(unknown)".into());
                        let artist = e.artist.clone().unwrap_or_default();
                        ListItem::new(format!(
                            "{}{}. {}  ·  {}",
                            marker,
                            e.position + 1,
                            title,
                            artist
                        ))
                    })
                    .collect();
                let status = if self.queue.status.is_empty() {
                    "queue".to_string()
                } else {
                    format!("queue [{}]", self.queue.status)
                };
                (
                    items,
                    format!("{} — {} entries", status, self.queue.entries.len()),
                )
            }
            Tab::Renderers => {
                let items = self
                    .renderers
                    .iter()
                    .map(|r| {
                        let active = self
                            .selected_renderer
                            .as_ref()
                            .map(|s| s.location == r.location)
                            .unwrap_or(false);
                        let marker = if active { "● " } else { "  " };
                        let kind = r.kind.clone().unwrap_or_else(|| "?".into());
                        let health = match &r.health {
                            Some(h) if h.reachable => "ok",
                            Some(_) => "down",
                            None => "?",
                        };
                        ListItem::new(format!("{}{}  [{}]  {}", marker, r.name, kind, health))
                    })
                    .collect();
                (
                    items,
                    format!(
                        "Renderers ({})  D=discover  Enter=select",
                        self.renderers.len()
                    ),
                )
            }
        };

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::DarkGray)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");
        frame.render_stateful_widget(list, area, &mut self.list_state);
    }

    fn draw_now_playing(&self, area: Rect, frame: &mut ratatui::Frame) {
        let session = self.queue.session.clone().unwrap_or_default();
        let line = format_now_playing(&session);
        let style = match session.transport_state.to_ascii_uppercase().as_str() {
            "PLAYING" => Style::default().fg(Color::Green),
            "PAUSED" => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::DarkGray),
        };
        let p = Paragraph::new(Line::from(Span::styled(line, style)))
            .block(Block::default().borders(Borders::ALL).title("now"))
            .wrap(Wrap { trim: true });
        frame.render_widget(p, area);
    }

    fn draw_footer(&self, area: Rect, frame: &mut ratatui::Frame) {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(0), Constraint::Length(40)])
            .split(area);

        let hints = match self.mode {
            Mode::Search => "type to filter   Esc cancel   Enter accept",
            Mode::Normal => match self.tab {
                Tab::Albums | Tab::Tracks => {
                    "↑↓ move  / search  Enter play  a queue  Space ⏯  n/p next/prev  s stop  C clear  r reload  ⇥ tab  q quit"
                }
                Tab::Queue => "Space ⏯  n/p next/prev  s stop  C clear  r reload  ⇥ tab  q quit",
                Tab::Renderers => "↑↓ move  Enter select  D discover  r reload  ⇥ tab  q quit",
            },
        };
        let hints_p = Paragraph::new(hints).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(hints_p, layout[0]);

        let status = match &self.status {
            Some((StatusKind::Info, msg, _)) => {
                Span::styled(msg.clone(), Style::default().fg(Color::Green))
            }
            Some((StatusKind::Error, msg, _)) => {
                Span::styled(msg.clone(), Style::default().fg(Color::Red))
            }
            None => self
                .local_audio
                .status()
                .map(|status| Span::styled(status.to_string(), Style::default().fg(Color::Cyan)))
                .unwrap_or_else(|| Span::raw("")),
        };
        let status_p =
            Paragraph::new(Line::from(status)).alignment(ratatui::layout::Alignment::Right);
        frame.render_widget(status_p, layout[1]);
    }
}

fn format_now_playing(session: &Session) -> String {
    let state = session.transport_state.to_ascii_uppercase();
    let icon = match state.as_str() {
        "PLAYING" => "▶",
        "PAUSED" => "⏸",
        "STOPPED" | "" => "■",
        _ => "?",
    };
    let title = session.title.clone().unwrap_or_else(|| "(idle)".into());
    let artist = session.artist.clone().unwrap_or_default();
    let album = session.album.clone().unwrap_or_default();
    let pos = session
        .position_seconds
        .map(format_secs)
        .unwrap_or_default();
    let dur = session
        .duration_seconds
        .map(format_secs)
        .unwrap_or_default();
    let time = if pos.is_empty() && dur.is_empty() {
        String::new()
    } else {
        format!("   {pos} / {dur}")
    };
    let parts = [artist, album]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if parts.is_empty() {
        format!("{icon}  {title}{time}")
    } else {
        format!("{icon}  {title}  ·  {parts}{time}")
    }
}

fn format_secs(s: u64) -> String {
    let m = s / 60;
    let r = s % 60;
    format!("{m:02}:{r:02}")
}
