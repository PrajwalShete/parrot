#!/bin/sh
# Parrot installer: https://github.com/PrajwalShete/parrot
#
#   curl -fsSL https://raw.githubusercontent.com/PrajwalShete/parrot/main/install.sh | sh
#
# Options:  --version <x.y.z>   install a specific release (default: latest)
#           --no-setup          install only; run `parrot` yourself later
#           --dry-run           show what would happen
# Env:      PARROT_INSTALL_DIR  where the binary goes (default ~/.parrot/bin)
#           PARROT_BINARY       install this local file instead of downloading (for testing)
#           NO_COLOR            plain output
set -eu

REPO="PrajwalShete/parrot"
VERSION="${PARROT_VERSION:-latest}"
INSTALL_DIR="${PARROT_INSTALL_DIR:-$HOME/.parrot/bin}"
LINK_DIR="$HOME/.local/bin"
RUN_SETUP=1
DRY_RUN=0

while [ $# -gt 0 ]; do
  case "$1" in
    --version) VERSION="${2#v}"; shift 2 ;;
    --no-setup) RUN_SETUP=0; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h | --help) sed -n '2,13p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) printf 'parrot install: unknown option %s\n' "$1" >&2; exit 2 ;;
  esac
done

# ---- Look ---------------------------------------------------------------------------------------

if [ -t 1 ] && [ -z "${NO_COLOR:-}" ] && [ "${TERM:-dumb}" != dumb ]; then
  FANCY=1
  ESC=$(printf '\033')
  RESET="${ESC}[0m"; BOLD="${ESC}[1m"; DIM="${ESC}[2m"
  GREEN="${ESC}[38;5;42m"; RED="${ESC}[38;5;196m"; YELLOW="${ESC}[38;5;220m"
  CLEAR="${ESC}[2K"; HIDE="${ESC}[?25l"; SHOW="${ESC}[?25h"
else
  FANCY=0
  RESET=""; BOLD=""; DIM=""; GREEN=""; RED=""; YELLOW=""; CLEAR=""; HIDE=""; SHOW=""
fi

restore() { printf '%s' "$SHOW"; }
trap 'restore' EXIT
trap 'restore; printf "\n"; exit 130' INT TERM

say() { printf '%s\n' "$*"; }
ok() { printf '\r%s  %s✓%s %s\n' "$CLEAR" "$GREEN" "$RESET" "$*"; }
warn() { printf '\r%s  %s!%s %s\n' "$CLEAR" "$YELLOW" "$RESET" "$*"; }
fail() { printf '\r%s  %s✗%s %s\n' "$CLEAR" "$RED" "$RESET" "$*" >&2; exit 1; }

# Three voice bars in macaw colors, rising and falling: the spinner.
# Frames are bar heights; each bar has its own color.
bar() {
  case "$1" in 1) printf '▁' ;; 2) printf '▂' ;; 3) printf '▃' ;; 4) printf '▄' ;; 5) printf '▅' ;; 6) printf '▆' ;; *) printf '▇' ;; esac
}

# Prints $1 repeated $2 times (portable; no printf '%*s').
repeat() {
  r=""; k=0
  while [ "$k" -lt "$2" ]; do r="$r$1"; k=$(( k + 1 )); done
  printf '%s' "$r"
}
FRAME=0
voice() {
  if [ "$FANCY" = 0 ]; then return; fi
  # Heights cycle with offsets so the bars move like speech.
  h1=$(( (FRAME * 3) % 12 )); h2=$(( (FRAME * 3 + 4) % 12 )); h3=$(( (FRAME * 3 + 8) % 12 ))
  for v in h1 h2 h3; do
    eval "x=\$$v"
    if [ "$x" -gt 6 ]; then x=$(( 12 - x )); fi
    eval "$v=$(( x + 1 ))"
  done
  printf '%s%s%s%s%s%s%s' "${ESC}[38;5;208m" "$(bar "$h1")" "${ESC}[38;5;220m" "$(bar "$h2")" "${ESC}[38;5;42m" "$(bar "$h3")" "$RESET"
  FRAME=$(( FRAME + 1 ))
}

# Runs "$@" in the background with an animated line until it finishes.
spin() {
  label="$1"; shift
  if [ "$FANCY" = 0 ]; then
    say "  $label"
    "$@"
    return
  fi
  "$@" &
  pid=$!
  printf '%s' "$HIDE"
  while kill -0 "$pid" 2>/dev/null; do
    printf '\r%s  %s %s' "$CLEAR" "$(voice)" "$label"
    sleep 0.08
  done
  wait "$pid"
}

# ---- Intro --------------------------------------------------------------------------------------

