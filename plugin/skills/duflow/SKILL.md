---
name: duflow
description: Read, query and edit the system flow graph (flows/*.kdl) with duflow. Use when the user asks about a flow/step/prerequisite, says "add/update this in flows", or when you need context before implementing a feature. Never edit the files by hand; go through the CLI.
---

# duflow

The system's flow graph lives in `flows/` as KDL files. It is not code but a **reflection** of it:
states, triggers, endpoints, checks, possible outcomes, variables. It changes in the same PR as the code.

## Workflow

1. **Get context, don't read files.** Ask for a node summary:
   ```bash
   duflow brief <id>            # how to get here, incoming/outgoing, checks, vars, file:line
   duflow prereq <id>           # steps, checks and vars required to reach this node
   duflow path <a> <b> [--all]  # path(s) between two nodes
   duflow search <text>         # fuzzy over ID/desc/check/var
   duflow var <var.id>          # who sets it, who reads it
   duflow perm [<id>]           # permissions; with an id: which endpoints require it
   duflow ls --prefix deploy --prefix api.deploys   # list (--kind, --layer, --prefix repeatable)
   ```
   Every command takes `--json`. If `flows/` is not found, pass `-d <dir>`.
2. **Update the graph after writing code** (code-first) or **write the graph first, then implement** (spec-first).
3. **Edit through the CLI**, never open the file by hand (formatting and reference consistency live in the CLI):
   ```bash
   duflow add state deploy.done --layer domain --desc "Deploy success" --child '-> "deploy.status"'
   duflow add call api.x.create --layer api --attr method=POST --attr path=/api/x \
     --child 'check "perm:x.create" fail=404' --child 'returns 202 -> "x.queued"'
   duflow add var x.count --attr type=int         # var/check/root/perm also via `add`
   duflow add perm deploy.trigger --attr scope=project --attr deny=404 --desc "..."
   duflow add root network.liveness --attr every=30s   # periodic entry (reconciler), not a fake edge
   duflow edit <id> --set desc="..." --child 'on "ev" -> "x"' --rm-edge <target> --rm-child sets:x.count
   duflow edit <id> --kind state                  # change kind, children kept (never rm + add)
   duflow edit <check|var|perm> --set reads=role  # definitions are editable too
   duflow rename <old> <new>                      # references + file move
   duflow rm <id> [--force]                       # node or definition; --force ALSO DELETES every reference to it
   echo '[{"op":"add_child","id":"a","line":"-> \"b\""}]' | duflow apply - [--dry-run]   # batch; atomic, lists every failing op
   ```
   Writes take a per-directory lock; parallel agents queue instead of clobbering each other.
4. **Validate when done:** `duflow validate` must report zero errors (`--prefix <ns>` for your own namespace, `--summary` for a code × namespace table). A new node with no incoming edge yields `unreachable`; connect it.
5. To show the user: `duflow ui build -o duflow.html` (single file) or `duflow ui serve`.
   PR diff: `duflow diff main` (text) / `duflow ui build --diff main`.

Shell autocomplete (node IDs, vars, layers, git revs): `duflow completions fish|zsh|bash` prints the one-liner to add.

Install: `curl -fsSL https://raw.githubusercontent.com/duhanbalci/duflow/main/install.sh | sh`. Update: `duflow self-update`.
Claude Code: `claude plugin marketplace add duhanbalci/duflow && claude plugin install duflow@duflow` (this skill + hooks).
Other agents (Codex, Cursor, ...): `duflow skill install [--project]` writes this file from the binary.

## Keeping the graph in sync (plugin hooks)

`flows/flow.kdl` can list source globs whose changes usually affect the graph:

```kdl
project "duploy" {
  layers "ui" "api" "domain"
  watch "dorch/src/api/**" "dorch/src/deploy/**" "dorch/ui/src/views/**"
}
```

With the plugin installed, editing a watched file adds a one-time reminder; trying to stop (or `git commit`)
after changing watched files without touching `flows/` is blocked until the graph is updated or you state in
one sentence why it is unaffected. Lint errors in `flows/` block stopping and committing.

## Format (KDL), cheatsheet

```kdl
state "deploy.rolling.retry" layer="domain" desc="Attempt counter increments" {
  sets "retry" "+1"                                          # dotless name = local counter, no `var` needed
  check "pool_has_ip" -> "deploy.rolling.pool_exhausted"     # checks are allowed in any node, not only call
  -> "deploy.rolling.create_instance" when="retry < 3"
  -> "deploy.rolling.rollback"        when="retry >= 3"
  -> "deploy.rolling.notify"          case="always"          # case= names a real fan-out branch (no guard)
  on "instance.failed" -> "deploy.rolling.retry"             # `on` may listen to an event or any node
}
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" desc="..." src="dorch/src/api/deploys.rs#trigger_deploy" {
  requires "deploy.trigger"                                 # perm: fail code + target from the `perm` definition
  check "has_success_build" fail=422 code="no_build" -> "ui.toast.no_build"   # usage `->` overrides the definition default
  check "service_not_frozen" fail=409 outcome="toast: service frozen"         # outcome= terminal text, no node
  returns 202 -> "deploy.queued"                            # internal call: returns ok -> "x"
  returns 200 case="noop" -> "ui.project.deploys"           # same status, several results → case=
  returns 500 outcome="toast: internal error"               # terminal, no node
}
action "ui.project.deploy_submit" layer="ui" kind="submit" desc="..." { calls "api.deploys.create" }
event  "instance.healthy" layer="domain" desc="..."         # external/async source; listened to via `on`
var   "deploy.attempts" type="int"                          # global var (dotted): written via sets, read across groups
var   "role" type="enum" source="session" values="admin member"   # external source
check "has_success_build" desc="..." -> "ui.toast.no_build"        # default fail target
perm  "deploy.trigger" scope="project" deny=404 desc="..." -> "ui.toast.not_found"
root  "ui.login"                                            # entry point; every node must be reachable from a root
root  "network.liveness" every="30s"                        # periodic entry (reconciler / liveness loop)
view  "deploy_from_ui" from="ui.login" to="deploy.done"     # saved query, not data
```

Rules:
- Node kinds: `state`, `action` (click/submit/prompt; `calls`), `call` (endpoint if `method`/`path`, otherwise internal step; `returns`), `event` (external/async source only; a state transition is already an "event" you can `on`).
- ID `a-z0-9_` + dots; dot = hierarchy/group. File: `a.b` → `a.kdl`; `a.b.c…` → `a/b.kdl`. One definition per ID; other files reference by ID.
- Layer `layer="ui|api|domain"` (project extends in `flow.kdl`).
- Guard `when="..."` is a string; not interpreted. Dotted names must be defined `var`s; dotless names are local counters that only need a `sets` in the same group. Declare a global `var` only for external sources or values read across groups.
- Fan-out without a guard: label each branch with `case="..."`, otherwise `ambiguous_transition`.
- Toasts/logs/dead ends are not states: use `outcome="..."` on the check or `returns`.
- Permissions: define `perm`, use `requires`. Don't write `check "perm:x"` by hand.
- Reconcilers and periodic loops are `root ... every="..."`, not edges from a boot state.
- Only state-changing things go in; spinners/cosmetics don't. Everything that must be checked is a `check`.
- `desc` is one line, in the project's language. Code/IDs are English. `src="path#symbol"` links the node to code; `validate` checks it.
- Don't invent roots: a node with incoming edges is not an entry point (`root_has_incoming`).

## Common lint errors
`dangling_ref` target missing · `unreachable` no path from a root · `unknown_var` undefined dotted name in a guard · `local_var_never_set` local counter never `sets` in its group · `var_never_set` read but no `sets`/`source` · `call_no_returns` · `id_path_mismatch` wrong file · `unknown_check` · `unknown_perm` · `ambiguous_transition` add `case=`/`when=` · `root_has_incoming` · `src_missing`.
