# duflow geliştirme komutları

build: ui-build
    cargo build --release -p duflow-cli

test:
    cargo test --workspace

check:
    cargo check --workspace && cd ui && bun run vue-tsc -b

# Vite dev server; ui/public/duflow.json'u yükler
ui-dev:
    cd ui && bun run dev

# Tek dosya UI → crates/duflow-cli/ui-dist/index.html
ui-build:
    cd ui && bun install --silent && bun run build

# Örnek export'u Duploy flows'undan tazele (ui-dev için)
ui-data dir="../duploy/flows":
    cargo run -q -p duflow-cli -- -d {{dir}} export > ui/public/duflow.json

# Duploy flows'u ile hızlı deneme
try dir="../duploy/flows":
    cargo run -q -p duflow-cli -- -d {{dir}} validate
    cargo run -q -p duflow-cli -- -d {{dir}} brief deploy.rolling.wait_healthy

# Duploy flows'u için UI sunucusu
serve dir="../duploy/flows":
    cargo run -q -p duflow-cli -- -d {{dir}} ui serve

install: build
    cp target/release/duflow ~/.cargo/bin/duflow

# Sürüm çıkar: Cargo.toml bump + commit + tag + push; release workflow gerisini yapar
release version:
    #!/usr/bin/env sh
    set -eu
    v="${{version}}"; v="${v#v}"
    [ -z "$(git status --porcelain)" ] || { echo "working tree dirty" >&2; exit 1; }
    sed -i '' "s/^version = \".*\"/version = \"$v\"/" Cargo.toml
    cargo update -w -q
    cargo build -q -p duflow-cli
    git add Cargo.toml Cargo.lock
    git commit -qm "chore: release v$v"
    git tag "v$v"
    git push origin main "v$v"
    echo "pushed v$v; see https://github.com/duhanbalci/duflow/actions"
