//! First-run setup: create the notebook folder, store a Gemini API key, hand over to the browser.

use crate::config::{self, Config};
use crate::gemini::{self, KeyCheck};
use crate::screen::Screen;
use crate::store::Store;
use crate::ui::{self, bold, cyan, dim};
use anyhow::{Result, bail};
use std::io::IsTerminal;
use std::time::Duration;

const TOTAL: usize = 3;

/// Runs setup. Returns the saved config. `key` (from `--key` or the environment) skips the prompt.
pub fn run(key: Option<String>) -> Result<Config> {
    let interactive = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
    let mut cfg = Config::load_or_default()?;
    let mut screen = Screen::start(interactive, "Setup");

    // The opening: let the mark breathe once before asking anything.
    screen.set_status(vec![format!("{} Getting things ready…", ui::spin())]);
    screen.pause(Duration::from_millis(1400));
    screen.set_status(vec![dim("Three quick steps.")]);

    // ── 1. Notebook ──
    screen.step(1, TOTAL, "Your notebook");
    let dir = cfg.data_dir.clone();
    let created = screen.work(&format!("Creating {}…", ui::tilde(&dir)), move || -> Result<bool> {
        std::thread::sleep(Duration::from_millis(500));
        let fresh = !dir.exists() || std::fs::read_dir(&dir).map(|mut d| d.next().is_none()).unwrap_or(true);
        let store = Store::open(&dir)?;
        if fresh {
            seed(&store)?;
        }
        Ok(fresh)
    })?;
    screen.ok(format!("Notebook at {}", bold(ui::tilde(&cfg.data_dir))));
    screen.info(dim(if created {
        "Topics are folders, notes are Markdown files. A welcome note is waiting inside."
    } else {
        "Found your existing notes; nothing was changed."
    }));

    // ── 2. API key ──
    screen.step(2, TOTAL, "Gemini API key");
    let key = match key {
        Some(k) => Some(k),
        None if cfg.is_ready() && interactive => {
            if screen.confirm(&format!("Keep your current key ({})?", cfg.masked_key()), true) {
                cfg.api_key.clone()
            } else {
                None
            }
        }
        None if cfg.is_ready() => cfg.api_key.clone(),
        None => None,
    };
    let key = match key {
        Some(k) => verify(&mut screen, k, false)?,
        None => {
            if !interactive {
                bail!("no API key: run `parrot setup --key YOUR_KEY` or set GEMINI_API_KEY");
            }
            ask_key(&mut screen)?
        }
    };
    cfg.api_key = Some(key);
    cfg.save()?;
    screen.ok(format!(
        "Saved to {} {}",
        bold(ui::tilde(&config::path())),
        dim("· only you can read it")
    ));

    // ── 3. Open ──
    screen.step(3, TOTAL, "Open Parrot");
    screen.ok("All set. Parrot opens in your browser; record meetings and talk to your notes there.");
    screen.info(dim("Run `parrot` any time to open it again."));
    screen.pause(Duration::from_millis(900));
    screen.finish();
    Ok(cfg)
}

/// Shows where to get a key, opens AI Studio on Enter, then asks until a key works.
pub fn ask_key(screen: &mut Screen) -> Result<String> {
    screen.card(
        "Get a free key from Google AI Studio",
        &[
            format!("{}  {}", dim("1"), cyan(gemini::AI_STUDIO_KEYS)),
            format!("{}  Click {} and copy it", dim("2"), bold("Create API key")),
            format!("{}  Paste it below", dim("3")),
        ],
    );
    screen.blank();
    if screen.press_enter(&format!(
        "{} to open AI Studio in your browser {}",
        bold("Enter"),
        dim("· esc to skip")
    )) {
        if crate::platform::open_url(gemini::AI_STUDIO_KEYS) {
            screen.ok("Opened AI Studio");
        } else {
            screen.info(format!("Open {}", cyan(gemini::AI_STUDIO_KEYS)));
        }
    }
    loop {
        let Some(key) = screen.input(
            "Paste your API key",
            "Paste with ⌘V / Ctrl+Shift+V · enter to check · esc to cancel",
            true,
        ) else {
            screen.cancel();
        };
        match verify(screen, key, true) {
            Ok(k) => return Ok(k),
            Err(_) => continue,
        }
    }
}

/// Checks a key with Google. Offline is fine (saved, checked again later); a rejected key is not.
fn verify(screen: &mut Screen, key: String, retrying: bool) -> Result<String> {
    let probe = key.clone();
    match screen.work("Checking your key with Google…", move || gemini::check_key(&probe)) {
        KeyCheck::Valid => {
            screen.ok(format!("Key works {}", dim(format!("({})", config::mask(&key)))));
            Ok(key)
        }
        KeyCheck::Unknown(why) => {
            screen.warn(format!("Couldn't check the key right now. {why} Saved anyway."));
            Ok(key)
        }
        KeyCheck::Invalid(why) => {
            screen.warn(format!("That key didn't work: {why}"));
            if retrying {
                screen.blank();
            }
            bail!("invalid key: {why}")
        }
    }
}

/// The first note in a new notebook: a short guide.
fn seed(store: &Store) -> Result<()> {
    store.create_note(
        "",
        "Welcome to Parrot",
        "# Welcome to Parrot\n\n\
         Parrot listens to your meetings and writes down what was said, in Marathi, Hindi, English or a mix.\n\n\
         ## How it's organized\n\n\
         - **Topics** are folders. Put subtopics inside them, like *Sales → Pune*.\n\
         - **Notes** are Markdown files. Recordings are notes too; every line starts with the time it was said.\n\
         - Everything lives in this folder, so you can open it in Finder or any editor.\n\n\
         ## Recording a meeting\n\n\
         1. Pick a topic in the sidebar and press **Record**.\n\
         2. Put the laptop (or a conference mic) in the middle of the table.\n\
         3. Press **Stop** when you're done. The transcript is already saved.\n\n\
         ## Asking about your meetings\n\n\
         Open the assistant on the right and press **Start talking**. Try:\n\n\
         - “What did we decide about the Pune stock today?”\n\
         - “Add a summary with action items to this meeting.”\n\
         - “Find every meeting where the IoT cost came up.”\n",
    )?;
    Ok(())
}
