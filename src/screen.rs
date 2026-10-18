//! A full-screen, animated terminal screen for setup and the running dashboard.
//!
//! Live mode uses the terminal's alternate screen: the mark animates at the top, the current
//! step sits under it, and each frame rewrites only the rows that changed. Everything settled is
//! kept as a transcript and printed to the normal screen when the screen closes, so the
//! scrollback reads like a plain run. Without a terminal the same calls print plain lines.

use crate::platform::{self, RawInput};
use crate::ui::{self, accent, bold, dim, green};
use console::{Key, Term, measure_text_width, style, truncate_str};
use std::time::{Duration, Instant};

const MARKS: [(usize, usize); 3] = [(ui::MARK_W, ui::MARK_H), (28, 12), (22, 9)];
const FRAME: Duration = Duration::from_millis(66);
/// Marks a step header in the section, drawn to the window's width when shown.
const STEP: char = '\u{0}';

pub struct Screen {
    live: Option<Live>,
    subtitle: String,
    status: Vec<String>,
    section: Vec<String>,
    transcript: Vec<String>,
    /// Ctrl+C returns `Key::CtrlC` instead of cancelling (the dashboard shuts down cleanly).
    ctrl_c_is_key: bool,
}

struct Live {
    term: Term,
    _raw: Option<RawInput>,
    shown: Vec<String>,
    size: (u16, u16),
}

impl Screen {
    pub fn start(live: bool, subtitle: &str) -> Self {
        let mut screen = Self {
            live: None,
            subtitle: subtitle.into(),
            status: Vec::new(),
            section: Vec::new(),
            transcript: Vec::new(),
            ctrl_c_is_key: false,
        };
        if live {
            let term = Term::stdout();
            let raw = RawInput::enable();
            let _ = term.write_str("\x1b[?1049h\x1b[?25l\x1b[2J");
            screen.live = Some(Live {
                term,
                _raw: raw,
                shown: Vec::new(),
                size: (0, 0),
            });
        } else {
            ui::print_hero(&ui::brand_lines(subtitle));
        }
        screen
    }

    pub fn quit_on_ctrl_c(&mut self) {
        self.ctrl_c_is_key = true;
    }

    pub fn is_live(&self) -> bool {
        self.live.is_some()
    }

    /// Lines under the brand, beside the mark.
    pub fn set_status(&mut self, lines: Vec<String>) {
        self.status = lines;
    }

    fn hero_right(&self, short: bool) -> Vec<String> {
        let mut right = ui::brand_lines(if short { "" } else { &self.subtitle });
        if !self.status.is_empty() {
            right.push(String::new());
            right.extend(self.status.iter().cloned());
        }
        right
    }

    // ---- Settled lines ----------------------------------------------------------------------

    pub fn line(&mut self, line: String) {
        if self.is_live() {
            if line.is_empty() && self.transcript.last().is_some_and(String::is_empty) {
                return;
            }
            self.section.push(line.clone());
            self.transcript.push(line);
        } else {
            println!("{line}");
        }
    }

    pub fn ok(&mut self, msg: impl std::fmt::Display) {
        self.line(format!("  {} {msg}", style("✓").green()));
    }

    pub fn warn(&mut self, msg: impl std::fmt::Display) {
        self.line(format!("  {} {msg}", style("!").yellow().bold()));
    }

    pub fn info(&mut self, msg: impl std::fmt::Display) {
        self.line(format!("  {msg}"));
    }

    pub fn blank(&mut self) {
        self.line(String::new());
    }

    pub fn card(&mut self, title: &str, lines: &[String]) {
        for l in ui::card_lines(title, lines) {
            self.line(l);
        }
    }

    pub fn step(&mut self, n: usize, total: usize, title: &str) {
        if self.is_live() {
            let rule = format!("{STEP}{n}{STEP}{total}{STEP}{title}");
            self.section = vec![rule.clone(), String::new()];
            self.transcript.extend([String::new(), rule, String::new()]);
        } else {
            println!();
            println!("{}", ui::step_line(n, total, title));
            println!();
        }
    }

    /// Replaces the section (the dashboard's activity feed).
    pub fn set_section(&mut self, lines: Vec<String>) {
        self.section = lines;
    }

    // ---- Drawing ------------------------------------------------------------------------------

