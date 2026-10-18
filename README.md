<p align="center">
  <img src="docs/logo.svg" width="64" height="64" alt="">
</p>

<h1 align="center">Parrot</h1>

<p align="center">
  <b>Hear every meeting. Ask it anything.</b><br>
  Live transcription in Marathi, Hindi and English, and a voice assistant that reads and edits your notes.<br>
  Runs on your machine, opens in your browser, keeps everything as plain Markdown files.
</p>

---

## Install

macOS (Apple Silicon and Intel) and Linux (x86-64 and ARM64):

```bash
curl -fsSL https://raw.githubusercontent.com/PrajwalShete/parrot/main/install.sh | sh
```

The installer downloads the right build for your machine, checks its SHA-256, puts `parrot` on your `PATH` and starts setup. Setup takes about a minute:

1. It creates your notebook at `~/Documents/Parrot`.
2. It asks for a free Gemini API key from [Google AI Studio](https://aistudio.google.com/app/apikey) and stores it in `~/.parrot/config.json`, readable only by you.
3. It opens Parrot in your browser.

After that, run `parrot` whenever you want it.

## What it does

**Record meetings live.** Put a laptop or a conference mic in the middle of the table and press Record. Parrot streams the room to Gemini 3.5 Transcribe Live and writes each finished sentence into a note as it happens:

```
[10:00:11] पुणे में इस महीने 4.5 लाख की सेल हुई, पिछले महीने से 12 प्रतिशत ज़्यादा।
[10:00:19] नाशिकचा supplier अजून delay करतोय, stock Friday पर्यंत येईल.
[10:00:27] Action item: Rahul will call the Nashik supplier by Wednesday.
```

It detects the language on its own and handles people switching languages mid-sentence. Long meetings are fine: Gemini caps a live session at 10 minutes, so Parrot moves to a fresh session just before the limit, without losing audio.

**Organize by topic.** Topics are folders, and subtopics nest inside them (Sales → Pune → Weekly review). Create, rename, move (drag and drop works) and delete from the sidebar. Deleting goes to a trash with Undo.

**Talk to your notes.** The assistant on the right uses Gemini 3.8 Live, a voice-to-voice model. Ask it things like:

- “What did we decide about the Pune stock today?”
- “Add a summary with action items to this meeting.”
- “Find every meeting where the IoT cost came up.”
- “Rename Rahul to Rahul Patil in all the sales notes.”

It answers out loud in the language you speak, and it works by actually reading and editing your files. Every tool call shows in the panel as it happens.

## How it works

```
 browser ──mic 16 kHz PCM──▶ parrot (127.0.0.1:4747) ──▶ Gemini 3.5 Transcribe Live
         ◀─live text─────────                          ──▶ Gemini 3.8 Live (voice + tools)
         ◀─voice 24 kHz──────        │
                                     ▼
                         ~/Documents/Parrot/**/*.md
```

- **One binary.** `parrot` is a Rust program with the web app built in. It serves the UI on `127.0.0.1` only and relays audio to Gemini over WebSockets. Your API key never reaches the browser.
- **Plain files.** Topics are folders and notes are Markdown, so you can open the notebook in Finder, VS Code or Obsidian. Parrot watches the folder, so edits from anywhere show up live.
- **Safe tools.** The assistant can list, read, search, create, edit, append, rename, move and delete notes. It can also do bulk find-and-replace across notes (the `sed -i` use case, with a dry-run preview) and run read-only shell pipelines such as `grep -ri pune . | wc -l`. Everything is locked to the notebook folder: no paths outside it, no redirection, no arbitrary programs, and deletes go to the trash.
- **Locked to localhost.** The server answers only requests that name `localhost` as the host and come from its own page, so other websites can't drive it (no DNS rebinding or CSRF).

## Commands

| Command | What it does |
| --- | --- |
| `parrot` | Set up on first run, then start the app and open the browser |
| `parrot setup` | Run setup again (keeps your notes) |
| `parrot key` | Replace the Gemini API key |
| `parrot status` | Where things are, and whether the app is running |
| `parrot doctor` | Check the key, the folder and both Gemini models, and explain fixes |
| `parrot update` | Install the latest release |
| `parrot uninstall` | Remove Parrot and its settings (your notes stay) |

Options: `--port <n>` (default 4747), `--no-open` (or `PARROT_NO_OPEN=1`).

While it runs, the terminal shows a live dashboard: press `o` to open the browser again, `q` to quit.

## Keyboard

| Keys | Action |
| --- | --- |
| `⌘K` / `Ctrl+K` | Search notes and topics, run commands |
| `⌘J` / `Ctrl+J` | Show or hide the assistant |
| `⌘\` / `Ctrl+\` | Show or hide the sidebar |
| `E` | Edit the open note (`Esc` to finish) |
| `↑ ↓ ← →`, `Enter`, `F2`, `Delete` | Move through the topic tree, open, rename, delete |

## Limits and costs

- Parrot uses your own Gemini API key. On the **free tier**, Google may use your audio and text to improve its products. For company meetings, use a paid key.
- The live transcriber doesn't label speakers. The assistant can often tell who said what from context, and saying your name before you speak helps.
- On the free tier, transcription allows about 20,000 tokens per minute. A meeting uses roughly 1,900 tokens per minute, so the limit isn't a concern for one room.

## Development

You need Rust (stable) and Node 22+.

```bash
make web           # build the UI into web/dist (embedded into the binary)
cargo run -- --port 4799 --no-open
make dev           # in another terminal: the UI with hot reload on :5173, proxied to :4799
make test          # Rust and web tests
make check         # fmt, clippy, typecheck
```

Releases: push a tag like `v0.1.0`. CI builds macOS (arm64, x64) and Linux (x64, arm64, static musl) binaries and attaches them, with checksums, to a GitHub release.

To check the Gemini integration end to end, `examples/live_check.rs` streams a WAV file through a running Parrot and prints every event.

## License

[MIT](LICENSE) © Prajwal Shete
