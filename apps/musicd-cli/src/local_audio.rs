use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::api::{ApiClient, Queue, Renderer};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlaybackKey {
    renderer_location: String,
    queue_entry_id: Option<i64>,
    uri: String,
}

struct ActivePlayback {
    key: PlaybackKey,
    child: Child,
    temp_path: Option<PathBuf>,
    paused: bool,
}

struct StartMessage {
    key: PlaybackKey,
    result: Result<StartedPlayback, String>,
}

struct StartedPlayback {
    child: Child,
    temp_path: Option<PathBuf>,
}

#[derive(Default)]
pub(crate) struct LocalAudioPlayer {
    active: Option<ActivePlayback>,
    pending: Option<PlaybackKey>,
    start_rx: Option<Receiver<StartMessage>>,
    last_error: Option<String>,
    last_status: Option<String>,
    last_session_report: Option<Instant>,
}

impl LocalAudioPlayer {
    pub(crate) fn sync(
        &mut self,
        api: &ApiClient,
        renderer: Option<&Renderer>,
        queue: &Queue,
    ) -> bool {
        self.accept_started_playback(api);
        if self.reap_finished_playback(api) {
            return true;
        }

        let Some(renderer) = renderer else {
            self.stop(api);
            return false;
        };
        let local_renderer_location = api.cli_local_renderer_location();
        if renderer.location != local_renderer_location {
            self.stop(api);
            return false;
        }

        let Some(session) = queue.session.as_ref() else {
            self.stop(api);
            return false;
        };
        if !matches!(
            session.transport_state.as_str(),
            "PLAYING" | "PAUSED_PLAYBACK"
        ) {
            self.stop(api);
            return false;
        }
        let Some(uri) = session
            .current_track_uri
            .as_deref()
            .filter(|uri| !uri.is_empty())
        else {
            self.stop(api);
            return false;
        };

        let key = PlaybackKey {
            renderer_location: renderer.location.clone(),
            queue_entry_id: session.queue_entry_id,
            uri: uri.to_string(),
        };
        if self.active.as_ref().is_some_and(|active| active.key == key) {
            if session.transport_state == "PAUSED_PLAYBACK" {
                self.pause_active(api);
            } else {
                self.resume_active(api);
                self.report_playing_session(api, session.duration_seconds);
            }
            return false;
        }
        if self.pending.as_ref() == Some(&key) {
            return false;
        }

        self.stop(api);
        if session.transport_state == "PAUSED_PLAYBACK" {
            self.last_status = Some("Local audio paused.".to_string());
            return false;
        }
        self.start(key);
        false
    }

    pub(crate) fn status(&self) -> Option<&str> {
        self.last_error.as_deref().or(self.last_status.as_deref())
    }

    fn start(&mut self, key: PlaybackKey) {
        let (tx, rx) = mpsc::channel();
        let thread_key = key.clone();
        thread::spawn(move || {
            let result = start_playback(&thread_key.uri);
            let _ = tx.send(StartMessage {
                key: thread_key,
                result,
            });
        });
        self.pending = Some(key);
        self.start_rx = Some(rx);
        self.last_error = None;
        self.last_status = Some("Preparing local audio...".to_string());
    }

    fn accept_started_playback(&mut self, api: &ApiClient) {
        let Some(rx) = self.start_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(message) => {
                self.pending = None;
                match message.result {
                    Ok(started) => {
                        let _ = api.report_cli_local_session(
                            &message.key.renderer_location,
                            "PLAYING",
                            Some(&message.key.uri),
                            None,
                        );
                        self.active = Some(ActivePlayback {
                            key: message.key,
                            child: started.child,
                            temp_path: started.temp_path,
                            paused: false,
                        });
                        self.last_error = None;
                        self.last_status = Some("Local audio playing.".to_string());
                        self.last_session_report = Some(Instant::now());
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        self.last_status = None;
                    }
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.start_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.pending = None;
                self.last_error = Some("Local audio player failed to start.".to_string());
            }
        }
    }

    fn reap_finished_playback(&mut self, api: &ApiClient) -> bool {
        let Some(active) = self.active.as_mut() else {
            return false;
        };
        match active.child.try_wait() {
            Ok(Some(status)) => {
                let mut active = self.active.take().expect("active playback exists");
                cleanup_temp(active.temp_path.take());
                if status.success() {
                    let _ = api.report_cli_local_completed(&active.key.renderer_location);
                    self.last_status = Some("Local queue entry completed.".to_string());
                    self.last_error = None;
                    true
                } else {
                    let _ = api.report_cli_local_session(
                        &active.key.renderer_location,
                        "STOPPED",
                        Some(&active.key.uri),
                        None,
                    );
                    self.last_error = Some(format!("Local audio player exited with {status}."));
                    self.last_status = None;
                    false
                }
            }
            Ok(None) => false,
            Err(error) => {
                self.last_error = Some(format!("Local audio player status failed: {error}"));
                false
            }
        }
    }

    fn report_playing_session(&mut self, api: &ApiClient, duration_seconds: Option<u64>) {
        if self
            .last_session_report
            .is_some_and(|last| last.elapsed().as_secs() < 10)
        {
            return;
        }
        if let Some(active) = self.active.as_ref() {
            let _ = api.report_cli_local_session(
                &active.key.renderer_location,
                "PLAYING",
                Some(&active.key.uri),
                duration_seconds,
            );
            self.last_session_report = Some(Instant::now());
        }
    }

