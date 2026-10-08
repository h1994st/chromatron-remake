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
    cargo clippy --workspace --all-targets -- -D warnings

# Run all tests (needs the original apps)
test: _require-originals
    cargo test --workspace

# Print a game's sections and strings (game: 1-4)
dump game: _require-originals
    cargo run --quiet -p spectrograph -- dump {{game}}

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
