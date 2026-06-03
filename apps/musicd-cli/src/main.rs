mod api;
mod app;
mod config;
mod local_audio;

use std::io::{self, Stdout};
use std::panic;

use anyhow::{Context, Result, bail};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use musicd_upnp::{
    RendererDescription, UpnpService, fetch_service_actions, inspect_renderer,
    query_av_transport_action, query_playlist_extension_queue,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::api::ApiClient;
use crate::app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some((command_index, command)) = first_command(&args) {
        match command {
            "renderer-state" => {
                let config = config::load().unwrap_or_default();
                return run_renderer_state_command(&args[command_index + 1..], &config);
            }
            "help" => {
                print_help();
                return Ok(());
            }
            _ if command.starts_with('-') => {}
            _ => bail!("unknown command: {command}"),
        }
    }

    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let mut config = config::load().unwrap_or_default();
    let mut server_url = config.server_url();

    if let Some(server) = parse_flag(&args, "--server") {
        config.server_url = Some(server);
        config::save(&config)?;
        server_url = config.server_url();
    } else if let Ok(env) = std::env::var("MUSICD_URL") {
        if !env.trim().is_empty() {
            server_url = env;
        }
    }

    let had_client_id = config
        .client_id
        .as_ref()
        .is_some_and(|value| !value.trim().is_empty());
    let client_id = config.client_id();
    if !had_client_id {
        config::save(&config)?;
    }
    let api = ApiClient::new(&server_url, &client_id)?;

    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(api, config);
    let res = app.run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
}

fn first_command(args: &[String]) -> Option<(usize, &str)> {
    let mut index = 0;
    while index < args.len() {
        let arg = args[index].as_str();
        if arg == "--server" {
            index += 2;
            continue;
        }
        if arg.starts_with("--server=") {
            index += 1;
            continue;
        }
        return Some((index, arg));
    }
    None
}

fn parse_flag(args: &[String], name: &str) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == name {
            return iter.next().cloned();
        }
        if let Some(value) = arg.strip_prefix(&format!("{name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn run_renderer_state_command(args: &[String], config: &config::CliConfig) -> Result<()> {
    let mut location = None;
    let mut raw = false;
    let mut services = false;
    let mut queue = false;

    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                print_renderer_state_help();
                return Ok(());
            }
            "--raw" => raw = true,
            "--services" => services = true,
            "--queue" => queue = true,
            other if other.starts_with('-') => bail!("unknown renderer-state option: {other}"),
            other => {
                if location.replace(other.to_string()).is_some() {
                    bail!("renderer-state accepts at most one renderer URL");
                }
            }
        }
    }

    let location = location
        .or_else(|| {
            config
                .renderer_location
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
        })
        .context("renderer-state needs a renderer URL, or a renderer selected in the TUI")?;

    run_renderer_state(&location, raw, services, queue)
}

fn run_renderer_state(location: &str, raw: bool, services: bool, queue: bool) -> Result<()> {
    let (resolved_location, renderer) = inspect_renderer_with_base_fallback(location)?;
    println!("Renderer: {}", renderer.friendly_name);
    println!("Location: {}", resolved_location);
    println!("Device type: {}", renderer.device_type);
    if let Some(manufacturer) = &renderer.manufacturer {
        println!("Manufacturer: {manufacturer}");
    }
    if let Some(model_name) = &renderer.model_name {
        println!("Model: {model_name}");
    }
    println!(
        "AVTransport control URL: {}",
        renderer.av_transport_control_url
    );
    if let Some(rendering_control_url) = &renderer.rendering_control_url {
        println!("RenderingControl control URL: {rendering_control_url}");
    }
    if let Some(actions) = &renderer.capabilities.av_transport_actions {
        println!("AVTransport actions: {}", actions.join(", "));
    }
    if services {
        println!();
        print_renderer_services(&renderer.services);
    }
    if queue {
        println!();
        print_renderer_queue(&renderer);
    }
    println!();

    for action in [
        "GetTransportInfo",
        "GetPositionInfo",
        "GetMediaInfo",
        "GetDeviceCapabilities",
        "GetTransportSettings",
        "GetCurrentTransportActions",
    ] {
        println!("{action}:");
        match query_av_transport_action(&renderer.av_transport_control_url, action) {
            Ok(response) => {
                if response.values.is_empty() {
                    println!("  ok, but no known response fields were parsed");
                } else {
                    for (name, value) in response.values {
                        println!("  {name}: {}", compact_upnp_value(&value));
                    }
                }
                if raw {
                    println!("  raw: {}", compact_upnp_value(&response.raw_xml));
                }
            }
            Err(error) => println!("  error: {error}"),
        }
        println!();
    }

    Ok(())
}

