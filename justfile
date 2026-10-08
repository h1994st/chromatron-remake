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
