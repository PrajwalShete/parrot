//! The smaller commands: start, key, status, doctor, update, uninstall.

use crate::config::{self, Config};
use crate::gemini::{self, KeyCheck};
use crate::screen::Screen;
use crate::store::{Kind, Node, Store};
use crate::ui::{self, accent, bold, dim, green};
use crate::{serve, server, setup};
use anyhow::{Result, bail};
use serde_json::json;
use std::io::IsTerminal;

pub const INSTALL_URL: &str = "https://raw.githubusercontent.com/PrajwalShete/parrot/main/install.sh";

pub fn with_port(mut cfg: Config, port: Option<u16>) -> Config {
    if let Some(p) = port {
        cfg.port = p;
    }
    cfg
}

/// `parrot`: set up on first run, then start the app and open it.
pub fn start(port: Option<u16>, open: bool) -> Result<()> {
    let cfg = Config::load_or_default()?;
    let cfg = if cfg.is_ready() {
        cfg
    } else {
        setup::run(std::env::var("GEMINI_API_KEY").ok().filter(|k| !k.is_empty()))?
    };
    serve::run(with_port(cfg, port), open)
}

/// `parrot key [KEY]`
pub fn key(key: Option<String>) -> Result<()> {
    let mut cfg = Config::load_or_default()?;
    let key = match key {
        Some(k) => k,
        None => {
            if !std::io::stdin().is_terminal() {
                bail!("pass the key: parrot key YOUR_KEY");
            }
            let mut screen = Screen::start(true, "API key");
            let k = setup::ask_key(&mut screen)?;
            screen.finish();
            cfg.api_key = Some(k);
            cfg.save()?;
            ui::ok(format!(
                "Saved. A running app picks it up the next time you start a recording {}",
                dim("(or restart parrot)")
            ));
            return Ok(());
        }
    };
    let probe = key.clone();
    match ui::with_spinner("Checking your key with Google…", move || gemini::check_key(&probe)) {
        KeyCheck::Invalid(why) => bail!("that key didn't work: {why}"),
        KeyCheck::Unknown(why) => ui::warn(format!("Couldn't check the key ({why}); saved anyway.")),
        KeyCheck::Valid => ui::ok("Key works"),
    }
    cfg.api_key = Some(key.trim().to_string());
    cfg.save()?;
    ui::ok(format!("Saved to {}", ui::tilde(&config::path())));
    Ok(())
}

fn count(nodes: &[Node], topics: &mut usize, notes: &mut usize) {
    for n in nodes {
        match n.kind {
            Kind::Topic => {
                *topics += 1;
                count(n.children.as_deref().unwrap_or_default(), topics, notes);
            }
            Kind::Note => *notes += 1,
        }
    }
}

/// `parrot status`
pub fn status() -> Result<()> {
    let Some(cfg) = Config::load()? else {
        ui::warn("Not set up yet. Run `parrot` to set up.");
        return Ok(());
    };
    let (mut topics, mut notes) = (0, 0);
    if let Ok(store) = Store::open(&cfg.data_dir) {
        count(&store.tree().unwrap_or_default(), &mut topics, &mut notes);
    }
    let running = server::running_on(cfg.port);
    let url = format!("http://localhost:{}", cfg.port);
    ui::blank();
    ui::card(
        &format!("parrot v{}", crate::VERSION),
        &[
            format!(
                "{}     {}",
                dim("App"),
                if running {
                    format!("{} {}", green("● running"), accent(&url))
                } else {
                    dim("○ not running · run `parrot`")
                }
            ),
            format!(
                "{}   {} {}",
                dim("Notes"),
                ui::tilde(&cfg.data_dir),
                dim(format!("· {topics} topics, {notes} notes"))
            ),
            format!(
                "{}     {}",
                dim("Key"),
                if cfg.is_ready() {
                    cfg.masked_key()
                } else {
                    "missing · run `parrot key`".into()
                }
            ),
            format!("{}  {}", dim("Config"), ui::tilde(&config::path())),
            format!("{}  {} · {}", dim("Models"), gemini::TRANSCRIBE_MODEL, gemini::VOICE_MODEL),
        ],
    );
    ui::blank();
    Ok(())
}