intro() {
  if [ "$FANCY" = 0 ]; then
    say ""
    say "  parrot  Hear every meeting. Ask it anything."
    say ""
    return
  fi
  printf '%s\n' "$HIDE"
  name="parrot"
  i=1
  while [ "$i" -le 6 ]; do
    shown=$(printf '%s' "$name" | awk -v n="$i" '{ print substr($0, 1, n) }')
    printf '\r%s  %s  %s%s%s' "$CLEAR" "$(voice)" "$BOLD$GREEN" "$shown" "$RESET"
    sleep 0.05
    i=$(( i + 1 ))
  done
  n=0
  while [ "$n" -lt 10 ]; do
    printf '\r%s  %s  %sparrot%s  %sHear every meeting. Ask it anything.%s' "$CLEAR" "$(voice)" "$BOLD$GREEN" "$RESET" "$DIM" "$RESET"
    sleep 0.06
    n=$(( n + 1 ))
  done
  printf '\r%s  %s▃▆▄%s  %sparrot%s  %sHear every meeting. Ask it anything.%s\n\n' "$CLEAR" "$GREEN" "$RESET" "$BOLD$GREEN" "$RESET" "$DIM" "$RESET"
}

# ---- Platform -----------------------------------------------------------------------------------

detect() {
  case "$(uname -s)" in
    Darwin) os=darwin; os_name="macOS" ;;
    Linux) os=linux; os_name="Linux" ;;
    *) fail "Parrot runs on macOS and Linux. On Windows, use WSL." ;;
  esac
  case "$(uname -m)" in
    arm64 | aarch64) arch=arm64 ;;
    x86_64 | amd64) arch=x64 ;;
    *) fail "unsupported processor: $(uname -m)" ;;
  esac
  # An Apple Silicon Mac running this shell under Rosetta still wants the native build.
  if [ "$os" = darwin ] && [ "$arch" = x64 ] && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
    arch=arm64
  fi
  if [ "$os" = darwin ]; then
    if [ "$arch" = arm64 ]; then chip="Apple Silicon"; else chip="Intel"; fi
  else
    if [ "$arch" = arm64 ]; then chip="ARM64"; else chip="x86-64"; fi
  fi
  asset="parrot-$os-$arch"
}

# ---- Download -----------------------------------------------------------------------------------

human() {
  awk -v b="$1" 'BEGIN { if (b >= 1048576) printf "%.1f MB", b / 1048576; else printf "%d KB", b / 1024 }'
}

size_of() { wc -c < "$1" 2>/dev/null | tr -d ' ' || echo 0; }

# Downloads $1 to $2 with a progress bar.
download() {
  url="$1"; out="$2"
  total=$(curl -fsSIL --proto '=https' --connect-timeout 15 "$url" 2>/dev/null | tr -d '\r' | awk 'tolower($1) == "content-length:" { n = $2 } END { print n + 0 }')
  : > "$out"
  curl -fsSL --proto '=https' --tlsv1.2 --connect-timeout 15 --max-time 600 --retry 3 --retry-delay 1 "$url" -o "$out" 2> "$out.err" &
  pid=$!
  label="Downloading parrot $shown_version"
  if [ "$FANCY" = 1 ]; then printf '%s' "$HIDE"; fi
  while kill -0 "$pid" 2>/dev/null; do
    if [ "$FANCY" = 1 ]; then
      got=$(size_of "$out")
      if [ "${total:-0}" -gt 0 ]; then
        pct=$(( got * 100 / total )); [ "$pct" -gt 100 ] && pct=100
        filled=$(( pct * 24 / 100 ))
        bar_on=$(repeat '━' "$filled")
        bar_off=$(repeat '─' "$(( 24 - filled ))")
        printf '\r%s  %s %s  %s%s%s%s%s  %s%3d%%  %s / %s%s' "$CLEAR" "$(voice)" "$label" "$GREEN" "$bar_on" "$RESET$DIM" "$bar_off" "$RESET" "$DIM" "$pct" "$(human "$got")" "$(human "$total")" "$RESET"
      else
        printf '\r%s  %s %s  %s%s%s' "$CLEAR" "$(voice)" "$label" "$DIM" "$(human "$got")" "$RESET"
      fi
    fi
    sleep 0.08
  done
  if ! wait "$pid"; then
    reason=$(cat "$out.err" 2>/dev/null)
    rm -f "$out" "$out.err"
    fail "download failed: $url ${reason:+($reason)}"
  fi
  rm -f "$out.err"
  ok "Downloaded parrot $shown_version ${DIM}($(human "$(size_of "$out")"))${RESET}"
}

checksum() {
  if command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | cut -d ' ' -f 1; else sha256sum "$1" | cut -d ' ' -f 1; fi
}

# ---- Main ---------------------------------------------------------------------------------------

