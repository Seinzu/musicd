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
    RendererDescription, UpnpService, fetch_service_action_descriptions, fetch_service_actions,
    inspect_renderer, query_av_transport_action, query_playlist_extension_queue,
    query_upnp_service_action, sm_search_service,
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
            "sm-search" => {
                let config = config::load().unwrap_or_default();
                return run_sm_search_command(&args[command_index + 1..], &config);
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

fn run_sm_search_command(args: &[String], config: &config::CliConfig) -> Result<()> {
    let mut location = None;
    let mut raw = false;
    let mut probe = false;
    let mut action = None;
    let mut search = None;
    let mut search_probe = None;
    let mut arg_pairs = Vec::new();
    let mut index = 0;

    while index < args.len() {
        let arg = &args[index];
        match arg.as_str() {
            "--help" | "-h" => {
                print_sm_search_help();
                return Ok(());
            }
            "--raw" => raw = true,
            "--probe" => probe = true,
            "--action" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--action needs an action name");
                };
                action = Some(value.clone());
            }
            "--search" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--search needs a name filter");
                };
                search = Some(value.clone());
            }
            "--search-probe" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--search-probe needs a name filter");
                };
                search_probe = Some(value.clone());
            }
            "--arg" => {
                index += 1;
                let Some(value) = args.get(index) else {
                    bail!("--arg needs NAME=VALUE");
                };
                arg_pairs.push(parse_name_value_arg(value)?);
            }
            other if other.starts_with("--action=") => {
                action = Some(other.trim_start_matches("--action=").to_string());
            }
            other if other.starts_with("--search=") => {
                search = Some(other.trim_start_matches("--search=").to_string());
            }
            other if other.starts_with("--search-probe=") => {
                search_probe = Some(other.trim_start_matches("--search-probe=").to_string());
            }
            other if other.starts_with("--arg=") => {
                arg_pairs.push(parse_name_value_arg(other.trim_start_matches("--arg="))?);
            }
            other if other.starts_with('-') => bail!("unknown sm-search option: {other}"),
            other => {
                if location.replace(other.to_string()).is_some() {
                    bail!("sm-search accepts at most one renderer URL");
                }
            }
        }
        index += 1;
    }

    let location = location
        .or_else(|| {
            config
                .renderer_location
                .as_ref()
                .filter(|value| !value.trim().is_empty())
                .cloned()
        })
        .context("sm-search needs a renderer URL, or a renderer selected in the TUI")?;

    if [search.is_some(), search_probe.is_some(), action.is_some()]
        .into_iter()
        .filter(|selected| *selected)
        .count()
        > 1
    {
        bail!("use only one of --search, --search-probe, or --action");
    }

    let action = action.or_else(|| search.as_ref().map(|_| "Search".to_string()));
    if let Some(search) = search {
        arg_pairs = default_sm_search_args(&search, arg_pairs);
    } else if matches!(action.as_deref(), Some("Search")) {
        arg_pairs = default_sm_search_args("", arg_pairs);
    }

    run_sm_search(
        &location,
        action.as_deref(),
        &arg_pairs,
        search_probe.as_deref(),
        raw,
        probe,
    )
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

fn run_sm_search(
    location: &str,
    action: Option<&str>,
    arg_pairs: &[(String, String)],
    search_probe: Option<&str>,
    raw: bool,
    probe: bool,
) -> Result<()> {
    let (resolved_location, renderer) = inspect_renderer_with_base_fallback(location)?;
    println!("Renderer: {}", renderer.friendly_name);
    println!("Location: {resolved_location}");

    let Some(service) = sm_search_service(&renderer) else {
        println!("SMSearch: service not advertised");
        return Ok(());
    };

    println!("SMSearch service: {}", service.service_type);
    if let Some(service_id) = &service.service_id {
        println!("serviceId: {service_id}");
    }
    println!("controlURL: {}", service.control_url);
    if let Some(event_sub_url) = &service.event_sub_url {
        println!("eventSubURL: {event_sub_url}");
    }
    if let Some(scpd_url) = &service.scpd_url {
        println!("SCPDURL: {scpd_url}");
        match fetch_service_action_descriptions(scpd_url) {
            Ok(actions) if actions.is_empty() => println!("actions: <none>"),
            Ok(actions) => {
                println!("actions:");
                for action in &actions {
                    println!("  {}", format_action_signature(&action));
                }
                if probe {
                    println!();
                    for action in actions.iter().filter(|action| {
                        action
                            .arguments
                            .iter()
                            .all(|arg| !matches!(arg.direction.as_deref(), Some("in")))
                    }) {
                        print_upnp_action_result(
                            &service.service_type,
                            &service.control_url,
                            &action.name,
                            &[],
                            raw,
                        );
                    }
                }
            }
            Err(error) => println!("actions error: {error}"),
        }
    } else {
        println!("SCPDURL: <none>");
    }

    if let Some(action) = action {
        println!();
        let args = arg_pairs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        print_upnp_action_result(
            &service.service_type,
            &service.control_url,
            action,
            &args,
            raw,
        );
    }

    if let Some(name_filter) = search_probe {
        println!();
        print_sm_search_probe(
            &service.service_type,
            &service.control_url,
            name_filter,
            raw,
        );
    }

    Ok(())
}

