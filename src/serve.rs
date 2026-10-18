//! `parrot`: start the local app (or find the one already running) and open it in the browser,
//! with a live dashboard in the terminal while it runs.

use crate::config::Config;
use crate::screen::Screen;
use crate::server::{self, Activity, AppState};
use crate::ui::{self, accent, bold, dim};
use anyhow::Result;
use console::Key;
use std::io::IsTerminal;
use std::time::Duration;
use tokio::sync::broadcast;

const FEED: usize = 200;

pub fn run(cfg: Config, open_browser: bool) -> Result<()> {
    if server::running_on(cfg.port) {
        let url = format!("http://localhost:{}", cfg.port);
        if open_browser {
            crate::platform::open_url(&url);
        }
        ui::ok(format!("Parrot is already running at {}", accent(&url)));
        return Ok(());
    }

    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    let (listener, port) = rt.block_on(server::bind(cfg.port))?;
    let state = AppState::new(cfg.clone(), port)?;
    let url = format!("http://localhost:{port}");
    let activity = state.activity.subscribe();
    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();
    let server_task = rt.spawn(server::serve(listener, state.clone(), async {
        let _ = stop_rx.await;
    }));

    if open_browser && !crate::platform::open_url(&url) {
        ui::warn(format!("Couldn't open a browser. Open {url} yourself."));
    }

    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        dashboard(&url, &cfg, activity, open_browser);
    } else {
        plain(&rt, &url, &cfg, activity);
    }

    let _ = stop_tx.send(());
    rt.block_on(async {
        let _ = tokio::time::timeout(Duration::from_secs(2), server_task).await;
    });
    rt.shutdown_timeout(Duration::from_millis(500));
    ui::ok("Parrot stopped. Your notes are saved.");
    Ok(())
}

fn stamp(line: &str) -> String {
    format!("  {}  {line}", dim(chrono::Local::now().format("%H:%M:%S")))
}

/// The live terminal: the animated mark, where things are, and what's happening.
fn dashboard(url: &str, cfg: &Config, mut activity: broadcast::Receiver<Activity>, opened: bool) {
    let mut screen = Screen::start(true, "Running");
    screen.quit_on_ctrl_c();
    let first = if opened {
        format!("Opened {} in your browser", accent(url))
    } else {
        format!("Started at {}", accent(url))
    };
    let mut feed = vec![stamp(&first)];
    loop {
        while let Ok(Activity(line)) = activity.try_recv() {
            feed.push(stamp(&line));
        }
        if feed.len() > FEED {
            feed.drain(..feed.len() - FEED);
        }
        let pulse = if ((ui::clock() * 1.2) as u64).is_multiple_of(2) {
            accent("●")
        } else {
            dim("●")
        };
        screen.set_status(vec![
            format!("{pulse} {}  {}", bold("Live"), accent(url)),
            format!("{}  {}", dim("Notes"), ui::tilde(&cfg.data_dir)),
        ]);
        let mut section = vec![format!("  {}", dim("Activity")), String::new()];
        section.extend(feed.iter().cloned());
        screen.set_section(section);
        let help = |_: usize| vec![String::new(), format!("  {}", dim("o open in browser · q quit"))];
        match screen.poll(2, &help, Some(Duration::from_millis(120))) {
            Some(Key::Char('o' | 'O')) => {
                crate::platform::open_url(url);
                feed.push(stamp("Opened the browser"));
            }
            Some(Key::Char('q' | 'Q') | Key::Escape | Key::CtrlC) => break,
            _ => {}
        }
    }
    screen.set_section(Vec::new());
    screen.finish();
}

/// Without a terminal (a service, a pipe): plain lines until Ctrl+C or SIGTERM.
fn plain(rt: &tokio::runtime::Runtime, url: &str, cfg: &Config, mut activity: broadcast::Receiver<Activity>) {
    println!("  Parrot is running at {url}");
    println!("  Notes in {}", ui::tilde(&cfg.data_dir));
    rt.block_on(async {
        loop {
            tokio::select! {
                msg = activity.recv() => match msg {
                    Ok(Activity(line)) => println!("{}", stamp(&line)),
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                },
                _ = tokio::signal::ctrl_c() => break,
            }
        }
    });
}