intro
detect
ok "Detected $os_name · $chip"

if [ -n "${PARROT_BINARY:-}" ]; then
  base=""
  shown_version="(local build)"
elif [ "$VERSION" = latest ]; then
  base="https://github.com/$REPO/releases/latest/download"
  tag=$(curl -fsSLI --proto '=https' --connect-timeout 15 -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" 2>/dev/null | sed 's|.*/tag/||')
  case "$tag" in v[0-9]*) shown_version="$tag" ;; *) shown_version="(latest)" ;; esac
else
  base="https://github.com/$REPO/releases/download/v$VERSION"
  shown_version="v$VERSION"
fi

if [ "$DRY_RUN" = 1 ]; then
  say "  Would download ${base:-$PARROT_BINARY}/$asset"
  say "  Would install to $INSTALL_DIR/parrot and link $LINK_DIR/parrot"
  [ "$RUN_SETUP" = 1 ] && say "  Would then run: parrot"
  exit 0
fi

command -v curl >/dev/null 2>&1 || fail "curl is required."
mkdir -p "$INSTALL_DIR" "$LINK_DIR"
tmp="$INSTALL_DIR/.parrot-download.$$"
trap 'restore; rm -f "$tmp" "$tmp.sha256" "$tmp.err"' EXIT

if [ -n "${PARROT_BINARY:-}" ]; then
  cp "$PARROT_BINARY" "$tmp"
  ok "Using local build ${DIM}$PARROT_BINARY${RESET}"
else
  download "$base/$asset" "$tmp"
  if curl -fsSL --proto '=https' --connect-timeout 10 --max-time 30 "$base/$asset.sha256" -o "$tmp.sha256" 2>/dev/null; then
    expected=$(cut -d ' ' -f 1 < "$tmp.sha256")
    actual=$(checksum "$tmp")
    [ "$expected" = "$actual" ] || fail "checksum mismatch: the download is corrupt or was tampered with."
    ok "Verified checksum ${DIM}(sha256 ${actual%"${actual#????????????}"}…)${RESET}"
  else
    warn "No checksum published for this release; skipped verification."
  fi
fi

chmod 755 "$tmp"
if [ "$os" = darwin ]; then xattr -d com.apple.quarantine "$tmp" 2>/dev/null || true; fi
check_binary() { "$tmp" --version >/dev/null 2>&1; }
if [ "$os" = darwin ]; then
  spin "Checking it runs on this Mac (the first launch can take a few seconds)…" check_binary || fail "the downloaded program doesn't run on this machine."
else
  check_binary || fail "the downloaded program doesn't run on this machine."
fi
mv -f "$tmp" "$INSTALL_DIR/parrot"
ln -sf "$INSTALL_DIR/parrot" "$LINK_DIR/parrot"
installed=$("$INSTALL_DIR/parrot" --version 2>/dev/null | awk '{ print $2 }')
ok "Installed parrot ${BOLD}v$installed${RESET} ${DIM}→ $INSTALL_DIR/parrot${RESET}"

case ":$PATH:" in
  *":$LINK_DIR:"*) ;;
  *)
    # Written literally into the shell's startup file, so $HOME expands there, not here.
    # shellcheck disable=SC2016
    line='export PATH="$HOME/.local/bin:$PATH"'
    case "${SHELL:-}" in
      */zsh) rc="$HOME/.zshrc" ;;
      */bash) if [ "$os" = darwin ]; then rc="$HOME/.bash_profile"; else rc="$HOME/.bashrc"; fi ;;
      */fish) rc="$HOME/.config/fish/config.fish"; line="fish_add_path \$HOME/.local/bin" ;;
      *) rc="" ;;
    esac
    if [ -n "$rc" ]; then
      mkdir -p "$(dirname "$rc")"
      grep -qsF "$line" "$rc" || printf '\n# Added by the Parrot installer\n%s\n' "$line" >> "$rc"
      ok "Added ~/.local/bin to your PATH ${DIM}(in $rc; new terminals pick it up)${RESET}"
    else
      warn "Add ~/.local/bin to your PATH:  $line"
    fi
    ;;
esac

say ""
if [ "$RUN_SETUP" = 0 ]; then
  say "  Next, run:  ${BOLD}parrot${RESET}"
elif [ -f "${PARROT_HOME:-$HOME/.parrot}/config.json" ]; then
  say "  ${GREEN}Updated.${RESET} Your notes and settings were kept. Run ${BOLD}parrot${RESET} to open it."
elif (exec </dev/tty) 2>/dev/null; then
  restore
  exec "$INSTALL_DIR/parrot" </dev/tty
else
  say "  Next, run:  ${BOLD}parrot${RESET}"
fi