fn print_renderer_queue(renderer: &RendererDescription) {
    let Some(service) = renderer
        .services
        .iter()
        .find(|service| service.service_type == "urn:UuVol-com:service:PlaylistExtension:1")
    else {
        println!("Renderer queue: PlaylistExtension service not advertised");
        return;
    };

    println!("Renderer queue via PlaylistExtension:");
    match query_playlist_extension_queue(&service.control_url) {
        Ok(playlist) => {
            println!(
                "  id_array_token: {}",
                playlist
                    .id_array_token
                    .map(|token| token.to_string())
                    .unwrap_or_else(|| "<none>".to_string())
            );
            println!("  ids: {}", join_u32s(&playlist.ids));
            println!("  entries: {}", playlist.entries.len());
            for (index, entry) in playlist.entries.iter().enumerate() {
                println!(
                    "  {:>3}. id={} title={} uri={}",
                    index + 1,
                    entry.id,
                    entry.title.as_deref().unwrap_or("<unknown>"),
                    entry.uri
                );
            }
            if playlist.entries.len() != playlist.ids.len() {
                println!(
                    "  note: renderer returned metadata for {} of {} queue id(s)",
                    playlist.entries.len(),
                    playlist.ids.len()
                );
            }
        }
        Err(error) => println!("  error: {error}"),
    }
}

fn join_u32s(values: &[u32]) -> String {
    if values.is_empty() {
        return "<none>".to_string();
    }
    values
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",")
}

fn print_renderer_services(services: &[UpnpService]) {
    if services.is_empty() {
        println!("Advertised services: none");
        return;
    }

    println!("Advertised services:");
    for service in services {
        println!("  {}", service.service_type);
        if let Some(service_id) = &service.service_id {
            println!("    serviceId: {service_id}");
        }
        println!("    controlURL: {}", service.control_url);
        if let Some(event_sub_url) = &service.event_sub_url {
            println!("    eventSubURL: {event_sub_url}");
        }
        if let Some(scpd_url) = &service.scpd_url {
            println!("    SCPDURL: {scpd_url}");
            match fetch_service_actions(scpd_url) {
                Ok(actions) if actions.is_empty() => {
                    println!("    actions: <none>");
                }
                Ok(actions) => {
                    println!("    actions: {}", actions.join(", "));
                    if service_or_actions_look_queue_related(service, &actions) {
                        println!("    queue candidate: yes");
                    }
                }
                Err(error) => println!("    actions error: {error}"),
            }
        }
    }
}

fn service_or_actions_look_queue_related(service: &UpnpService, actions: &[String]) -> bool {
    let service_text = format!(
        "{} {}",
        service.service_type,
        service.service_id.as_deref().unwrap_or_default()
    )
    .to_ascii_lowercase();
    service_text.contains("playlist")
        || service_text.contains("queue")
        || actions.iter().any(|action| {
            let action = action.to_ascii_lowercase();
            action.contains("playlist")
                || action.contains("queue")
                || action == "idarray"
                || action == "read"
                || action == "readlist"
        })
}

fn inspect_renderer_with_base_fallback(location: &str) -> Result<(String, RendererDescription)> {
    let trimmed = location.trim().trim_end_matches('/').to_string();
    match inspect_renderer(&trimmed) {
        Ok(renderer) => Ok((trimmed, renderer)),
        Err(first_error) => {
            if trimmed.ends_with("/description.xml") {
                return Err(first_error).context("inspecting renderer");
            }
            let candidate = format!("{trimmed}/description.xml");
            inspect_renderer(&candidate)
                .map(|renderer| (candidate.clone(), renderer))
                .map_err(|second_error| {
                    anyhow::anyhow!(
                        "failed to inspect renderer as {trimmed:?} ({first_error}) or {candidate:?} ({second_error})"
                    )
                })
        }
    }
}

fn compact_upnp_value(value: &str) -> String {
    let compact = value.split_whitespace().collect::<Vec<_>>().join(" ");
    const LIMIT: usize = 1600;
    if compact.len() > LIMIT {
        let end = compact
            .char_indices()
            .map(|(index, _)| index)
            .take_while(|index| *index <= LIMIT)
            .last()
            .unwrap_or(0);
        format!("{}... <{} chars>", &compact[..end], compact.len())
    } else {
        compact
    }
}

fn print_help() {
    println!(
        "musicdctl — TUI controller for the musicd service\n\n\
         USAGE:\n  \
           musicdctl [--server URL]\n  \
           musicdctl renderer-state [RENDERER_URL_OR_BASE_URL] [--raw] [--services] [--queue]\n\n\
         OPTIONS:\n  \
           --server URL   musicd HTTP base URL (default http://127.0.0.1:7878)\n  \
           --help, -h     show this help\n\n\
         COMMANDS:\n  \
           renderer-state  query live UPnP AVTransport state from a renderer\n\n\
         ENV:\n  \
           MUSICD_URL     overrides the default server URL\n\n\
         CONFIG:\n  \
           ~/.config/musicd/cli.toml — persists server URL and selected renderer"
    );
}

fn print_renderer_state_help() {
    println!(
        "musicdctl renderer-state — query live UPnP AVTransport state\n\n\
         USAGE:\n  \
           musicdctl renderer-state [RENDERER_URL_OR_BASE_URL] [--raw] [--services] [--queue]\n\n\
         OPTIONS:\n  \
           --raw       include compacted raw SOAP responses\n  \
           --services  fetch advertised UPnP services and action lists\n  \
           --queue     fetch CXN/UuVol PlaylistExtension queue entries\n  \
           --help, -h  show this help\n\n\
         If no renderer URL is provided, musicdctl uses the renderer selected in the TUI."
    );
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

fn install_panic_hook() {
    let original = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original(info);
    }));
}
