//! The terminal design system: one accent (macaw green), quiet grays, step headers, cards,
//! spinners, and the animated Parrot mark.

use console::{Term, measure_text_width, style};
use std::time::Duration;

/// 42 = macaw green, the accent everywhere in the terminal.
pub const ACCENT: u8 = 42;

pub fn accent(msg: impl std::fmt::Display) -> String {
    style(msg).color256(ACCENT).to_string()
}

pub fn dim(msg: impl std::fmt::Display) -> String {
    style(msg).dim().to_string()
}

pub fn bold(msg: impl std::fmt::Display) -> String {
    style(msg).bold().to_string()
}

pub fn cyan(msg: impl std::fmt::Display) -> String {
    style(msg).cyan().to_string()
}

pub fn green(msg: impl std::fmt::Display) -> String {
    style(msg).green().to_string()
}

pub fn ok(msg: impl std::fmt::Display) {
    println!("  {} {msg}", style("✓").green());
}

pub fn warn(msg: impl std::fmt::Display) {
    println!("  {} {msg}", style("!").yellow().bold());
}

pub fn fail(msg: impl std::fmt::Display) {
    eprintln!("  {} {msg}", style("✗").red().bold());
}

pub fn info(msg: impl std::fmt::Display) {
    println!("  {msg}");
}

pub fn blank() {
    println!();
}

pub fn width() -> usize {
    Term::stdout().size_checked().map(|(_, w)| w as usize).unwrap_or(80).clamp(40, 100)
}

/// `── 1 of 3 · Your notebook ──────────`
pub fn step_line(n: usize, total: usize, title: &str) -> String {
    let label = format!(" {n} of {total} · {title} ");
    let line = width().min(72) - 4;
    let rule = "─".repeat(line.saturating_sub(2 + measure_text_width(&label)));
    format!("  {}{}{}", dim("──"), style(label).bold(), dim(rule))
}

/// A rounded box around `lines` (already styled). Plain lines on narrow terminals.
pub fn card_lines(title: &str, lines: &[String]) -> Vec<String> {
    let inner = lines
        .iter()
        .map(|l| measure_text_width(l))
        .chain([measure_text_width(title)])
        .max()
        .unwrap_or(0)
        + 4;
    if inner + 4 > width() {
        return std::iter::once(format!("  {}", bold(title)))
            .chain(lines.iter().map(|l| format!("  {l}")))
            .collect();
    }
    let pad = |s: &str| format!("{s}{}", " ".repeat(inner.saturating_sub(measure_text_width(s) + 2)));
    let mut out = vec![format!("  {}", dim(format!("╭{}╮", "─".repeat(inner))))];
    out.push(format!("  {} {} {}", dim("│"), pad(&bold(title)), dim("│")));
    out.push(format!("  {} {} {}", dim("│"), pad(""), dim("│")));
    for l in lines {
        out.push(format!("  {} {} {}", dim("│"), pad(l), dim("│")));
    }
    out.push(format!("  {}", dim(format!("╰{}╯", "─".repeat(inner)))));
    out
}

pub fn card(title: &str, lines: &[String]) {
    for l in card_lines(title, lines) {
        println!("{l}");
    }
}

/// Replaces the home directory with `~` for display.
pub fn tilde(path: &std::path::Path) -> String {
    if let Some(home) = dirs::home_dir()
        && let Ok(rest) = path.strip_prefix(&home)
    {
        return if rest.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display())
        };
    }
    path.display().to_string()
}

/// One clock for every animation, so handing over from one screen to the next doesn't jump.
pub fn clock() -> f64 {
    static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    START.get_or_init(std::time::Instant::now).elapsed().as_secs_f64()
}

/// A spinner glyph in step with the mark.
pub fn spin() -> String {
    const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    style(FRAMES[(clock() * 12.0) as usize % FRAMES.len()]).color256(ACCENT).to_string()
}

/// A spinner for plain (non full screen) output: runs `work` on a thread meanwhile.
pub fn with_spinner<T: Send + 'static>(label: &str, work: impl FnOnce() -> T + Send + 'static) -> T {
    use std::io::Write;
    let handle = std::thread::spawn(work);
    let tty = console::user_attended();
    while !handle.is_finished() {
        if tty {
            print!("\r\x1b[2K  {} {label}", spin());
            let _ = std::io::stdout().flush();
        }
        std::thread::sleep(Duration::from_millis(70));
    }
    if tty {
        print!("\r\x1b[2K");
        let _ = std::io::stdout().flush();
    }
    handle.join().expect("worker")
}

// ---- The mark -----------------------------------------------------------------------------------

pub const MARK_W: usize = 40;
pub const MARK_H: usize = 17;

/// Macaw feathers around the ring: red, orange, yellow, green, teal, blue, and back.
const FEATHERS: [u8; 12] = [196, 202, 208, 214, 220, 190, 118, 48, 43, 39, 33, 63];

