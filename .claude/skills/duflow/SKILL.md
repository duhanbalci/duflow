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
   duflow ls --prefix deploy    # list (--kind, --layer)
   ```
   Every command takes `--json`. If `flows/` is not found, pass `-d <dir>`.
2. **Update the graph after writing code** (code-first) or **write the graph first, then implement** (spec-first).
3. **Edit through the CLI**, never open the file by hand (formatting and reference consistency live in the CLI):
   ```bash
   duflow add state deploy.done --layer domain --desc "Deploy success" --child '-> "deploy.status"'
   duflow add call api.x.create --layer api --attr method=POST --attr path=/api/x \
     --child 'check "perm:x.create" fail=404' --child 'returns 202 -> "x.queued"'
   duflow add var x.count --attr type=int         # var/check/root also via `add`
   duflow edit <id> --set desc="..." --child 'on "ev" -> "x"' --rm-edge <target> --rm-child sets:x.count
   duflow rename <old> <new>                      # references + file move
   duflow rm <id> [--force]
   echo '[{"op":"add_child","id":"a","line":"-> \"b\""}]' | duflow apply - [--dry-run]   # batch (JSON op list)
   ```
4. **Validate when done:** `duflow validate` must report zero errors. A new node with no incoming edge yields `unreachable`; connect it.
5. To show the user: `duflow ui build -o duflow.html` (single file) or `duflow ui serve`.
   PR diff: `duflow diff main` (text) / `duflow ui build --diff main`.

Shell autocomplete (node IDs, vars, layers, git revs): `duflow completions fish|zsh|bash` prints the one-liner to add.

## Format (KDL), cheatsheet

```kdl
state "deploy.rolling.retry" layer="domain" desc="Attempt counter increments" {
  sets "deploy.attempts" "+1"
  -> "deploy.rolling.create_instance" when="deploy.attempts < 3"
  -> "deploy.rolling.rollback"        when="deploy.attempts >= 3"
  on "instance.failed" -> "deploy.rolling.retry"
}
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" desc="..." {
  check "perm:deploy.trigger" fail=404                      # fail target comes from the check definition
  check "has_success_build" fail=422 code="no_build" -> "ui.toast.no_build"
  returns 202 -> "deploy.queued"                            # internal call: returns ok -> "x"
}
action "ui.project.deploy_submit" layer="ui" kind="submit" desc="..." { calls "api.deploys.create" }
event  "instance.healthy" layer="domain" desc="..."         # listened to via `on`
var   "deploy.attempts" type="int"                          # written via sets
var   "role" type="enum" source="session" values="admin member"   # external source
check "perm:deploy.trigger" desc="..." reads="role" -> "ui.toast.not_found"
root  "ui.login"                                            # entry point; every node must be reachable from a root
view  "deploy_from_ui" from="ui.login" to="deploy.done"     # saved query, not data
```

Rules:
- Node kinds: `state`, `action` (click/submit/prompt; `calls`), `call` (endpoint/RPC; `check` + `returns`), `event` (async).
- ID `a-z0-9_` + dots; dot = hierarchy/group. File: `a.b` → `a.kdl` or `a/b.kdl`; `a.b.c…` → `a/b.kdl`. One definition per ID; other files reference by ID.
- Layer `layer="ui|api|domain"` (project extends in `flow.kdl`).
- Guard `when="..."` is a string; not interpreted, but the names inside must be defined `var`s.
- Only state-changing things go in; spinners/cosmetics don't. Everything that must be checked is a `check`.
- `desc` is one line, in the project's language. Code/IDs are English.

## Common lint errors
`dangling_ref` target missing · `unreachable` no path from a root · `unknown_var` undefined name in a guard · `var_never_set` read but no `sets`/`source` · `call_no_returns` · `id_path_mismatch` wrong file · `unknown_check`.
