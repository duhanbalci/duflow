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
   duflow ls --kind call --no-attr method            # calls without method/path = fake calls (--attr k[=v] too)
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
   duflow edit <id> --rm-edge x --child '-> "x" case="ok"'   # removals run first, so re-adding with case= works
   duflow edit <id> --kind state                  # change kind, children kept (never rm + add)
   duflow edit <check|var|perm> --set reads=role  # definitions are editable too
   duflow rename <old> <new>                      # references + file move
   duflow rm <id> [--force]                       # node or definition; --force ALSO DELETES every reference (edges, check/perm default targets, reads)
   echo '[{"op":"add_child","id":"a","line":"-> \"b\""}]' | duflow apply - [--dry-run]   # batch; atomic, lists every failing op
   ```
   Writes take a per-directory lock; parallel agents queue instead of clobbering each other.
4. **Validate when done:** `duflow validate` must report zero errors (`--prefix <ns>` for your own namespace, `--summary [--depth 2]` for a code × namespace table). A new node with no incoming edge yields `unreachable`; connect it.
   Before a big edit batch, snapshot: `cp -r flows /tmp/flows-before`; afterwards `duflow diff /tmp/flows-before --stat` shows nodes/edges lost per namespace (works with git revs too: `duflow diff main --stat`).
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
one sentence why it is unaffected. Lint errors in files you changed block stopping (except `unreachable`, which is cross-file and transient while other agents write); any error blocks `git commit`.

## Format (KDL), cheatsheet

```kdl
state "deploy.rolling.retry" layer="domain" desc="Attempt counter increments" {
  sets "retry" "+1"                                          # dotless name = local counter, no `var` needed
  check "pool_has_ip" -> "deploy.rolling.pool_exhausted"     # checks are allowed in any node, not only call
  -> "deploy.rolling.create_instance" when="retry < 3"
  -> "deploy.rolling.rollback"        when="retry >= 3"
  -> "deploy.rolling.notify"          case="always"          # case= names a real fan-out branch (either/or, no guard)
  -> outcome="toast: too many attempts" when="retry > 5"     # target-less `->`: terminal text with a guard/case
  on "instance.failed" -> "deploy.rolling.retry"             # `on` may listen to an event or any node
}
state "cluster.purge" layer="domain" desc="..." {
  -> "cluster.purge.proxy" seq=1                             # seq= = steps that ALL run in order (not a branch)
  -> "cluster.purge.dns"   seq=2
  -> "cluster.purge.done"  seq=3
}
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" desc="..." src="dorch/src/api/deploys.rs#trigger_deploy" {
  requires "deploy.trigger"                                 # perm: fail code + target from the `perm` definition
  requires "project.view" fail=404 code="NAME_UNKNOWN"      # usage overrides deny code / body code (also -> / outcome=)
  check "has_success_build" fail=422 code="no_build" -> "ui.toast.no_build"   # usage `->` overrides the definition default
  check "service_not_frozen" fail=409 outcome="toast: service frozen"         # outcome= terminal text, no node
  returns 202 -> "deploy.queued"                            # internal call: returns ok -> "x"
  returns 200 case="noop" -> "ui.project.deploys"           # same status, several results → case=
  returns 500 outcome="toast: internal error"               # terminal, no node
}
call "api.admin.nodes.drain" layer="api" method="POST" path="/admin/nodes/{id}/drain" entry="cli" desc="..." { returns 200 -> "node.draining" }   # entry= : called by an external client (CLI/API), no UI, no fake root
action "ui.project.deploy_submit" layer="ui" kind="submit" desc="..." { calls "api.deploys.create" }
event  "instance.healthy" layer="domain" desc="..."         # external/async source; consumed via `on` only — no `->` out of an event
var   "deploy.attempts" type="int"                          # global var (dotted): written via sets, read across groups
var   "role" type="enum" source="session" values="admin member"   # external source
check "has_success_build" desc="..." -> "ui.toast.no_build"        # default fail target
check "service_not_frozen" desc="..." outcome="toast: service frozen"   # default terminal text; usages need no repeat
perm  "deploy.trigger" scope="project" deny=404 desc="..." outcome="toast: not found"   # (or -> "node")
root  "ui.login"                                            # entry point; every node must be reachable from a root
root  "network.liveness" every="30s"                        # periodic entry (reconciler / liveness loop)
view  "deploy_from_ui" from="ui.login" to="deploy.done"     # saved query, not data
```

Rules:
- Node kinds: `state`, `action` (click/submit/prompt; `calls`), `call` (endpoint if `method`/`path`, otherwise internal step; `returns`), `event` (external/async source only; a state transition is already an "event" you can `on`).
- ID `a-z0-9_` + dots; dot = hierarchy/group. File: `a.b` → `a.kdl`; `a.b.c…` → `a/b.kdl`. One definition per ID; other files reference by ID.
- Layer `layer="ui|api|domain"` (project extends in `flow.kdl`).
- Guard `when="..."` is a string; not interpreted. Dotted names must be defined `var`s; dotless names are local counters that only need a `sets` in the same scope (the ID prefix up to the last dot: `restore.snapshot` and `restore.applying` share `restore`). Declare a global `var` only for external sources or values read across scopes.
- Several `->` without a guard: either/or branches get `case="..."`; steps that all run in order get `seq=1`, `seq=2`, …; otherwise `ambiguous_transition`. Don't fake `case=` labels for sequential steps. Duplicate `case`/`seq` on one node warns; mixing `seq=` and `case=` on one node warns (`mixed_branching`): put the branching on the last step's node.
- Toasts/logs/dead ends are not states: use `outcome="..."` on the check, `returns`, or a target-less `-> outcome="..." when="..."`. Put the common text once on the `check`/`perm` definition; a usage only overrides.
- Events: `event` nodes have no outgoing `->`; consumers use `on`. Model the delivery chain (emit → SSE → UI refresh) once in its own infrastructure nodes, never as direct event → ui edges.
- Endpoints with no UI (admin/CLI): `entry="cli"` on the call instead of a fake state or root.
- Permissions: define `perm`, use `requires`. Don't write `check "perm:x"` by hand.
- Reconcilers and periodic loops are `root ... every="..."`, not edges from a boot state. Periodic roots don't count toward `max_roots` (set in `flow.kdl`).
- Only state-changing things go in; spinners/cosmetics don't. Everything that must be checked is a `check`.
- `desc` is one line, in the project's language. Code/IDs are English. `src="path#symbol"` links the node to code; `validate` checks the file and the symbol as a whole word (any language, incl. `.vue`/`.ts`; `Type::method` matches when both parts appear). A code file without `#symbol` or `:line` warns `src_symbol_unchecked`.
- Don't invent roots: a node with incoming edges is not an entry point (`root_has_incoming`).

## Common lint errors
`dangling_ref` target missing · `unreachable` no path from a root/entry · `unknown_var` undefined dotted name in a guard · `local_var_never_set` local counter never `sets` in its scope · `var_never_set` read but no `sets`/`source` · `call_no_returns` · `id_path_mismatch` wrong file · `unknown_check` · `unknown_perm` · `ambiguous_transition` add `case=`/`seq=`/`when=` · `duplicate_case`/`duplicate_seq`/`mixed_branching` · `event_has_transition` · `root_has_incoming` · `too_many_roots` · `src_missing`/`src_symbol_missing`/`src_symbol_unchecked`.