    /// Draws one frame: the hero, the section, then `widget(rows)` (at most `rows` lines, needing
    /// `min`). The mark shrinks, then becomes a one-line brand, when the window is short.
    pub fn draw(&mut self, min: usize, widget: &dyn Fn(usize) -> Vec<String>) {
        let Some(live) = self.live.as_ref() else { return };
        let (rows, cols) = live.term.size_checked().unwrap_or((24, 80));
        let (rows, cols) = (rows as usize, cols as usize);
        let section: Vec<String> = self.section.iter().map(|l| expand_step(l)).collect();
        let (full, short) = (self.hero_right(false), self.hero_right(true));
        let mut frame = Vec::with_capacity(rows);
        let hero = MARKS.iter().find_map(|&(mw, mh)| {
            let right = if full.len() <= mh { &full } else { &short };
            let right_w = right.iter().map(|l| measure_text_width(l)).max().unwrap_or(0);
            let height = mh.max(right.len()) + 2;
            (cols >= mw + 6 + right_w && height + section.len().min(4) + min <= rows).then_some((mw, mh, right))
        });
        match hero {
            Some((mw, mh, right)) => frame.extend(ui::hero_beside(right, &ui::mark_sized(ui::clock(), mw, mh), mw)),
            None => {
                frame.push(String::new());
                frame.push(format!(
                    "  {}  {}",
                    style("parrot").color256(ui::ACCENT).bold(),
                    dim(format!("{} · v{}", self.subtitle, crate::VERSION))
                ));
                frame.extend(self.status.iter().map(|s| format!("  {s}")));
                frame.push(String::new());
            }
        }
        let room = rows.saturating_sub(frame.len() + min);
        if section.len() <= room {
            frame.extend(section);
        } else if room > 0 {
            // Keep the step header and the newest lines.
            frame.push(section[0].clone());
            frame.extend(section[section.len() + 1 - room..].iter().cloned());
        }
        let avail = rows.saturating_sub(frame.len());
        let mut lines = widget(avail);
        lines.truncate(avail);
        frame.extend(lines);
        self.paint(frame, (rows as u16, cols as u16));
    }

    fn paint(&mut self, frame: Vec<String>, size: (u16, u16)) {
        let Some(live) = self.live.as_mut() else { return };
        let width = (size.1 as usize).saturating_sub(1);
        let frame: Vec<String> = frame
            .into_iter()
            .map(|l| {
                if measure_text_width(&l) > width {
                    truncate_str(&l, width, "…").into_owned()
                } else {
                    l
                }
            })
            .collect();
        let mut out = String::from("\x1b[?2026h");
        if size != live.size {
            out.push_str("\x1b[H\x1b[2J");
            live.shown.clear();
            live.size = size;
        }
        for i in 0..frame.len().max(live.shown.len()) {
            let new = frame.get(i).map(String::as_str).unwrap_or("");
            if live.shown.get(i).map(String::as_str) != Some(new) {
                out.push_str(&format!("\x1b[{};1H{new}\x1b[0m\x1b[K", i + 1));
            }
        }
        out.push_str("\x1b[?2026l");
        let _ = live.term.write_str(&out);
        live.shown = frame;
    }

