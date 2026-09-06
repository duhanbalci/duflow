# duflow

A version-controlled flow graph of your software system, written by hand (human or AI) in
[KDL](https://kdl.dev) files that live next to the code. States, triggers, endpoints, checks,
outcomes and variables go in `flows/*.kdl`; duflow lints the graph, answers questions about it
from the CLI (token-efficient, `--json` everywhere), edits it deterministically, and renders an
interactive single-file UI (walk, simulate, map, diff overlay).

```kdl
// flows/deploy/rolling.kdl
state "deploy.rolling.wait_healthy" layer="domain" {
  desc "Wait until the new instance is healthy"
  on "instance.healthy" -> "deploy.rolling.route_switch"
  on "instance.failed"  -> "deploy.rolling.retry"
  on "timeout"          -> "deploy.rolling.retry"
}

state "deploy.rolling.retry" layer="domain" {
  sets "deploy.attempts" "+1"
  -> "deploy.rolling.create_instance" when="deploy.attempts < 3"
  -> "deploy.rolling.rollback"        when="deploy.attempts >= 3"
}
```

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/duhanbalci/duflow/main/install.sh | sh
```

Installs a prebuilt binary to `~/.local/bin` (macOS arm64/x86_64, Linux x86_64/arm64 static).
`DUFLOW_INSTALL_DIR` changes the directory, `DUFLOW_VERSION=0.1.0` pins a version.
Or build from source: `cargo install --git https://github.com/duhanbalci/duflow duflow-cli`.

```sh
duflow self-update               # upgrade to the latest release (--check only reports)
duflow completions fish|zsh|bash # shell autocomplete for node IDs, vars, layers, git revs
```

## Usage

```sh
duflow validate                       # dangling refs, unreachable nodes, undefined vars
duflow brief deploy.rolling.retry     # one-shot summary: how to get here, in/out edges, checks, vars
duflow prereq deploy.done             # steps, checks and vars needed to reach a node
duflow path api.deploy deploy.done    # shortest (or --all) paths between two nodes
duflow search healthy                 # fuzzy search over IDs, descriptions, checks, vars
duflow var deploy.attempts            # who sets it, who reads it
duflow ls --layer api                 # list nodes (--kind, --layer, --prefix)

duflow add state deploy.done --layer domain --desc "Done" --child '-> "deploy.status"'
duflow edit deploy.done --set desc="..." --child 'on "ev" -> "x"' --rm-edge deploy.status
duflow rename deploy.done deploy.finished
duflow rm deploy.finished [--force]
echo '[{"op":"add_child","id":"a","line":"-> \"b\""}]' | duflow apply -   # batch edits

duflow diff main                      # graph diff against a git rev
duflow ui serve                       # interactive UI on localhost:4646
duflow ui build -o duflow.html --diff main   # single-file static UI with diff overlay
```

`flows/` is searched upward from the current directory; pass `-d <dir>` otherwise.

Format and design: [`docs/design.md`](docs/design.md). Guide for AI agents:
[`.claude/skills/duflow/SKILL.md`](.claude/skills/duflow/SKILL.md).

## License

MIT
