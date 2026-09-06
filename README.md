# duflow

Version-controlled flow graph of a software system, kept in [KDL](https://kdl.dev) files next to the code.
States, triggers, endpoints, checks, outcomes and variables live in `flows/*.kdl`; the graph is queried
by AI (token-efficient CLI, deterministic write-back) and by humans (single-file interactive UI:
walk, simulate, map, diff overlay).

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/duhanbalci/duflow/main/install.sh | sh
```

Installs to `~/.local/bin` (override with `DUFLOW_INSTALL_DIR`). Pin a version with `DUFLOW_VERSION=0.1.0`.
Prebuilt binaries: macOS (arm64, x86_64) and Linux (x86_64, arm64, static musl).

From source: `cargo install --git https://github.com/duhanbalci/duflow duflow-cli`

Update later with:

```sh
duflow self-update          # or --check
```

Shell completion (node IDs, vars, layers, git revs): `duflow completions fish|zsh|bash`.

## Usage

```sh
duflow validate                       # lint: dangling refs, unreachable nodes, undefined vars
duflow brief deploy.rolling           # one-shot summary of a node
duflow prereq deploy.done             # what must happen to reach a node
duflow path api.deploy deploy.done    # paths between nodes
duflow search healthy                 # fuzzy search
duflow add state deploy.done --layer domain --desc "Done" --child '-> "deploy.status"'
duflow edit deploy.done --set desc="..." --rm-edge deploy.status
duflow diff main                      # graph diff against a git rev
duflow ui serve                       # interactive UI on localhost:4646
duflow ui build -o duflow.html        # single-file static UI
```

Every query takes `--json`. `flows/` is searched upward from the cwd; pass `-d <dir>` otherwise.

Format and design notes: `docs/design.md`. AI usage guide: `.claude/skills/duflow/SKILL.md`.

## Release

```sh
just release 0.2.0
```

Bumps the workspace version, tags `v0.2.0` and pushes; GitHub Actions builds the binaries and publishes the release.

## License

MIT