    fn pause_active(&mut self, api: &ApiClient) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if active.paused {
            return;
        }
        match signal_child(&active.child, ChildSignal::Pause) {
            Ok(()) => {
                active.paused = true;
                let _ = api.report_cli_local_session(
                    &active.key.renderer_location,
                    "PAUSED_PLAYBACK",
                    Some(&active.key.uri),
                    None,
                );
                self.last_status = Some("Local audio paused.".to_string());
                self.last_error = None;
            }
            Err(error) => {
                self.last_error = Some(error);
            }
        }
    }

    fn resume_active(&mut self, api: &ApiClient) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        if !active.paused {
            return;
        }
        match signal_child(&active.child, ChildSignal::Resume) {
            Ok(()) => {
                active.paused = false;
                let _ = api.report_cli_local_session(
                    &active.key.renderer_location,
                    "PLAYING",
                    Some(&active.key.uri),
                    None,
                );
                self.last_status = Some("Local audio playing.".to_string());
                self.last_error = None;
                self.last_session_report = Some(Instant::now());
            }
            Err(error) => {
                self.last_error = Some(error);
            }
        }
    }

    fn stop(&mut self, api: &ApiClient) {
        self.pending = None;
        self.start_rx = None;
        let Some(mut active) = self.active.take() else {
            return;
        };
        let _ = active.child.kill();
        let _ = active.child.wait();
        cleanup_temp(active.temp_path.take());
        let _ = api.report_cli_local_session(
            &active.key.renderer_location,
            "STOPPED",
            Some(&active.key.uri),
            None,
        );
        self.last_status = Some("Local audio stopped.".to_string());
    }
}

enum ChildSignal {
    Pause,
    Resume,
}

#[cfg(unix)]
fn signal_child(child: &Child, signal: ChildSignal) -> Result<(), String> {
    let signal = match signal {
        ChildSignal::Pause => libc::SIGSTOP,
        ChildSignal::Resume => libc::SIGCONT,
    };
    let pid = child.id() as libc::pid_t;
    let result = unsafe { libc::kill(pid, signal) };
    if result == 0 {
        Ok(())
    } else {
        Err(format!(
            "Failed to signal local audio player: {}",
            std::io::Error::last_os_error()
        ))
    }
}

#[cfg(not(unix))]
fn signal_child(_child: &Child, _signal: ChildSignal) -> Result<(), String> {
    Err("Pause/resume for local audio is only supported on Unix-like systems.".to_string())
}

impl Drop for LocalAudioPlayer {
    fn drop(&mut self) {
        if let Some(mut active) = self.active.take() {
            let _ = active.child.kill();
            let _ = active.child.wait();
            cleanup_temp(active.temp_path.take());
        }
    }
}

fn start_playback(uri: &str) -> Result<StartedPlayback, String> {
    if let Some(command) = env::var("MUSICDCTL_AUDIO_PLAYER")
        .ok()
        .filter(|value| !value.trim().is_empty())
    {
        return spawn_configured_player(&command, uri);
    }
    if command_exists("ffplay") {
        return spawn_player(
            "ffplay",
            &["-nodisp", "-autoexit", "-loglevel", "quiet", uri],
            None,
        );
    }
    if command_exists("mpv") {
        return spawn_player("mpv", &["--no-video", "--really-quiet", uri], None);
    }
    if command_exists("vlc") {
        return spawn_player("vlc", &["-I", "dummy", "--play-and-exit", uri], None);
    }
    if command_exists("afplay") {
        return start_afplay(uri);
    }
    Err(
        "No local audio player found. Install ffplay, mpv, or vlc; on macOS afplay is used as a fallback."
            .to_string(),
    )
}

fn spawn_configured_player(command: &str, uri: &str) -> Result<StartedPlayback, String> {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return Err("MUSICDCTL_AUDIO_PLAYER is empty.".to_string());
    };
    let mut args = parts.collect::<Vec<_>>();
    args.push(uri);
    spawn_player(program, &args, None)
}

fn spawn_player(
    program: &str,
    args: &[&str],
    temp_path: Option<PathBuf>,
) -> Result<StartedPlayback, String> {
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("Failed to start {program}: {error}"))?;
    Ok(StartedPlayback { child, temp_path })
}

fn start_afplay(uri: &str) -> Result<StartedPlayback, String> {
    let response = reqwest::blocking::get(uri)
        .map_err(|error| format!("Failed to download track for afplay: {error}"))?
        .error_for_status()
        .map_err(|error| format!("Track download failed: {error}"))?;
    let bytes = response
        .bytes()
        .map_err(|error| format!("Failed to read downloaded track: {error}"))?;
    let temp_path = temp_audio_path(uri);
    fs::write(&temp_path, &bytes)
        .map_err(|error| format!("Failed to write {}: {error}", temp_path.display()))?;
    let temp_path_arg = temp_path.to_string_lossy().to_string();
    spawn_player("afplay", &[temp_path_arg.as_str()], Some(temp_path))
}

fn temp_audio_path(uri: &str) -> PathBuf {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let extension = uri
        .split('?')
        .next()
        .and_then(|path| path.rsplit('.').next())
        .filter(|part| part.len() <= 8 && part.chars().all(|c| c.is_ascii_alphanumeric()))
        .unwrap_or("audio");
    env::temp_dir().join(format!("musicdctl-{now}.{extension}"))
}

fn cleanup_temp(path: Option<PathBuf>) {
    if let Some(path) = path {
        let _ = fs::remove_file(path);
    }
}

fn command_exists(program: &str) -> bool {
    let program_path = Path::new(program);
    if program_path.components().count() > 1 {
        return program_path.is_file();
    }
    env::var_os("PATH")
        .into_iter()
        .flat_map(|paths| env::split_paths(&paths).collect::<Vec<_>>())
        .any(|path| executable_candidate(path, program).is_file())
}

fn executable_candidate(mut path: PathBuf, program: &str) -> PathBuf {
    path.push(OsString::from(program));
    path
}
