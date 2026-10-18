import { ExternalLink, FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";
import { api, type AppState } from "../lib/api";
import { useFeedback } from "./feedback";
import { Button, Modal, Spinner } from "./primitives";

export type Theme = "system" | "light" | "dark";

const LANGUAGES: { code: string; label: string }[] = [
  { code: "mr-IN", label: "Marathi" },
  { code: "hi-IN", label: "Hindi" },
  { code: "en-IN", label: "English" },
];

export function SettingsDialog({
  open,
  onClose,
  state,
  onState,
  theme,
  onTheme,
  focusKey,
}: {
  open: boolean;
  onClose: () => void;
  state: AppState | null;
  onState: (s: AppState) => void;
  theme: Theme;
  onTheme: (t: Theme) => void;
  focusKey: boolean;
}) {
  const fb = useFeedback();
  const [editingKey, setEditingKey] = useState(false);
  const [key, setKey] = useState("");
  const [checking, setChecking] = useState(false);
  const [keyError, setKeyError] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setEditingKey(focusKey || !state?.has_key);
      setKey("");
      setKeyError(null);
    }
  }, [open, focusKey, state?.has_key]);

  if (!state) return null;

  const update = async (patch: Parameters<typeof api.settings>[0]) => {
    try {
      onState(await api.settings(patch));
    } catch (e) {
      fb.toast(e instanceof Error ? e.message : String(e), { tone: "error" });
    }
  };

  const saveKey = async () => {
    setChecking(true);
    setKeyError(null);
    try {
      onState(await api.settings({ api_key: key.trim() }));
      setEditingKey(false);
      setKey("");
      fb.toast("API key saved");
    } catch (e) {
      setKeyError(e instanceof Error ? e.message : String(e));
    } finally {
      setChecking(false);
    }
  };

  const langs = state.language_codes;
  const toggleLang = (code: string) => void update({ language_codes: langs.includes(code) ? langs.filter((c) => c !== code) : [...langs, code] });

  return (
    <Modal open={open} onClose={onClose} title="Settings" width={520} footer={<Button onClick={onClose}>Done</Button>}>
      <div className="settings">
        <section className="setting">
          <div className="setting-text">
            <h3>Gemini API key</h3>
            <p>
              Stored in <code>~/.parrot/config.json</code>, readable only by you. Get one free in{" "}
              <a href={state.keys_url} target="_blank" rel="noreferrer">
                Google AI Studio <ExternalLink size={11} strokeWidth={2} />
              </a>
              .
            </p>
          </div>
          {editingKey ? (
            <form
              className="key-form"
              onSubmit={(e) => {
                e.preventDefault();
                if (key.trim()) void saveKey();
              }}
            >
              <input
                className="input mono"
                type="password"
                autoComplete="off"
                spellCheck={false}
                placeholder="AIza…"
                value={key}
                autoFocus
                onChange={(e) => setKey(e.target.value)}
                aria-label="Gemini API key"
                aria-invalid={!!keyError}
              />
              <div className="key-actions">
                {state.has_key && (
                  <Button size="sm" onClick={() => setEditingKey(false)}>
                    Cancel
                  </Button>
                )}
                <Button size="sm" variant="primary" type="submit" disabled={!key.trim() || checking}>
                  {checking ? (
                    <>
                      <Spinner size={12} /> Checking…
                    </>
                  ) : (
                    "Save key"
                  )}
                </Button>
              </div>
              {keyError && <p className="field-error">{keyError}</p>}
            </form>
          ) : (
            <div className="setting-control">
              <code className="key-mask">{state.masked_key}</code>
              <Button size="sm" onClick={() => setEditingKey(true)}>
                Replace
              </Button>
            </div>
          )}
        </section>

        <section className="setting">
          <div className="setting-text">
            <h3>Transcription languages</h3>
            <p>Leave all off to detect automatically. Pick the ones your meetings use if Parrot guesses wrong.</p>
          </div>
          <div className="chips" role="group" aria-label="Transcription languages">
            {LANGUAGES.map((l) => (
              <button key={l.code} type="button" className={`chip ${langs.includes(l.code) ? "is-on" : ""}`} aria-pressed={langs.includes(l.code)} onClick={() => toggleLang(l.code)}>
                {l.label}
              </button>
            ))}
          </div>
        </section>

        <section className="setting is-row">
          <div className="setting-text">
            <h3>Assistant voice</h3>
          </div>
          <select className="select" value={state.voice} onChange={(e) => void update({ voice: e.target.value })} aria-label="Assistant voice">
            {state.voices.map((v) => (
              <option key={v}>{v}</option>
            ))}
          </select>
        </section>

        <section className="setting is-row">
          <div className="setting-text">
            <h3>Appearance</h3>
          </div>
          <div className="segmented" role="radiogroup" aria-label="Appearance">
            {(["system", "light", "dark"] as Theme[]).map((t) => (
              <button key={t} type="button" role="radio" aria-checked={theme === t} className={theme === t ? "is-on" : ""} onClick={() => onTheme(t)}>
                {t[0]!.toUpperCase() + t.slice(1)}
              </button>
            ))}
          </div>
        </section>

        <section className="setting is-row">
          <div className="setting-text">
            <h3>Notebook folder</h3>
            <p className="mono small">{state.data_dir_display}</p>
          </div>
          <Button size="sm" onClick={() => void api.reveal()}>
            <FolderOpen size={14} strokeWidth={1.75} /> Show
          </Button>
        </section>

        <p className="settings-about">
          Parrot {state.version} · {state.models.transcribe} · {state.models.voice}
        </p>
      </div>
    </Modal>
  );
}
