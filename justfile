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
    cargo clippy --workspace --all-targets --features lumen/cli -- -D warnings

# Run all tests
test:
    cargo test --workspace --features lumen/cli

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

# Benchmark trace and render on real levels of every game; fails if over the spec's budgets
bench:
    cargo bench -p lumen --bench engine

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
    cargo build --quiet -p chromatron-web --profile web-release --target wasm32-unknown-unknown
    out="{{justfile_directory()}}/dist/web"
    rm -rf "$out"
    mkdir -p "$out"
    wasm-bindgen --target web --no-typescript --out-dir "$out" \
        "${CARGO_TARGET_DIR:-{{justfile_directory()}}/target}/wasm32-unknown-unknown/web-release/chromatron_web.wasm"
    cp "{{justfile_directory()}}"/web/* "$out/"
    # Every page URL carries a hash of all shipped files (wasm, glue, main.js, touch_layout.js, style.css, index.html template), so a browser never mixes files of two deploys.
    build="$(cd "$out" && cat chromatron_web_bg.wasm chromatron_web.js main.js touch_layout.js style.css index.html | shasum -a 256 | cut -c 1-12)"
    perl -pi -e "s/__BUILD__/$build/g" "$out/index.html" "$out/main.js"
    for file in "$out"/*; do
        printf '%8d bytes %8d gzip  %s\n' "$(wc -c < "$file")" "$(gzip -9 -c "$file" | wc -c)" "$(basename "$file")"
    done

# Serve dist/web/ at http://127.0.0.1:<port>/ for playing in a browser (Ctrl-C stops it)
web-serve port="8000": web
    cd "{{justfile_directory()}}/dist/web" && python3 -m http.server {{port}} --bind 127.0.0.1

# Build dist/web/ and smoke-test it in Node: every game starts, every tests/solutions/ fixture wins and reloads, size budgets hold, the touch layout rules hold
web-check: web
    node "{{justfile_directory()}}/crates/chromatron-web/tests/web_check.mjs" "{{justfile_directory()}}/dist/web" "{{justfile_directory()}}/tests/solutions"
    node "{{justfile_directory()}}/crates/chromatron-web/tests/touch_layout_check.mjs" "{{justfile_directory()}}/dist/web"

# Build dist/web/ and photograph it in headless Google Chrome (phone portrait and landscape, desktop) into <dir> (relative to the repo; default re/out/web-shots, git-ignored), checking each view; local only, needs Chrome
web-shots dir="re/out/web-shots": web
    node "{{justfile_directory()}}/crates/chromatron-web/tests/web_shots.mjs" "{{justfile_directory()}}/dist/web" "{{join(justfile_directory(), dir)}}"

# Run a game's app (game: 1-4)
run game:
    cargo run --release --bin chromatron{{game}}

# Write a game's first presented frame, text included, to a PNG, then quit (game: 1-4; level: strip number 1-50; solution: `stored` or copied text such as 3-17-hODwhH, pasted before the frame)
snapshot game png level="1" solution="":
    cargo build --release --quiet --bin chromatron{{game}}
    CHROMATRON_SNAPSHOT="{{join(invocation_directory(), png)}}" CHROMATRON_SNAPSHOT_LEVEL="{{level}}" CHROMATRON_SNAPSHOT_SOLUTION="{{solution}}" perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 60 "{{justfile_directory()}}/target/release/chromatron{{game}}"
    @sips -g pixelWidth -g pixelHeight "{{join(invocation_directory(), png)}}" | tail -n 2

# Regenerate the README screenshots in assets/screenshots/: one solved level per game
screenshots: \
    (snapshot "1" (justfile_directory() / "assets/screenshots/chromatron1.png") "46" "stored") \
    (snapshot "2" (justfile_directory() / "assets/screenshots/chromatron2.png") "22" "stored") \
    (snapshot "3" (justfile_directory() / "assets/screenshots/chromatron3.png") "41" "stored") \
    (snapshot "4" (justfile_directory() / "assets/screenshots/chromatron4.png") "21" "stored")

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

# Bundle a game, open it, check it idles at ~0% CPU, quit it with an Apple Event and check that it saved (game: 1-4)
smoke game="1": (_bundle game)
    #!/usr/bin/env bash
    set -euo pipefail
    name="Chromatron"
    save_name="com.silverspaceship.chromatron.dat"
    if [[ "{{game}}" != 1 ]]; then
        name="Chromatron {{game}}"
        save_name="com.silverspaceship.chromatron{{game}}.dat"
    fi
    binary="{{justfile_directory()}}/dist/$name.app/Contents/MacOS/chromatron{{game}}"
    save="$HOME/Library/Application Support/Chromatron {{game}}/$save_name"
    if pgrep -f -x "$binary" > /dev/null; then
        echo "error: Chromatron {{game}} from dist/ is already running; quit it first" >&2
        exit 1
    fi
    trap 'pkill -KILL -f -x "$binary" 2> /dev/null || true' EXIT
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
    echo "running as pid $pid, idle CPU ${cpu}%"
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
    echo "ok: launched, idled, quit and saved"

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
