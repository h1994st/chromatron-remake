set shell := ["bash", "-euo", "pipefail", "-c"]

originals := env("CHROMATRON_ORIGINALS", justfile_directory() / "chromatron_mac")

# List recipes
default:
    @just --list

# Install Homebrew dependencies from the Brewfile
setup:
    brew bundle

# Check formatting and lints
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --features lumen/cli -- -D warnings

# Run all tests (needs the original apps)
test: _require-originals
    cargo test --workspace --features lumen/cli

# Print a game's sections, strings and levels; write its sprite sheet to re/out/chromatron<game>/ (game: 1-4)
dump game: _require-originals
    cargo run --quiet -p spectrograph -- dump {{game}} "{{justfile_directory()}}/re/out/chromatron{{game}}"

# Render a level to re/out/shots/ (level: 1-50; solution: "stored" or a copied solution such as 1-01-hI)
shot game level solution="": _require-originals
    #!/usr/bin/env bash
    set -euo pipefail
    out="{{justfile_directory()}}/re/out/shots"
    mkdir -p "$out"
    if [ -n "{{solution}}" ]; then
        cargo run --quiet -p lumen --features cli -- shot {{game}} {{level}} "$out/chromatron{{game}}-level{{level}}-solved.png" "{{solution}}"
    else
        cargo run --quiet -p lumen --features cli -- shot {{game}} {{level}} "$out/chromatron{{game}}-level{{level}}.png"
    fi

# Benchmark trace and render on real levels; fails if over the spec's budgets
bench: _require-originals
    cargo bench -p lumen --bench engine

# Run a game's app (game: 1)
run game: _require-originals
    cargo run --release --bin chromatron{{game}}

# Write a game's first presented frame, text included, to a PNG, then quit (game: 1)
snapshot game png: _require-originals
    cargo build --release --quiet --bin chromatron{{game}}
    CHROMATRON_SNAPSHOT="{{join(invocation_directory(), png)}}" perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 60 "{{justfile_directory()}}/target/release/chromatron{{game}}"
    @sips -g pixelWidth -g pixelHeight "{{join(invocation_directory(), png)}}" | tail -n 2

# Build dist/Chromatron.app: release binary, Info.plist and icon from the original, ad-hoc signed
bundle: _require-originals
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release --bin chromatron1
    original="{{originals}}/Chromatron.app/Contents"
    app="{{justfile_directory()}}/dist/Chromatron.app"
    plist="$app/Contents/Info.plist"
    from_original() { plutil -extract "$1" raw -o - "$original/Info.plist"; }
    set_string() { plutil -insert "$1" -string "$2" "$plist"; }
    rm -rf "$app"
    mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
    cp "{{justfile_directory()}}/target/release/chromatron1" "$app/Contents/MacOS/chromatron1"
    icon="$(from_original CFBundleIconFile)"
    cp "$original/Resources/$icon" "$app/Contents/Resources/$icon"
    plutil -create xml1 "$plist"
    set_string CFBundleDevelopmentRegion en
    set_string CFBundleExecutable chromatron1
    set_string CFBundleGetInfoString "$(from_original CFBundleGetInfoString)"
    set_string CFBundleIconFile "$icon"
    set_string CFBundleIdentifier me.shengtuo.chromatron1
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

# Bundle, open the app, check it idles at ~0% CPU, quit it with an Apple Event and check that it saved
smoke: bundle
    #!/usr/bin/env bash
    set -euo pipefail
    binary="{{justfile_directory()}}/dist/Chromatron.app/Contents/MacOS/chromatron1"
    save="$HOME/Library/Application Support/Chromatron 1/com.silverspaceship.chromatron.dat"
    if pgrep -f -x "$binary" > /dev/null; then
        echo "error: Chromatron 1 from dist/ is already running; quit it first" >&2
        exit 1
    fi
    trap 'pkill -KILL -f -x "$binary" 2> /dev/null || true' EXIT
    before="$(stat -f %m "$save" 2> /dev/null || echo none)"
    open -n "{{justfile_directory()}}/dist/Chromatron.app"
    pid=""
    for _ in $(seq 40); do
        pid="$(pgrep -f -x "$binary" || true)"
        [[ -z "$pid" ]] || break
        sleep 0.25
    done
    [[ -n "$pid" ]] || { echo "error: Chromatron 1 did not start" >&2; exit 1; }
    sleep 2
    cpu="$(top -l 2 -s 3 -pid "$pid" -stats cpu | tail -n 1 | tr -d ' ')"
    echo "running as pid $pid, idle CPU ${cpu}%"
    awk -v cpu="$cpu" 'BEGIN { exit !(cpu + 0 < 1.0) }' || { echo "error: idle CPU ${cpu}% is over the ~0% budget" >&2; exit 1; }
    perl -e 'alarm shift; exec @ARGV or die "exec: $!"' 20 osascript -e 'tell application id "me.shengtuo.chromatron1" to quit'
    for _ in $(seq 20); do
        pgrep -f -x "$binary" > /dev/null || break
        sleep 0.25
    done
    if pgrep -f -x "$binary" > /dev/null; then
        echo "error: Chromatron 1 did not quit within 5 s" >&2
        exit 1
    fi
    after="$(stat -f %m "$save" 2> /dev/null || echo none)"
    [[ "$after" != none && "$after" != "$before" ]] || { echo "error: quitting did not write $save" >&2; exit 1; }
    echo "ok: launched, idled, quit and saved"

_require-originals:
    @test -d "{{originals}}/Chromatron.app" || { echo "error: originals not found — put the original apps in chromatron_mac/" >&2; exit 1; }

# Decompile a game with Ghidra headless into re/out/chromatron<game>/ (game: 1-4)
decompile game: _require-originals
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