fn print_sm_search_probe(service_type: &str, control_url: &str, name_filter: &str, raw: bool) {
    println!("Search probe:");
    for (label, args) in sm_search_probe_arg_sets(name_filter) {
        println!("  candidate: {label}");
        println!("  args: {}", format_name_value_pairs(&args));
        let refs = args
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect::<Vec<_>>();
        print_upnp_action_result(service_type, control_url, "Search", &refs, raw);
    }
}

fn print_upnp_action_result(
    service_type: &str,
    control_url: &str,
    action: &str,
    args: &[(&str, &str)],
    raw: bool,
) {
    println!("{action}:");
    match query_upnp_service_action(service_type, control_url, action, args) {
        Ok(response) => {
            if response.values.is_empty() {
                println!("  ok, but no simple response fields were parsed");
            } else {
                for (name, value) in &response.values {
                    println!("  {name}: {}", compact_upnp_value(&value));
                    print_sm_search_xml_summary(name, value);
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

fn parse_name_value_arg(value: &str) -> Result<(String, String)> {
    let Some((name, value)) = value.split_once('=') else {
        bail!("--arg values must be NAME=VALUE");
    };
    let name = name.trim();
    if name.is_empty() {
        bail!("--arg name cannot be empty");
    }
    Ok((name.to_string(), value.to_string()))
}

fn default_sm_search_args(
    name_filter: &str,
    mut explicit: Vec<(String, String)>,
) -> Vec<(String, String)> {
    for (name, value) in [
        ("NameFilter", name_filter),
        ("CodecFilter", "0"),
        ("LocationIDFilter", "0"),
        ("GenreIDFilter", "0"),
        ("MinBitrateFilter", "0"),
    ] {
        if explicit.iter().any(|(candidate, _)| candidate == name) {
            continue;
        }
        explicit.push((name.to_string(), value.to_string()));
    }
    explicit
}

fn sm_search_probe_arg_sets(name_filter: &str) -> Vec<(&'static str, Vec<(String, String)>)> {
    [
        ("zero filters", ("0", "0", "0")),
        ("empty codec, zero ids", ("", "0", "0")),
    ]
    .into_iter()
    .map(|(label, (codec, location, genre))| {
        (
            label,
            vec![
                ("NameFilter".to_string(), name_filter.to_string()),
                ("CodecFilter".to_string(), codec.to_string()),
                ("LocationIDFilter".to_string(), location.to_string()),
                ("GenreIDFilter".to_string(), genre.to_string()),
                ("MinBitrateFilter".to_string(), "0".to_string()),
            ],
        )
    })
    .collect()
}

fn format_name_value_pairs(values: &[(String, String)]) -> String {
    values
        .iter()
        .map(|(name, value)| format!("{name}={value:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn print_sm_search_xml_summary(name: &str, value: &str) {
    match name {
        "ResultXML" => print_reciva_station_summary(value),
        "GenreXML" => print_reciva_list_summary("genres", value),
        "LocationXML" => print_reciva_list_summary("locations", value),
        "CodecList" => print_reciva_list_summary("codecs", value),
        _ => {}
    }
}

fn print_reciva_station_summary(xml: &str) {
    if let Some(count) = xml_opening_tag(xml, "stations")
        .and_then(|tag| xml_attribute(tag, "count"))
        .filter(|value| !value.is_empty())
    {
        println!("  stations: {count}");
    }

    let stations = xml_tag_blocks(xml, "station");
    for (index, station) in stations.iter().take(10).enumerate() {
        let id = xml_opening_tag(station, "station")
            .and_then(|tag| xml_attribute(tag, "id"))
            .or_else(|| xml_first_tag(station, "id"))
            .unwrap_or_else(|| "<none>".to_string());
        let title = xml_first_tag(station, "name")
            .or_else(|| xml_first_tag(station, "title"))
            .or_else(|| xml_first_tag(station, "stationName"))
            .unwrap_or_else(|| "<unknown>".to_string());
        let url = xml_first_tag(station, "url")
            .or_else(|| xml_first_tag(station, "streamUrl"))
            .or_else(|| xml_first_tag(station, "stream"))
            .unwrap_or_else(|| "<no-url>".to_string());
        println!("  {:>3}. id={} title={} url={}", index + 1, id, title, url);
    }
    if stations.len() > 10 {
        println!("  ... {} more station(s)", stations.len() - 10);
    }
}

fn print_reciva_list_summary(label: &str, xml: &str) {
    let entries = ["codec", "genre", "location", "item"]
        .into_iter()
        .flat_map(|tag| xml_tag_blocks(xml, tag))
        .collect::<Vec<_>>();
    if entries.is_empty() {
        return;
    }
    println!("  {label}: {}", entries.len());
    for (index, entry) in entries.iter().take(10).enumerate() {
        let id = xml_opening_tag(entry, "codec")
            .or_else(|| xml_opening_tag(entry, "genre"))
            .or_else(|| xml_opening_tag(entry, "location"))
            .or_else(|| xml_opening_tag(entry, "item"))
            .and_then(|tag| xml_attribute(tag, "id"))
            .or_else(|| xml_first_tag(entry, "id"))
            .unwrap_or_else(|| "<none>".to_string());
        let title = xml_first_tag(entry, "name")
            .or_else(|| xml_first_tag(entry, "title"))
            .unwrap_or_else(|| {
                entry
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join(" ")
                    .chars()
                    .take(80)
                    .collect()
            });
        println!("  {:>3}. id={} {}", index + 1, id, title);
    }
    if entries.len() > 10 {
        println!("  ... {} more {label}", entries.len() - 10);
    }
}

fn xml_opening_tag(xml: &str, tag: &str) -> Option<String> {
    let mut search_start = 0;
    loop {
        let start = xml[search_start..].find('<')? + search_start;
        let name_start = start + 1;
        let name_end = name_start + tag.len();
        if xml.get(name_start..name_end) != Some(tag) {
            search_start = name_start;
            continue;
        }
        if !xml
            .as_bytes()
            .get(name_end)
            .is_some_and(|byte| byte.is_ascii_whitespace() || *byte == b'>' || *byte == b'/')
        {
            search_start = name_start;
            continue;
        }
        let end = xml[name_end..].find('>').map(|offset| name_end + offset)?;
        return Some(xml[start..=end].to_string());
    }
}

fn xml_attribute(opening_tag: String, attribute: &str) -> Option<String> {
    let pattern = format!("{attribute}=\"");
    let start = opening_tag.find(&pattern)? + pattern.len();
    let end = opening_tag[start..].find('"')? + start;
    Some(opening_tag[start..end].to_string())
}

fn xml_tag_blocks(xml: &str, tag: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut search_start = 0;
    while let Some(open_tag) = xml_opening_tag(&xml[search_start..], tag) {
        let relative_open = match xml[search_start..].find(&open_tag) {
            Some(index) => index,
            None => break,
        };
        let open_start = search_start + relative_open;
        let content_start = open_start + open_tag.len();
        let close_tag = format!("</{tag}>");
        let Some(close_start) = xml[content_start..]
            .find(&close_tag)
            .map(|index| content_start + index)
        else {
            search_start = content_start;
            continue;
        };
        blocks.push(xml[open_start..close_start + close_tag.len()].to_string());
        search_start = close_start + close_tag.len();
    }
    blocks
}

fn xml_first_tag(xml: &str, tag: &str) -> Option<String> {
    let open_tag = xml_opening_tag(xml, tag)?;
    let start = xml.find(&open_tag)? + open_tag.len();
    let close_tag = format!("</{tag}>");
    let end = xml[start..].find(&close_tag)? + start;
    Some(xml[start..end].trim().to_string())
}

fn format_action_signature(action: &musicd_upnp::UpnpActionDescription) -> String {
    if action.arguments.is_empty() {
        return format!("{}()", action.name);
    }
    let args = action
        .arguments
        .iter()
        .map(|arg| {
            let direction = arg.direction.as_deref().unwrap_or("?");
            let related = arg.related_state_variable.as_deref().unwrap_or("unknown");
            format!("{direction} {}: {related}", arg.name)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{}({args})", action.name)
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
           musicdctl renderer-state [RENDERER_URL_OR_BASE_URL] [--raw] [--services] [--queue]\n  \
           musicdctl sm-search [RENDERER_URL_OR_BASE_URL] [--probe] [--search NAME] [--search-probe NAME] [--action ACTION] [--arg NAME=VALUE] [--raw]\n\n\
         OPTIONS:\n  \
           --server URL   musicd HTTP base URL (default http://127.0.0.1:7878)\n  \
           --help, -h     show this help\n\n\
         COMMANDS:\n  \
           renderer-state  query live UPnP AVTransport state from a renderer\n  \
           sm-search       inspect and call a CXN/StreamMagic SMSearch service\n\n\
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

fn print_sm_search_help() {
    println!(
        "musicdctl sm-search — inspect a CXN/StreamMagic SMSearch service\n\n\
         USAGE:\n  \
           musicdctl sm-search [RENDERER_URL_OR_BASE_URL] [--probe] [--search NAME] [--search-probe NAME] [--action ACTION] [--arg NAME=VALUE] [--raw]\n\n\
         OPTIONS:\n  \
           --probe          invoke advertised SMSearch actions that do not require input args\n  \
           --search NAME    invoke Search with default zero filters and this NameFilter\n  \
           --search-probe NAME try likely Search filter defaults and print each result\n  \
           --action ACTION  invoke one advertised SMSearch action\n  \
           --arg NAME=VALUE pass one SOAP argument; repeat for multiple arguments\n  \
           --raw            include the compacted raw SOAP response when invoking an action\n  \
           --help, -h       show this help\n\n\
         Search defaults CodecFilter, LocationIDFilter, GenreIDFilter, and MinBitrateFilter to 0.\n  \
         With no --probe or --action, this prints the advertised SMSearch control URL and action signatures.\n  \
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

#[cfg(test)]
mod tests {
    use super::{default_sm_search_args, sm_search_probe_arg_sets, xml_first_tag, xml_tag_blocks};

    #[test]
    fn sm_search_defaults_supply_required_filters_without_overwriting_explicit_args() {
        let args = default_sm_search_args(
            "radio",
            vec![
                ("GenreIDFilter".to_string(), "12".to_string()),
                ("MinBitrateFilter".to_string(), "128".to_string()),
            ],
        );

        assert_eq!(
            args,
            vec![
                ("GenreIDFilter".to_string(), "12".to_string()),
                ("MinBitrateFilter".to_string(), "128".to_string()),
                ("NameFilter".to_string(), "radio".to_string()),
                ("CodecFilter".to_string(), "0".to_string()),
                ("LocationIDFilter".to_string(), "0".to_string()),
            ]
        );
    }

    #[test]
    fn sm_search_probe_includes_confirmed_filter_shapes() {
        let arg_sets = sm_search_probe_arg_sets("radio");

        assert_eq!(arg_sets[0].0, "zero filters");
        assert_eq!(
            arg_sets[0].1,
            vec![
                ("NameFilter".to_string(), "radio".to_string()),
                ("CodecFilter".to_string(), "0".to_string()),
                ("LocationIDFilter".to_string(), "0".to_string()),
                ("GenreIDFilter".to_string(), "0".to_string()),
                ("MinBitrateFilter".to_string(), "0".to_string()),
            ]
        );
        assert_eq!(arg_sets[1].0, "empty codec, zero ids");
    }

    #[test]
    fn xml_helpers_extract_reciva_station_blocks_with_attributes() {
        let xml = r#"<reciva><stations count="1"><station id="abc"><name>BBC Radio 6 Music</name><url>http://example.test/stream</url></station></stations></reciva>"#;
        let stations = xml_tag_blocks(xml, "station");

        assert_eq!(stations.len(), 1);
        assert_eq!(
            xml_first_tag(&stations[0], "name").as_deref(),
            Some("BBC Radio 6 Music")
        );
        assert_eq!(
            xml_first_tag(&stations[0], "url").as_deref(),
            Some("http://example.test/stream")
        );
    }
}
