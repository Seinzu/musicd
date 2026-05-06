mod api;
mod app;
mod config;

use std::io::{self, Stdout};
use std::panic;

use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

use crate::api::ApiClient;
use crate::app::App;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return Ok(());
    }

    let mut config = config::load().unwrap_or_default();

    if let Some(server) = parse_flag(&args, "--server") {
        config.server_url = Some(server);
        config::save(&config)?;
    } else if let Ok(env) = std::env::var("MUSICD_URL") {
        if !env.trim().is_empty() {
            config.server_url = Some(env);
        }
    }

    let api = ApiClient::new(&config.server_url())?;

    install_panic_hook();
    let mut terminal = setup_terminal()?;
    let mut app = App::new(api, config);
    let res = app.run(&mut terminal);
    restore_terminal(&mut terminal)?;
    res
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

fn print_help() {
    println!(
        "musicdctl — TUI controller for the musicd service\n\n\
         USAGE:\n  \
           musicdctl [--server URL]\n\n\
         OPTIONS:\n  \
           --server URL   musicd HTTP base URL (default http://127.0.0.1:7878)\n  \
           --help, -h     show this help\n\n\
         ENV:\n  \
           MUSICD_URL     overrides the default server URL\n\n\
         CONFIG:\n  \
           ~/.config/musicd/cli.toml — persists server URL and selected renderer"
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