/// `parrot doctor`: checks each piece, says how to fix what's broken.
pub fn doctor() -> Result<()> {
    ui::blank();
    ui::info(bold("Checking Parrot…"));
    ui::blank();
    let mut problems = 0;
    let Some(cfg) = Config::load()? else {
        ui::fail("Not set up. Run `parrot`.");
        return Ok(());
    };
    ui::ok(format!("Config at {}", ui::tilde(&config::path())));

    match Store::open(&cfg.data_dir).and_then(|s| {
        let probe = s.root().join(".parrot-doctor");
        std::fs::write(&probe, "ok")?;
        std::fs::remove_file(&probe)?;
        Ok(())
    }) {
        Ok(()) => ui::ok(format!("Notebook folder is writable ({})", ui::tilde(&cfg.data_dir))),
        Err(e) => {
            problems += 1;
            ui::fail(format!("Notebook folder: {e:#}"));
        }
    }

    let Some(key) = cfg.api_key.clone().filter(|k| !k.is_empty()) else {
        ui::fail("No API key. Run `parrot key`.");
        return Ok(());
    };
    let probe = key.clone();
    match ui::with_spinner("Checking the key…", move || gemini::check_key(&probe)) {
        KeyCheck::Valid => ui::ok("API key works"),
        KeyCheck::Invalid(why) => {
            problems += 1;
            ui::fail(format!("API key rejected: {why} → run `parrot key`"));
        }
        KeyCheck::Unknown(why) => {
            problems += 1;
            ui::warn(format!("Couldn't reach Google: {why}"));
        }
    }

    for (label, model, modality) in [
        ("Live transcription", gemini::TRANSCRIBE_MODEL, "TEXT"),
        ("Voice assistant", gemini::VOICE_MODEL, "AUDIO"),
    ] {
        let setup = json!({ "setup": { "model": format!("models/{model}"), "generationConfig": { "responseModalities": [modality] } } });
        let key = key.clone();
        let result = ui::with_spinner(&format!("Connecting to {model}…"), move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(gemini::connect(&key, &setup))
                .map(|_| ())
        });
        match result {
            Ok(()) => ui::ok(format!("{label} ({model}) is reachable")),
            Err(e) => {
                problems += 1;
                ui::fail(format!("{label} ({model}): {e:#}"));
            }
        }
    }

    if server::running_on(cfg.port) {
        ui::ok(format!("App is running at http://localhost:{}", cfg.port));
    } else {
        ui::info(format!("{} App isn't running {}", dim("○"), dim("· start it with `parrot`")));
    }
    ui::blank();
    if problems == 0 {
        ui::ok(bold("Everything looks good."));
    } else {
        ui::warn(format!("{problems} problem(s) found."));
    }
    ui::blank();
    Ok(())
}

/// `parrot update`: re-runs the installer for the latest release.
pub fn update() -> Result<()> {
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("curl -fsSL {INSTALL_URL} | sh -s -- --no-setup"))
        .status()?;
    if !status.success() {
        bail!("the update didn't finish; try again or run the install command from the README");
    }
    Ok(())
}

/// `parrot uninstall`: removes the program and its settings; the notes stay.
pub fn uninstall(yes: bool) -> Result<()> {
    let cfg = Config::load_or_default()?;
    let confirmed = yes
        || (std::io::stdin().is_terminal() && crate::screen::plain_confirm("Remove Parrot from this computer? Your notes stay.", false));
    if !confirmed {
        return Ok(());
    }
    let home = config::home();
    if home.exists() {
        std::fs::remove_dir_all(&home)?;
        ui::ok(format!("Removed {}", ui::tilde(&home)));
    }
    if let Some(link) = dirs::home_dir().map(|h| h.join(".local/bin/parrot"))
        && link.symlink_metadata().is_ok()
    {
        std::fs::remove_file(&link)?;
        ui::ok(format!("Removed {}", ui::tilde(&link)));
    }
    if let Ok(exe) = std::env::current_exe()
        && exe.exists()
        && std::fs::remove_file(&exe).is_ok()
    {
        ui::ok(format!("Removed {}", ui::tilde(&exe)));
    }
    ui::info(format!("Your notes are still in {}", bold(ui::tilde(&cfg.data_dir))));
    Ok(())
}