    /// Animates until a key arrives (returned) or `timeout` passes (None). Ctrl+C cancels.
    pub fn poll(&mut self, min: usize, widget: &dyn Fn(usize) -> Vec<String>, timeout: Option<Duration>) -> Option<Key> {
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            self.draw(min, widget);
            let wait = deadline
                .map(|d| d.saturating_duration_since(Instant::now()).min(FRAME))
                .unwrap_or(FRAME);
            if RawInput::wait(wait) {
                let key = self.live.as_ref().map(|l| l.term.read_key_raw());
                match key {
                    Some(Ok(Key::CtrlC | Key::Char('\u{3}'))) if self.ctrl_c_is_key => return Some(Key::CtrlC),
                    Some(Ok(Key::CtrlC | Key::Char('\u{3}'))) | Some(Err(_)) | None => self.cancel(),
                    Some(Ok(key)) => return Some(key),
                }
            }
            if deadline.is_some_and(|d| Instant::now() >= d) {
                return None;
            }
        }
    }

    fn key(&mut self, min: usize, widget: &dyn Fn(usize) -> Vec<String>) -> Key {
        loop {
            if let Some(k) = self.poll(min, widget, None) {
                return k;
            }
        }
    }

    /// Keeps animating for `d` (so a step is seen, not just flashed).
    pub fn pause(&mut self, d: Duration) {
        if self.is_live() {
            let nothing = |_: usize| Vec::new();
            self.poll(0, &nothing, Some(d));
        }
    }

    // ---- Widgets --------------------------------------------------------------------------------

    /// Runs `work` on another thread with a spinner line until it's done.
    pub fn work<T: Send + 'static>(&mut self, label: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
        if !self.is_live() {
            return ui::with_spinner(label, work);
        }
        let worker = std::thread::spawn(work);
        let line = |_: usize| vec![format!("  {} {label}", ui::spin())];
        while !worker.is_finished() {
            self.poll(1, &line, Some(FRAME));
        }
        worker.join().expect("worker")
    }

    /// A yes/no question. Settles as `✓ prompt · yes`.
    pub fn confirm(&mut self, prompt: &str, default: bool) -> bool {
        if !self.is_live() {
            return plain_confirm(prompt, default);
        }
        platform::flush_input();
        let mut yes = default;
        let answer = loop {
            let render = |_: usize| {
                let (y, n) = if yes {
                    (accent(bold("Yes")), dim("No"))
                } else {
                    (dim("Yes"), accent(bold("No")))
                };
                vec![
                    format!("  {} {}  {y}  {n}", bold(prompt), dim("›")),
                    String::new(),
                    format!("  {}", dim("←→ to choose · y/n · enter to confirm")),
                ]
            };
            match self.key(3, &render) {
                Key::ArrowLeft | Key::ArrowRight | Key::Tab | Key::ArrowUp | Key::ArrowDown => yes = !yes,
                Key::Char('y' | 'Y') => break true,
                Key::Char('n' | 'N') | Key::Escape => break false,
                Key::Enter => break yes,
                _ => {}
            }
        };
        self.line(format!(
            "  {} {} {} {}",
            green("✓"),
            prompt,
            dim("·"),
            accent(if answer { "yes" } else { "no" })
        ));
        answer
    }

    /// `Enter to …  · esc to skip`. True on Enter.
    pub fn press_enter(&mut self, prompt: &str) -> bool {
        if !self.is_live() {
            return plain_confirm(prompt, true);
        }
        platform::flush_input();
        let render = |_: usize| vec![format!("  {prompt}")];
        loop {
            match self.key(1, &render) {
                Key::Enter => return true,
                Key::Escape | Key::Char('q') => return false,
                _ => {}
            }
        }
    }

    /// A one-line text field. `secret` shows dots except the last four characters. Returns None on
    /// Esc. Pasting works (the characters arrive as keys).
    pub fn input(&mut self, prompt: &str, hint: &str, secret: bool) -> Option<String> {
        if !self.is_live() {
            return plain_input(prompt);
        }
        platform::flush_input();
        let mut value = String::new();
        loop {
            let shown = if secret { crate::config::mask(&value) } else { value.clone() };
            let render = |_: usize| {
                let caret = if ((ui::clock() * 2.0) as u64).is_multiple_of(2) {
                    accent("▍")
                } else {
                    " ".into()
                };
                vec![
                    format!("  {} {}", bold(prompt), dim("›")),
                    format!("  {}{}{caret}", accent("❯ "), shown),
                    String::new(),
                    format!("  {}", dim(hint)),
                ]
            };
            match self.key(4, &render) {
                Key::Enter if !value.trim().is_empty() => break,
                Key::Escape => return None,
                Key::Backspace => {
                    value.pop();
                }
                Key::Char(c) if !c.is_control() => value.push(c),
                _ => {}
            }
        }
        let value = value.trim().to_string();
        let shown = if secret { crate::config::mask(&value) } else { value.clone() };
        self.line(format!("  {} {} {} {}", green("✓"), prompt, dim("·"), accent(shown)));
        Some(value)
    }

    // ---- Ending -------------------------------------------------------------------------------

    /// Leaves the full screen and prints the hero (still) and the transcript.
    pub fn finish(&mut self) {
        let Some(live) = self.live.take() else { return };
        let _ = live.term.write_str("\x1b[?2026l\x1b[0m\x1b[?1049l\x1b[?25h");
        drop(live);
        ui::print_hero(&self.hero_right(false));
        if self.transcript.first().is_some_and(String::is_empty) {
            self.transcript.remove(0);
        }
        for l in self.transcript.drain(..) {
            println!("{}", expand_step(&l));
        }
    }

    /// Ctrl+C: restore the terminal, keep what was settled, and exit.
    pub fn cancel(&mut self) -> ! {
        if self.is_live() {
            self.blank();
            self.warn("Cancelled.");
            self.blank();
        }
        self.finish();
        std::process::exit(130);
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        self.finish();
    }
}

fn expand_step(line: &str) -> String {
    match line.strip_prefix(STEP) {
        Some(step) => {
            let mut parts = step.splitn(3, STEP);
            let n = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            let total = parts.next().and_then(|n| n.parse().ok()).unwrap_or(0);
            ui::step_line(n, total, parts.next().unwrap_or(""))
        }
        None => line.to_string(),
    }
}

pub fn plain_confirm(prompt: &str, default: bool) -> bool {
    use std::io::{BufRead, Write};
    print!("  {prompt} {} ", dim(if default { "[Y/n]" } else { "[y/N]" }));
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line).unwrap_or(0) == 0 {
        return default;
    }
    match line.trim().to_lowercase().as_str() {
        "" => default,
        a => a.starts_with('y'),
    }
}

fn plain_input(prompt: &str) -> Option<String> {
    use std::io::{BufRead, Write};
    print!("  {prompt}: ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line).ok()?;
    let v = line.trim().to_string();
    (!v.is_empty()).then_some(v)
}
