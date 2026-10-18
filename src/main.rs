mod commands;
mod config;
mod gemini;
mod platform;
mod screen;
mod serve;
mod server;
mod setup;
mod store;
mod tools;
mod ui;

use clap::{Parser, Subcommand};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(
    name = "parrot",
    version,
    about = "Live meeting transcription and a voice assistant for your notes, in your browser.",
    after_help = "Run `parrot` to set up (first time) and open the app.\nDocs: https://github.com/PrajwalShete/parrot"
)]
struct Cli {
    /// Don't open the browser
    #[arg(long, global = true, env = "PARROT_NO_OPEN", value_parser = clap::builder::FalseyValueParser::new())]
    no_open: bool,
    /// Port for the local app (default 4747, or the next free one)
    #[arg(long, global = true)]
    port: Option<u16>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Set up (or redo setup): notebook folder and Gemini API key
    Setup {
        /// Use this API key instead of asking
        #[arg(long, env = "GEMINI_API_KEY", hide_env_values = true)]
        key: Option<String>,
    },
    /// Open the app in the browser (starting it if needed)
    Open,
    /// Change the Gemini API key
    Key {
        /// The new key (asks if omitted)
        key: Option<String>,
    },
    /// Show the setup, the notebook and whether the app is running
    Status,
    /// Check the setup and explain how to fix problems
    Doctor,
    /// Download and install the latest version
    Update,
    /// Remove Parrot (your notes stay where they are)
    Uninstall {
        #[arg(short, long)]
        yes: bool,
    },
}

fn main() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    platform::guard_terminal();
    let cli = Cli::parse();
    let result = match cli.command {
        None | Some(Command::Open) => commands::start(cli.port, !cli.no_open),
        Some(Command::Setup { key }) => setup::run(key).and_then(|cfg| {
            let cfg = commands::with_port(cfg, cli.port);
            serve::run(cfg, !cli.no_open)
        }),
        Some(Command::Key { key }) => commands::key(key),
        Some(Command::Status) => commands::status(),
        Some(Command::Doctor) => commands::doctor(),
        Some(Command::Update) => commands::update(),
        Some(Command::Uninstall { yes }) => commands::uninstall(yes),
    };
    if let Err(e) = result {
        ui::fail(format!("{e:#}"));
        std::process::exit(1);
    }
}