/// The mark at `t` seconds, `w` by `h` cells: a ring of macaw colors turning slowly, sound waves
/// rippling out from it, and five voice bars in the middle that move like someone speaking.
/// Computed, so it stays crisp at every size.
pub fn mark_sized(t: f64, w: usize, h: usize) -> Vec<String> {
    const RAMP: &[u8] = b" .:-=+*#%@";
    let scale = MARK_H as f64 / h as f64;
    // Cells are about twice as tall as wide.
    let sample = |i: f64, j: f64| -> (f64, f64) {
        let x = (i - (w as f64 - 1.0) / 2.0) / 2.0 * scale;
        let y = (j - (h as f64 - 1.0) / 2.0) * scale;
        let r = x.hypot(y);
        // The ring.
        let ring = (-((r - 6.0) / 0.85).powi(2)).exp();
        // Waves leaving the ring.
        let mut wave = 0.0;
        if r > 6.4 {
            let phase = ((r - 6.4) / 2.4 - t / 1.7).rem_euclid(1.0);
            let fade = ((8.6 - r) / 2.2).clamp(0.0, 1.0);
            wave = 0.55 * (-((phase - 0.5) / 0.12).powi(2)).exp() * fade;
        }
        (ring.max(wave), y.atan2(x))
    };
    // Voice bars, drawn in cells: thin vertical strokes with half-cell caps, so they read like an
    // equalizer at every size.
    let (cx, cy) = ((w as f64 - 1.0) / 2.0, (h as f64 - 1.0) / 2.0);
    let step = (w as f64 / 13.0).round().max(2.0);
    let bars: Vec<(usize, f64)> = (0..5)
        .map(|k| {
            let k = k as f64 - 2.0;
            let amp = 0.6 + 2.4 * ((t * 2.2 + (k + 2.0) * 1.9).sin() * 0.5 + 0.5) * (0.65 + 0.35 * (t * 0.8 + k).sin().abs());
            let amp = amp * (1.0 - 0.16 * k.abs()) / scale;
            ((cx + k * step).round() as usize, amp)
        })
        .collect();
    let mut lines = Vec::with_capacity(h);
    for j in 0..h {
        let mut line = String::new();
        for i in 0..w {
            if let Some(&(_, amp)) = bars.iter().find(|(col, _)| *col == i) {
                let d = (j as f64 - cy).abs();
                let glyph = if d <= amp - 0.25 {
                    Some('┃')
                } else if d <= amp + 0.25 {
                    Some(if (j as f64) < cy { '╻' } else { '╹' })
                } else {
                    None
                };
                if let Some(g) = glyph {
                    line.push_str(&style(g).color256(255).bold().to_string());
                    continue;
                }
            }
            let mut v = 0.0;
            for a in 0..3 {
                for b in 0..3 {
                    v += sample(i as f64 + (a as f64 + 0.5) / 3.0 - 0.5, j as f64 + (b as f64 + 0.5) / 3.0 - 0.5).0;
                }
            }
            // The angle from the cell's center (averaging angles breaks where they wrap around).
            let (_, ang) = sample(i as f64, j as f64);
            let v = v / 9.0;
            if v < 0.1 {
                line.push(' ');
                continue;
            }
            let ch = RAMP[((v * RAMP.len() as f64) as usize).min(RAMP.len() - 1)] as char;
            // Color by angle, turning slowly.
            let turn = (ang / std::f64::consts::TAU + 0.5 + t * 0.05).rem_euclid(1.0);
            let color = FEATHERS[((turn * FEATHERS.len() as f64) as usize).min(FEATHERS.len() - 1)];
            let styled = if v < 0.35 {
                style(ch).color256(color).dim()
            } else {
                style(ch).color256(color)
            };
            line.push_str(&styled.to_string());
        }
        lines.push(line);
    }
    lines
}

/// Name, tagline and version: the right column beside the mark.
pub fn brand_lines(subtitle: &str) -> Vec<String> {
    let mut lines = vec![
        style("parrot").color256(ACCENT).bold().to_string(),
        "Hear every meeting. Ask it anything.".to_string(),
        String::new(),
        dim(format!("v{}", crate::VERSION)),
    ];
    if !subtitle.is_empty() {
        lines.push(String::new());
        lines.push(dim("─".repeat(36)));
        lines.push(String::new());
        lines.push(style(subtitle).bold().to_string());
    }
    lines
}

/// The mark with `right` beside it, vertically centered, with a blank line above and below.
pub fn hero_beside(right: &[String], mark: &[String], mark_w: usize) -> Vec<String> {
    let mut out = vec![String::new()];
    let top = mark.len().saturating_sub(right.len()) / 2;
    let empty = " ".repeat(mark_w);
    for i in 0..mark.len().max(right.len()) {
        let art = mark.get(i).map(String::as_str).unwrap_or(&empty);
        let text = i.checked_sub(top).and_then(|j| right.get(j)).map(String::as_str).unwrap_or("");
        out.push(format!("  {art}    {text}"));
    }
    out.push(String::new());
    out
}

/// A still hero for plain output: the biggest mark that fits, or just the brand.
pub fn print_hero(right: &[String]) {
    let cols = Term::stdout().size_checked().map(|(_, w)| w as usize).unwrap_or(80);
    let right_w = right.iter().map(|l| measure_text_width(l)).max().unwrap_or(0);
    for (mw, mh) in [(MARK_W, MARK_H), (28, 12), (22, 9)] {
        if cols >= mw + 6 + right_w {
            for l in hero_beside(right, &mark_sized(0.6, mw, mh), mw) {
                println!("{l}");
            }
            return;
        }
    }
    println!();
    for l in right {
        println!("  {l}");
    }
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_has_the_requested_size_and_moves() {
        for (w, h) in [(MARK_W, MARK_H), (28, 12), (22, 9)] {
            let a = mark_sized(0.0, w, h);
            assert_eq!(a.len(), h);
            assert!(a.iter().all(|l| measure_text_width(l) == w), "every row is {w} wide");
        }
        assert_ne!(mark_sized(0.0, MARK_W, MARK_H), mark_sized(0.5, MARK_W, MARK_H), "it animates");
    }

    #[test]
    fn step_and_card_fit() {
        assert!(measure_text_width(&step_line(1, 3, "Your notebook")) <= 72);
        let card = card_lines("Title", &["one".into(), "two".into()]);
        assert_eq!(card.len(), 6);
    }
}
