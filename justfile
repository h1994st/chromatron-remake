set shell := ["bash", "-euo", "pipefail", "-c"]

originals := env("CHROMATRON_ORIGINALS", justfile_directory() / "chromatron_mac")
originals_url := "https://silverspaceship.com/chromatron/chromatron_mac.zip"
originals_sha256 := "cda82f278f74b025e50982dc064fce4c50e7a91c645203daa02f3b8551ee675c"

# List recipes
default:
    @just --list

# First-time setup: install the Homebrew dependencies and download the original games
init: setup fetch-originals

# Install Homebrew dependencies from the Brewfile
setup:
    brew bundle

# Download the original Mac games from silverspaceship.com and unpack them into chromatron_mac/
fetch-originals:
    #!/usr/bin/env bash
    set -euo pipefail
    dest="{{originals}}"
    missing=0
    for app in "Chromatron" "Chromatron 2" "Chromatron 3" "Chromatron 4"; do
        [[ -d "$dest/$app.app" ]] || missing=1
    done
    if [[ "$missing" == 0 ]]; then
        echo "the original apps are already in $dest"
        exit 0
    fi
    command -v unar > /dev/null || { echo "error: unar not found — run just setup" >&2; exit 1; }
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    curl -fsSL --retry 3 -o "$tmp/chromatron_mac.zip" "{{originals_url}}"
    echo "{{originals_sha256}}  $tmp/chromatron_mac.zip" | shasum -a 256 -c -s \
        || { echo "error: chromatron_mac.zip does not match the expected SHA-256" >&2; exit 1; }
    ditto -x -k "$tmp/chromatron_mac.zip" "$tmp/archives"
    mkdir -p "$dest"
    for archive in "$tmp"/archives/*.sit; do
        unar -q -f -o "$dest" "$archive" > /dev/null
    done
    echo "unpacked the four original apps into $dest"

# Check formatting and lints
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --features lumen/cli,halation/cli -- -D warnings

# Run all tests
test:
    cargo test --workspace --features lumen/cli,halation/cli

# Print a game's sections, strings and levels; write its sprite sheet to re/out/chromatron<game>/ (game: 1-4)
dump game:
    cargo run --quiet -p spectrograph -- dump {{game}} "{{justfile_directory()}}/re/out/chromatron{{game}}"

# Render a level to re/out/shots/ (game: 1-4; level: 1-50; solution: "stored" or a copied solution such as 1-01-hI)
shot game level solution="":
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{justfile_directory()}}/re/out/shots"
    mkdir -p "$out"
    if [ -n "{{solution}}" ]; then
        cargo run --quiet -p lumen --features cli -- shot {{game}} {{level}} "$out/chromatron{{game}}-level{{level}}-solved.png" "{{solution}}"
    else
        cargo run --quiet -p lumen --features cli -- shot {{game}} {{level}} "$out/chromatron{{game}}-level{{level}}.png"
    fi

# Render one level in the modern look to dist/shots-modern/ for review: optionally solved (stored, or a copied solution), at an easel (WxH@S), with the level sheet open (sheet=1) or on a touch layout (touch=1), in a theme (theme=light)
shot-modern game level solution="" easel="1280x800@2" sheet="" touch="" theme="":
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{justfile_directory()}}/dist/shots-modern"
    mkdir -p "$out"
    png="$out/chromatron{{game}}-level{{level}}{{ if solution == "" { "" } else { "-solved" } }}{{ if sheet == "" { "" } else { "-sheet" } }}{{ if touch == "" { "" } else { "-touch" } }}{{ if theme == "" { "" } else { "-" + theme } }}-{{easel}}.png"
    cargo run --quiet --release -p halation --features cli -- shot {{game}} {{level}} "$png" {{ if solution == "" { "" } else { quote(solution) } }} --easel {{easel}} {{ if sheet == "" { "" } else { "--sheet" } }} {{ if touch == "" { "" } else { "--touch" } }} {{ if theme == "" { "" } else { "--theme " + theme } }}

# Render frames of a cue in the modern look to dist/shots-motion/ for review: cue is turn:<cell>, move:<from>,<to>, lift:<from>,<to>, win, level or sheet (cells: 0–224 board, 225– tray); one PNG per time in `at` (ms after the cue); with `sheet`, from the level sheet open (so `sheet` closes it); in a theme (theme=light)
shot-motion game level cue at="0 50 100 200 400 800" solution="stored" easel="1280x800@2" touch="" sheet="" theme="":
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{justfile_directory()}}/dist/shots-motion"
    mkdir -p "$out"
    cargo build --quiet --release -p halation --features cli
    name="$(printf '%s' "{{cue}}" | tr ':,' '--'){{ if sheet == "" { "" } else { "-from-sheet" } }}"
    for t in {{at}}; do
        "{{justfile_directory()}}/target/release/halation" shot {{game}} {{level}} "$out/c{{game}}-l{{level}}-$name-${t}ms{{ if theme == "" { "" } else { "-" + theme } }}-{{easel}}.png" {{ if solution == "" { "" } else { quote(solution) } }} --easel {{easel}} --cue {{cue}} --at "$t" {{ if touch == "" { "" } else { "--touch" } }} {{ if sheet == "" { "" } else { "--sheet" } }} {{ if theme == "" { "" } else { "--theme " + theme } }}
    done

# Render the synthetic pieces sheet (every kind, colour and direction) in the modern look to dist/shots-modern/, in a theme (theme=light)
shot-pieces easel="1280x800@2" theme="":
    mkdir -p "{{justfile_directory()}}/dist/shots-modern"
    cargo run --quiet --release -p halation --features cli -- pieces "{{justfile_directory()}}/dist/shots-modern/pieces{{ if theme == "" { "" } else { "-" + theme } }}-{{easel}}" --easel {{easel}} {{ if theme == "" { "" } else { "--theme " + theme } }}

# Benchmark the engine (trace, classic render), the modern look (arrange, cold paint, drag) and the macOS captions on real levels of every game; fails if over the spec's budgets
bench:
    cargo bench -p lumen --bench engine
    cargo bench -p halation --bench darkroom
    cargo bench -p chromatron --bench captions

# Print the wasm-bindgen version locked in Cargo.lock; the wasm-bindgen CLI must be exactly this version
wasm-bindgen-version:
    @awk '$0 == "name = \"wasm-bindgen\"" { getline; gsub(/version = |"/, ""); print; exit }' "{{justfile_directory()}}/Cargo.lock"

# Build the browser version into dist/web/: one static page that plays all four games
web: _require-originals
    #!/usr/bin/env bash
    set -euo pipefail
    want="$("{{just_executable()}}" --justfile "{{justfile()}}" wasm-bindgen-version)"
    [[ -n "$want" ]] || { echo "error: no wasm-bindgen version found in Cargo.lock" >&2; exit 1; }
    have="$(wasm-bindgen --version 2> /dev/null | awk '{ print $2 }' || true)"
    if [[ "$have" != "$want" ]]; then
        echo "error: the wasm-bindgen CLI is ${have:-not installed}, Cargo.lock has $want; run: cargo install wasm-bindgen-cli --locked --version $want" >&2
        exit 1
    fi
    targets="$(rustup target list --installed)"
    grep -x wasm32-unknown-unknown <<< "$targets" > /dev/null \
        || { echo "error: the wasm32 target is missing; run: rustup target add wasm32-unknown-unknown" >&2; exit 1; }
    command -v wasm-opt > /dev/null || { echo "error: wasm-opt not found; run: just setup (binaryen)" >&2; exit 1; }
    wasm-opt --version
    # Wasm SIMD (Safari 16.4+, Chrome 91+, Firefox 89+) for halation's light planes.
    CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUSTFLAGS="-C target-feature=+simd128" \
        cargo build --quiet -p chromatron-web --profile web-release --target wasm32-unknown-unknown
    out="{{justfile_directory()}}/dist/web"
    rm -rf "$out"
    mkdir -p "$out"
    wasm-bindgen --target web --no-typescript --out-dir "$out" \
        "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/wasm32-unknown-unknown/web-release/chromatron_web.wasm"
    node "{{justfile_directory()}}/crates/chromatron-web/tests/wasm_ledger.mjs" "$out/chromatron_web_bg.wasm" halation
    # Every feature the build may use, all inside the browsers the page supports (spec §4).
    wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
        --enable-reference-types --enable-multivalue --enable-mutable-globals --enable-simd \
        -o "$out/chromatron_web_bg.opt.wasm" "$out/chromatron_web_bg.wasm"
    mv "$out/chromatron_web_bg.opt.wasm" "$out/chromatron_web_bg.wasm"
    cp "{{justfile_directory()}}"/web/* "$out/"
    # Every page URL carries a hash of all shipped files (wasm, glue, main.js, touch_layout.js, analytics.js, style.css, index.html template), so a browser never mixes files of two deploys.
    build="$(cd "$out" && cat chromatron_web_bg.wasm chromatron_web.js main.js touch_layout.js analytics.js style.css index.html | shasum -a 256 | cut -c 1-12)"
    perl -pi -e "s/__BUILD__/$build/g" "$out/index.html" "$out/main.js"
    for file in "$out"/*; do
        printf '%8d bytes %8d gzip  %s\n' "$(wc -c < "$file")" "$(gzip -9 -c "$file" | wc -c)" "$(basename "$file")"
    done

# Serve dist/web/ at http://127.0.0.1:<port>/ for playing in a browser (Ctrl-C stops it)
web-serve port="8000": web
    cd "{{justfile_directory()}}/dist/web" && python3 -m http.server {{port}} --bind 127.0.0.1

# Build dist/web/ and smoke-test it in Node: every game starts, every tests/solutions/ fixture wins and reloads, size budgets hold, the touch layout rules hold, the modern look paints within its time budgets and a live resize within its memory budget
web-check: web
    node "{{justfile_directory()}}/crates/chromatron-web/tests/web_check.mjs" "{{justfile_directory()}}/dist/web" "{{justfile_directory()}}/tests/solutions"
    node "{{justfile_directory()}}/crates/chromatron-web/tests/touch_layout_check.mjs" "{{justfile_directory()}}/dist/web"
    node "{{justfile_directory()}}/crates/chromatron-web/tests/lightbox_budgets.mjs" "{{justfile_directory()}}/dist/web" "{{justfile_directory()}}/tests/solutions" light-memory
    node "{{justfile_directory()}}/crates/chromatron-web/tests/lightbox_budgets.mjs" "{{justfile_directory()}}/dist/web" "{{justfile_directory()}}/tests/solutions"

# Build dist/web/ and photograph it in headless Google Chrome (both looks; phone portrait and landscape, small phones, desktop) into <dir> (relative to the repo; default dist/web-shots, git-ignored), checking each view; local only, needs Chrome
web-shots dir="dist/web-shots": web
    node "{{justfile_directory()}}/crates/chromatron-web/tests/web_shots.mjs" "{{justfile_directory()}}/dist/web" "{{join(justfile_directory(), dir)}}"

# Render the brand stills (og.png, apple-touch-icon.png, assets/promo/*: banner, portrait cover and three 1280 × 720 video thumbnails for A/B testing) with the trailer renderer; with <dir> (under dist/ or outside the repo), into it for review; local only
social-images dir="":
    node "{{justfile_directory()}}/trailer/reel.mjs" stills {{ if dir == "" { "" } else { quote(join(invocation_directory(), dir)) } }}

# Test the trailer's beam tracer, playback, timeline and lettering
trailer-test:
    node --test "{{justfile_directory()}}/trailer/"*.test.mjs

# Render the 30 s promo videos, landscape (1920 × 1080) and portrait (1080 × 1920), with the synthesised soundtrack, into dist/trailer/ in headless Google Chrome and ffmpeg, then check them with ffprobe and ebur128; local only, takes about 10 minutes
trailer: trailer-test
    #!/usr/bin/env bash
    set -euo pipefail
    work="$(mktemp -d "${TMPDIR:-/tmp}/chromatron-trailer.XXXXXX")"
    trap 'rm -rf "$work"' EXIT
    mkdir -p "{{justfile_directory()}}/dist/trailer"
    SECONDS=0
    node "{{justfile_directory()}}/trailer/score.mjs" "$work/score.wav"
    node "{{justfile_directory()}}/trailer/reel.mjs" video "{{justfile_directory()}}/dist/trailer" "$work/score.wav"
    echo "just trailer took $((SECONDS / 60)) min $((SECONDS % 60)) s"

# Re-render only the soundtrack and lay it onto the videos already in dist/trailer/ (the picture is copied), then check them again; seconds instead of `just trailer`'s minutes
trailer-score: trailer-test
    #!/usr/bin/env bash
    set -euo pipefail
    work="$(mktemp -d "${TMPDIR:-/tmp}/chromatron-trailer.XXXXXX")"
    trap 'rm -rf "$work"' EXIT
    node "{{justfile_directory()}}/trailer/score.mjs" "$work/score.wav"
    node "{{justfile_directory()}}/trailer/reel.mjs" remux "{{justfile_directory()}}/dist/trailer" "$work/score.wav"

# Render keyframe contact sheets of the trailer, one per shape, into <dir> (under dist/, e.g. dist/trailer/review, or outside the repo; relative to the current directory), for review
trailer-sheet dir:
    node "{{justfile_directory()}}/trailer/reel.mjs" sheet {{quote(join(invocation_directory(), dir))}}

# Run a game's app (game: 1-4)
run game:
    cargo run --release --bin chromatron{{game}}

# Write a game's snapshot to a PNG, then quit (game: 1-4; level: strip number 1-50; solution: `stored` or copied text such as 3-17-hODwhH, pasted first; look: modern, painted offscreen at size WxH@S with its captions, or classic, the window's first presented frame; theme: dark or light, the modern look's)
snapshot game png level="1" solution="" look="modern" size="1280x800@2" theme="dark":
    cargo build --release --quiet --bin chromatron{{game}}
    CHROMATRON_SNAPSHOT="{{join(invocation_directory(), png)}}" CHROMATRON_SNAPSHOT_LEVEL="{{level}}" CHROMATRON_SNAPSHOT_SOLUTION="{{solution}}" CHROMATRON_LOOK="{{look}}" CHROMATRON_SNAPSHOT_SIZE="{{ if look == "classic" { "" } else { size } }}" CHROMATRON_THEME="{{theme}}" perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 60 "{{justfile_directory()}}/target/release/chromatron{{game}}"
    @sips -g pixelWidth -g pixelHeight "{{join(invocation_directory(), png)}}" | tail -n 2

# Open a game's window and present the whole canvas `frames` times (switching look half way when `switch` is set), then print the present's p50/p90/max, the surfaces' memory and a read-back check, and quit (look: modern or classic; size: a modern window's WxH in pt, such as 2560x1400, else the default; damage: set to present only the stage's damage while the level's solution is pasted and reset, at the cues' own pace); never touches saves or preferences; local only, needs a display (spec §14 "Present")
present-probe game="1" look="modern" frames="240" switch="" size="" damage="":
    cargo build --release --quiet --bin chromatron{{game}}
    CHROMATRON_LOOK="{{look}}" CHROMATRON_PRESENT_PROBE="{{frames}}" CHROMATRON_PRESENT_PROBE_SWITCH="{{switch}}" CHROMATRON_PROBE_SIZE="{{size}}" CHROMATRON_PRESENT_PROBE_DAMAGE="{{damage}}" perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 60 "{{justfile_directory()}}/target/release/chromatron{{game}}"

# Regenerate the README's modern screenshots as 1280 × 800 JPEGs (the same solved level per game as the classic ones), through each app's offscreen modern snapshot, captions included; dir defaults to the repo's assets/screenshots, and a relative dir is taken from where just runs, as snapshot's png is
screenshots-modern dir=(justfile_directory() / "assets/screenshots"):
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{join(invocation_directory(), dir)}}"
    mkdir -p "$out"
    tmp="$(mktemp -d)"
    trap 'rm -rf "$tmp"' EXIT
    for shot in 1:46 2:22 3:41 4:21; do
        game="${shot%%:*}"
        cargo build --quiet --release --bin "chromatron$game"
        CHROMATRON_SNAPSHOT="$tmp/$game.png" CHROMATRON_SNAPSHOT_LEVEL="${shot##*:}" CHROMATRON_SNAPSHOT_SOLUTION=stored CHROMATRON_LOOK=modern CHROMATRON_SNAPSHOT_SIZE=1280x800@1 CHROMATRON_THEME=dark \
            perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 60 "{{justfile_directory()}}/target/release/chromatron$game"
        sips -s format jpeg -s formatOptions 90 "$tmp/$game.png" --out "$out/chromatron$game-modern.jpg" >/dev/null
    done

# Regenerate the README screenshots in assets/screenshots/: one solved level per game, in the modern look and the classic one
screenshots: \
    screenshots-modern \
    (snapshot "1" (justfile_directory() / "assets/screenshots/chromatron1.png") "46" "stored" "classic") \
    (snapshot "2" (justfile_directory() / "assets/screenshots/chromatron2.png") "22" "stored" "classic") \
    (snapshot "3" (justfile_directory() / "assets/screenshots/chromatron3.png") "41" "stored" "classic") \
    (snapshot "4" (justfile_directory() / "assets/screenshots/chromatron4.png") "21" "stored" "classic")

# Build the four apps in dist/ (Chromatron.app, Chromatron 2.app, …): Info.plist and icon from each original, ad-hoc signed
bundle: (_bundle "1") (_bundle "2") (_bundle "3") (_bundle "4")

# Build one game's app in dist/, named like its original (game: 1-4)
_bundle game: _require-originals
    #!/usr/bin/env bash
    set -euo pipefail
    name="Chromatron"
    [[ "{{game}}" == 1 ]] || name="Chromatron {{game}}"
    binary="chromatron{{game}}"
    cargo build --release --bin "$binary"
    original="{{originals}}/$name.app/Contents"
    app="{{justfile_directory()}}/dist/$name.app"
    plist="$app/Contents/Info.plist"
    from_original() { plutil -extract "$1" raw -o - "$original/Info.plist"; }
    set_string() { plutil -insert "$1" -string "$2" "$plist"; }
    rm -rf "$app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "{{justfile_directory()}}/target/release/$binary" "$app/Contents/MacOS/$binary"
    icon="$(from_original CFBundleIconFile)"
    cp "$original/Resources/$icon" "$app/Contents/Resources/$icon"
    plutil -create xml1 "$plist"
    set_string CFBundleDevelopmentRegion en
    set_string CFBundleExecutable "$binary"
    set_string CFBundleGetInfoString "$(from_original CFBundleGetInfoString)"
    set_string CFBundleIconFile "$icon"
    set_string CFBundleIdentifier "me.shengtuo.$binary"
    set_string CFBundleInfoDictionaryVersion 6.0
    set_string CFBundleName "$(from_original CFBundleName)"
    set_string CFBundlePackageType APPL
    set_string CFBundleShortVersionString "$(from_original CFBundleVersion)"
    set_string CFBundleVersion "$(from_original CFBundleVersion)"
    set_string LSApplicationCategoryType public.app-category.puzzle-games
    set_string LSMinimumSystemVersion 11.0
    set_string NSHumanReadableCopyright "$(from_original CFBundleGetInfoString)"
    plutil -insert NSHighResolutionCapable -bool YES "$plist"
    plutil -lint -s "$plist"
    codesign --force --sign - --timestamp=none "$app"
    codesign --verify --strict "$app"
    echo "built $app"

# Bundle a game, open it in a look (modern or classic), check it idles at ~0% CPU, quit it with an Apple Event and check that it saved (game: 1-4); the player's ClassicLook preference is put back afterwards
smoke game="1" look="modern": (_bundle game)
    #!/usr/bin/env bash
    set -euo pipefail
    [[ "{{look}}" == modern || "{{look}}" == classic ]] || { echo "error: look is modern or classic, not {{look}}" >&2; exit 1; }
    name="Chromatron"
    save_name="com.silverspaceship.chromatron.dat"
    if [[ "{{game}}" != 1 ]]; then
        name="Chromatron {{game}}"
        save_name="com.silverspaceship.chromatron{{game}}.dat"
    fi
    domain="me.shengtuo.chromatron{{game}}"
    binary="{{justfile_directory()}}/dist/$name.app/Contents/MacOS/chromatron{{game}}"
    save="$HOME/Library/Application Support/Chromatron {{game}}/$save_name"
    if pgrep -f -x "$binary" > /dev/null; then
        echo "error: Chromatron {{game}} from dist/ is already running; quit it first" >&2
        exit 1
    fi
    # The player's look, put back (or deleted, if it was never set) however the run ends.
    saved_look="$(defaults read "$domain" ClassicLook 2> /dev/null || echo absent)"
    restore_look() {
        case "$saved_look" in
            absent) defaults delete "$domain" ClassicLook 2> /dev/null || true ;;
            1) defaults write "$domain" ClassicLook -bool true ;;
            *) defaults write "$domain" ClassicLook -bool false ;;
        esac
    }
    trap 'pkill -KILL -f -x "$binary" 2> /dev/null || true; restore_look' EXIT
    defaults write "$domain" ClassicLook -bool {{ if look == "classic" { "true" } else { "false" } }}
    before="$(stat -f %m "$save" 2> /dev/null || echo none)"
    open -n "{{justfile_directory()}}/dist/$name.app"
    pid=""
    for _ in $(seq 40); do
        pid="$(pgrep -f -x "$binary" || true)"
        [[ -z "$pid" ]] || break
        sleep 0.25
    done
    [[ -n "$pid" ]] || { echo "error: Chromatron {{game}} did not start" >&2; exit 1; }
    sleep 2
    cpu="$(top -l 2 -s 3 -pid "$pid" -stats cpu | tail -n 1 | tr -d ' ')"
    echo "running as pid $pid ({{look}} look), idle CPU ${cpu}%"
    awk -v cpu="$cpu" 'BEGIN { exit !(cpu + 0 < 1.0) }' || { echo "error: idle CPU ${cpu}% is over the ~0% budget" >&2; exit 1; }
    perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 20 osascript -e 'tell application id "me.shengtuo.chromatron{{game}}" to quit'
    for _ in $(seq 20); do
        pgrep -f -x "$binary" > /dev/null || break
        sleep 0.25
    done
    if pgrep -f -x "$binary" > /dev/null; then
        echo "error: Chromatron {{game}} did not quit within 5 s" >&2
        exit 1
    fi
    after="$(stat -f %m "$save" 2> /dev/null || echo none)"
    [[ "$after" != none && "$after" != "$before" ]] || { echo "error: quitting did not write $save" >&2; exit 1; }
    echo "ok: launched in the {{look}} look, idled, quit and saved"

# All four games: bundling copies each original's Info.plist and icon
_require-originals:
    @for app in "Chromatron" "Chromatron 2" "Chromatron 3" "Chromatron 4"; do test -d "{{originals}}/$app.app" || { echo "error: $app.app not found — put all four original apps in chromatron_mac/" >&2; exit 1; }; done

# Decompile a game with Ghidra headless into re/out/chromatron<game>/ (game: 1-4)
decompile game:
    #!/usr/bin/env bash
    set -euo pipefail
    bin="$(cargo run --quiet -p spectrograph -- path {{game}})"
    [[ -f "$bin" ]] || { echo "error: $bin not found — put the original apps in chromatron_mac/" >&2; exit 1; }
    out="{{justfile_directory()}}/re/out/chromatron{{game}}"
    rm -rf "$out"
    mkdir -p "$out/ghidra"
    export JAVA_HOME="$(brew --prefix openjdk@21)/libexec/openjdk.jdk/Contents/Home"
    "$(brew --prefix ghidra)/libexec/support/analyzeHeadless" "$out/ghidra" chromatron{{game}} \
        -import "$bin" \
        -scriptPath "{{justfile_directory()}}/re/ghidra" \
        -postScript ApplyNames.java "{{justfile_directory()}}/re/notes/chromatron{{game}}" \
        -postScript ExportAll.java "$out" \
        2>&1 | tail -n 5
    echo "functions: $(($(wc -l < "$out/functions.tsv") - 1))  strings: $(($(wc -l < "$out/strings.tsv") - 1))"
