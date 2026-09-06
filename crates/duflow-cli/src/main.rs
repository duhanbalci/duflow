//! `duflow` CLI. Tüm sorgular `--json` alır; insan çıktısı markdown/metin.

use anyhow::{Context, Result, bail};
use clap::CommandFactory as _;
use clap::{Args, Parser, Subcommand};
use clap_complete::engine::{ArgValueCompleter, CompletionCandidate};
use duflow_core::Graph;
use duflow_core::diff::diff;
use duflow_core::edit::{Op, Workspace};
use duflow_core::export;
use duflow_core::lint::{Level, lint};
use duflow_core::query;
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

mod hook;
mod ui;

#[derive(Parser)]
#[command(
    name = "duflow",
    version,
    about = "System flow graph: KDL files, queries, editing, UI"
)]
struct Cli {
    /// flows directory (default: ./flows, searched upward)
    #[arg(short = 'd', long, global = true)]
    dir: Option<PathBuf>,
    /// JSON output
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lint: dangling refs, unreachable nodes, undefined vars... (exit 1 on errors)
    Validate {
        /// Only diagnostics whose node ID (or file) starts with this; repeatable
        #[arg(long, add = ArgValueCompleter::new(complete_node))]
        prefix: Vec<String>,
        /// Table of code × namespace instead of the flat list
        #[arg(long)]
        summary: bool,
        /// Namespace depth for --summary columns (1 = first ID segment)
        #[arg(long, default_value_t = 1)]
        depth: usize,
        /// Skip the `src=` attribute check against the repository
        #[arg(long)]
        no_src: bool,
    },
    /// One-shot summary of a node, for AI
    Brief {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        id: String,
        /// Depth (ancestor list)
        #[arg(long, default_value_t = 1)]
        depth: usize,
    },
    /// Steps, checks and variables needed to reach a node
    Prereq {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        id: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// Paths between two nodes
    Path {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        from: String,
        #[arg(add = ArgValueCompleter::new(complete_node))]
        to: String,
        /// All simple paths instead of the shortest (up to --max)
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 5)]
        max: usize,
        #[arg(long, default_value_t = 30)]
        depth: usize,
    },
    /// Set of nodes reachable forward from a node
    Reach {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        id: String,
    },
    /// Who sets and who reads a variable
    Var {
        #[arg(add = ArgValueCompleter::new(complete_var))]
        id: String,
    },
    /// Fuzzy search over ID/desc/check/var
    Search {
        query: String,
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
    /// List nodes (filters: --kind, --layer, --prefix, --attr, --no-attr; all repeatable except kind/layer)
    Ls {
        #[arg(long, value_parser = ["state", "action", "call", "event"])]
        kind: Option<String>,
        #[arg(long, add = ArgValueCompleter::new(complete_layer))]
        layer: Option<String>,
        #[arg(long, add = ArgValueCompleter::new(complete_node))]
        prefix: Vec<String>,
        /// Only nodes that have this attribute (`method`, or `method=POST`)
        #[arg(long = "attr")]
        with_attr: Vec<String>,
        /// Only nodes that lack this attribute (e.g. `--kind call --no-attr method` finds fake calls)
        #[arg(long = "no-attr")]
        without_attr: Vec<String>,
    },
    /// Permissions: list all, or who requires one (`duflow perm deploy.trigger`)
    Perm { id: Option<String> },
    /// New node or definition: duflow add state deploy.done --layer domain --desc "..." --child '-> "x"'
    Add(AddArgs),
    /// Edit a node or definition. Removals (--rm-edge, --rm-child) run BEFORE additions (--kind, --set, --child),
    /// so "delete edge, re-add with case=" works in one command
    Edit(EditArgs),
    /// Rename an ID (all references + file move)
    Rename {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        from: String,
        to: String,
    },
    /// Remove a node or definition (var/check/root/perm). --force also deletes every reference to it
    Rm {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        id: String,
        #[arg(long)]
        force: bool,
    },
    /// Apply a JSON op list from stdin (`-`) or a file. Atomic: any failing op → nothing is written,
    /// all failures listed (--continue-on-error writes the successful ones). `--help` shows the op schema
    #[command(after_long_help = APPLY_HELP)]
    Apply {
        #[arg(default_value = "-")]
        file: String,
        /// Preview without writing; lists every failing op
        #[arg(long)]
        dry_run: bool,
        /// Write the ops that succeeded even if some failed
        #[arg(long)]
        continue_on_error: bool,
    },
    /// Graph diff between two git revs or flows directories (working tree if the second is omitted)
    Diff {
        /// Git rev, or a path to a flows directory (works when flows/ is gitignored: copy it first)
        #[arg(add = ArgValueCompleter::new(complete_rev))]
        rev1: String,
        #[arg(add = ArgValueCompleter::new(complete_rev))]
        rev2: Option<String>,
        /// Counts only: nodes/edges added/removed per namespace
        #[arg(long)]
        stat: bool,
    },
    /// Print the graph in the UI JSON schema
    Export {
        /// Base rev for the diff overlay
        #[arg(long, add = ArgValueCompleter::new(complete_rev))]
        diff: Option<String>,
    },
    /// UI
    #[command(subcommand)]
    Ui(UiCmd),
    /// Install shell autocomplete (fish | zsh | bash); --print only shows the line
    Completions {
        #[arg(value_parser = ["fish", "zsh", "bash"])]
        shell: String,
        #[arg(long)]
        print: bool,
    },
    /// AI agent skill (SKILL.md): install for Claude Code / Codex / .agents, or print
    #[command(subcommand)]
    Skill(SkillCmd),
    /// Claude Code hook entry point (used by the duflow plugin; reads hook JSON on stdin)
    #[command(hide = true)]
    Hook {
        #[arg(value_enum)]
        event: hook::Event,
    },
    /// Update duflow to the latest GitHub release (--check only reports)
    SelfUpdate {
        /// Only check, do not install
        #[arg(long)]
        check: bool,
    },
}

/// `duflow apply --help` kuyruğu: op şeması + örnek. `edit::Op` ile birlikte güncellenir.
const APPLY_HELP: &str = r#"OPS (JSON array; "op" selects the shape, IDs are strings):
  {"op":"add_node","kind":"state|action|call|event","id":"a.b","attrs":{"layer":"domain","desc":"..."},"children":["-> "x" case="ok"","returns 200 -> "y""]}
  {"op":"set_attr","id":"a.b","key":"desc","value":"..."}          empty value removes the attribute
  {"op":"set_kind","id":"a.b","kind":"call"}
  {"op":"add_child","id":"a.b","line":"check "has_ip" fail=409 outcome="toast: no ip""}
  {"op":"rm_edge","id":"a.b","to":"a.c"}                           every child line targeting a.c (->, on, returns, calls, check ->)
  {"op":"rm_child","id":"a.b","name":"sets","arg":"retry"}          child by name + first argument
  {"op":"rename","from":"a.b","to":"a.c"}
  {"op":"rm_node","id":"a.b","force":true}                          node or definition; force also deletes references
  {"op":"add_var","id":"deploy.attempts","attrs":{"type":"int"}}
  {"op":"add_check","id":"has_ip","attrs":{"desc":"...","outcome":"toast: no ip"}}
  {"op":"add_perm","id":"deploy.trigger","attrs":{"scope":"project","deny":"404"}}
  {"op":"add_root","id":"network.liveness","attrs":{"every":"30s"}}

EXAMPLE
  printf '%s' '[{"op":"add_child","id":"a.b","line":"-> "a.c" seq=1"},{"op":"rm_edge","id":"a.b","to":"a.d"}]' | duflow apply - --dry-run
"#;

/// Release'lerin yayınlandığı GitHub deposu; asset adı `duflow-v<ver>-<target>.tar.gz`.
const REPO_OWNER: &str = "duhanbalci";
const REPO_NAME: &str = "duflow";

#[derive(Subcommand)]
enum SkillCmd {
    /// Write SKILL.md to ~/.agents/skills and ~/.claude/skills (--project: ./.agents, ./.claude)
    Install {
        /// Into the current project instead of the home directory (commit it with the repo)
        #[arg(long)]
        project: bool,
    },
    /// Print SKILL.md to stdout
    Print,
}

/// Binary'ye gömülü skill; kaynak repo'daki `plugin/skills/duflow/SKILL.md`
const SKILL_MD: &str = include_str!("../../../plugin/skills/duflow/SKILL.md");

#[derive(Subcommand)]
enum UiCmd {
    /// Build a single-file static site
    Build {
        #[arg(short, long, default_value = "duflow.html")]
        out: PathBuf,
        /// Diff overlay: `main` or `a..b`
        #[arg(long, add = ArgValueCompleter::new(complete_rev))]
        diff: Option<String>,
    },
    /// Local server; rebuilds on every request (F5 is enough)
    Serve {
        #[arg(long, default_value = "127.0.0.1:4646")]
        addr: String,
    },
}

#[derive(Args)]
struct AddArgs {
    /// state | action | call | event | var | check | root | perm
    #[arg(value_parser = ["state", "action", "call", "event", "var", "check", "root", "perm"])]
    kind: String,
    id: String,
    #[arg(long, add = ArgValueCompleter::new(complete_layer))]
    layer: Option<String>,
    #[arg(long)]
    desc: Option<String>,
    /// Extra attribute: --attr method=POST --attr path=/x
    #[arg(long = "attr")]
    attrs: Vec<String>,
    /// Child line (raw KDL), repeatable
    #[arg(long = "child", allow_hyphen_values = true)]
    children: Vec<String>,
}

#[derive(Args)]
struct EditArgs {
    #[arg(add = ArgValueCompleter::new(complete_node))]
    id: String,
    #[arg(long = "set", allow_hyphen_values = true)]
    sets: Vec<String>,
    /// Change the node kind (state|action|call|event); children are kept
    #[arg(long, value_parser = ["state", "action", "call", "event"])]
    kind: Option<String>,
    #[arg(long = "child", allow_hyphen_values = true)]
    children: Vec<String>,
    #[arg(long = "rm-edge", add = ArgValueCompleter::new(complete_node))]
    rm_edges: Vec<String>,
    /// `name:arg`, e.g. `check:perm:x` or `sets:deploy.attempts`
    #[arg(long = "rm-child")]
    rm_children: Vec<String>,
}

fn main() {
    // Shell autocomplete: `COMPLETE=fish duflow | source` (bkz. `duflow completions`)
    clap_complete::CompleteEnv::with_factory(Cli::command).complete();
    if let Err(e) = run() {
        eprintln!("error: {e:#}");
        std::process::exit(2);
    }
}

fn find_dir(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(d) = explicit {
        return Ok(d);
    }
    let mut cur = std::env::current_dir()?;
    loop {
        let c = cur.join("flows");
        if c.is_dir() {
            return Ok(c);
        }
        if !cur.pop() {
            bail!("`flows/` directory not found (pass --dir)");
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let json = cli.json;
    // flows/ gerektirmeyen komutlar
    match cli.cmd {
        Cmd::SelfUpdate { check } => return self_update(check),
        Cmd::Skill(SkillCmd::Print) => {
            print!("{SKILL_MD}");
            return Ok(());
        }
        Cmd::Skill(SkillCmd::Install { project }) => return skill_install(project),
        Cmd::Hook { event } => return hook::run(event, cli.dir.clone()),
        _ => {}
    }
    let dir = match &cli.cmd {
        Cmd::Completions { .. } => PathBuf::new(),
        _ => find_dir(cli.dir.clone())?,
    };
    match cli.cmd {
        Cmd::SelfUpdate { .. } | Cmd::Skill(_) | Cmd::Hook { .. } => unreachable!(),
        Cmd::Validate {
            prefix,
            summary,
            depth,
            no_src,
        } => {
            let g = Graph::load(&dir)?;
            update_hint();
            let mut d = lint(&g);
            if !no_src {
                d.extend(duflow_core::lint::src_lint(&g, &repo_root(&dir)));
            }
            if !prefix.is_empty() {
                d.retain(|x| {
                    prefix.iter().any(|p| {
                        x.id.as_deref().is_some_and(|i| i.starts_with(p.as_str()))
                            || x.file.starts_with(p.as_str())
                    })
                });
            }
            let e = d.iter().filter(|x| x.level == Level::Error).count();
            let w = d.len() - e;
            if json {
                println!("{}", serde_json::to_string_pretty(&d)?);
            } else if summary {
                print!("{}", lint_summary(&d, depth.max(1)));
                println!("{e} errors, {w} warnings");
            } else {
                for x in &d {
                    println!("{x}");
                }
                println!(
                    "{} nodes, {} vars, {} checks, {} perms, {} roots · {e} errors, {w} warnings",
                    g.nodes.len(),
                    g.vars.len(),
                    g.checks.len(),
                    g.perms.len(),
                    g.roots.len()
                );
            }
            if e > 0 {
                std::process::exit(1);
            }
        }
        Cmd::Perm { id } => {
            let g = Graph::load(&dir)?;
            match id {
                Some(id) => {
                    let p = g
                        .perms
                        .get(&id)
                        .with_context(|| format!("perm `{id}` not found"))?;
                    let users: Vec<&duflow_core::model::Node> = g.perm_users(&id);
                    if json {
                        println!(
                            "{}",
                            serde_json::json!({"perm": p, "required_by": users.iter().map(|n| &n.id).collect::<Vec<_>>()})
                        );
                    } else {
                        println!(
                            "# perm {} · scope {} · deny {}",
                            p.id,
                            p.scope.as_deref().unwrap_or("-"),
                            p.deny.map(|d| d.to_string()).unwrap_or_else(|| "-".into())
                        );
                        if let Some(d) = &p.desc {
                            println!("{d}");
                        }
                        if let Some(t) = &p.fail_to {
                            println!("fail → {t}");
                        }
                        println!("\n## Required by");
                        for n in users {
                            println!(
                                "- {} {} {}",
                                n.id,
                                n.attrs.get("method").cloned().unwrap_or_default(),
                                n.attrs.get("path").cloned().unwrap_or_default()
                            );
                        }
                    }
                }
                None => {
                    if json {
                        println!("{}", serde_json::to_string_pretty(&g.perms)?);
                    } else {
                        for p in g.perms.values() {
                            println!(
                                "{:<24} {:<8} {:<4} {:<20} {:>2} uses  {}",
                                p.id,
                                p.scope.as_deref().unwrap_or("-"),
                                p.deny.map(|d| d.to_string()).unwrap_or_else(|| "-".into()),
                                p.fail_to.as_deref().unwrap_or("-"),
                                g.perm_users(&p.id).len(),
                                p.desc.clone().unwrap_or_default()
                            );
                        }
                    }
                }
            }
        }
        Cmd::Brief { id, depth } => {
            let g = Graph::load(&dir)?;
            let b = query::brief(&g, &id).with_context(|| format!("`{id}` not found"))?;
            let _ = depth;
            if json {
                println!("{}", serde_json::to_string_pretty(&b)?);
            } else {
                print!("{}", b.to_markdown(&g));
            }
        }
        Cmd::Prereq { id, depth } => {
            let g = Graph::load(&dir)?;
            let p = query::prereq(&g, &id, depth).with_context(|| format!("`{id}` not found"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&p)?);
            } else {
                print!("{}", p.to_markdown());
            }
        }
        Cmd::Path {
            from,
            to,
            all,
            max,
            depth,
        } => {
            let g = Graph::load(&dir)?;
            for x in [&from, &to] {
                if !g.nodes.contains_key(x) {
                    bail!("`{x}` not found");
                }
            }
            let paths: Vec<Vec<String>> = if all {
                g.all_paths(&from, &to, max, depth)
            } else {
                g.shortest_path(&from, &to).into_iter().collect()
            };
            if json {
                println!("{}", serde_json::to_string_pretty(&paths)?);
            } else if paths.is_empty() {
                println!("no path");
                std::process::exit(1);
            } else {
                for p in &paths {
                    println!("{} steps: {}", p.len() - 1, p.join(" → "));
                }
            }
        }
        Cmd::Reach { id } => {
            let g = Graph::load(&dir)?;
            if !g.nodes.contains_key(&id) {
                bail!("`{id}` not found");
            }
            let r = g.reach_forward(&[&id]);
            if json {
                println!("{}", serde_json::to_string_pretty(&r)?);
            } else {
                for x in &r {
                    println!("{x}");
                }
            }
        }
        Cmd::Var { id } => {
            let g = Graph::load(&dir)?;
            let v = g
                .vars
                .get(&id)
                .with_context(|| format!("var `{id}` not found"))?;
            let writers: Vec<String> = g
                .var_writers(&id)
                .into_iter()
                .map(|(n, s)| format!("{} {} ({}:{})", n.id, s.value, n.file, s.line))
                .collect();
            let readers: Vec<String> = g
                .var_readers(&id)
                .into_iter()
                .map(|(n, w)| format!("{n} [{w}]"))
                .collect();
            if json {
                println!(
                    "{}",
                    serde_json::json!({"var": v, "writers": writers, "readers": readers})
                );
            } else {
                println!(
                    "# {} ({}){}",
                    v.id,
                    v.ty,
                    v.source
                        .as_ref()
                        .map(|s| format!(" · source {s}"))
                        .unwrap_or_default()
                );
                println!(
                    "\n## Set by\n{}",
                    if writers.is_empty() {
                        "(none)".into()
                    } else {
                        writers.join("\n")
                    }
                );
                println!(
                    "\n## Read by\n{}",
                    if readers.is_empty() {
                        "(none)".into()
                    } else {
                        readers.join("\n")
                    }
                );
            }
        }
        Cmd::Search { query: q, limit } => {
            let g = Graph::load(&dir)?;
            let hits = query::search(&g, &q, limit);
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else {
                for h in &hits {
                    println!("{:<7} {}  {}", h.kind, h.id, h.desc);
                }
            }
        }
        Cmd::Ls {
            kind,
            layer,
            prefix,
            with_attr,
            without_attr,
        } => {
            let g = Graph::load(&dir)?;
            let list: Vec<_> = g
                .nodes
                .values()
                .filter(|n| kind.as_deref().is_none_or(|k| n.kind.as_str() == k))
                .filter(|n| layer.as_deref().is_none_or(|l| n.layer == l))
                .filter(|n| {
                    prefix.is_empty() || prefix.iter().any(|p| n.id.starts_with(p.as_str()))
                })
                .filter(|n| with_attr.iter().all(|a| attr_matches(n, a)))
                .filter(|n| !without_attr.iter().any(|a| attr_matches(n, a)))
                .collect();
            if json {
                println!("{}", serde_json::to_string_pretty(&list)?);
            } else {
                for n in list {
                    println!(
                        "{:<7} {:<7} {}  {}",
                        n.kind.as_str(),
                        n.layer,
                        n.id,
                        n.desc.clone().unwrap_or_default()
                    );
                }
            }
        }
        Cmd::Add(a) => {
            let mut attrs = BTreeMap::new();
            if let Some(l) = a.layer {
                attrs.insert("layer".into(), l);
            }
            if let Some(d) = a.desc {
                attrs.insert("desc".into(), d);
            }
            for kv in a.attrs {
                let (k, v) = kv
                    .split_once('=')
                    .with_context(|| format!("--attr `{kv}`: expected key=value"))?;
                attrs.insert(k.into(), v.into());
            }
            let op = match a.kind.as_str() {
                "var" => Op::AddVar { id: a.id, attrs },
                "check" => Op::AddCheck { id: a.id, attrs },
                "perm" => Op::AddPerm { id: a.id, attrs },
                "root" => Op::AddRoot { id: a.id, attrs },
                k => Op::AddNode {
                    kind: k.into(),
                    id: a.id,
                    attrs,
                    children: a.children,
                },
            };
            apply_ops(&dir, &[op], false, json)?;
        }
        Cmd::Edit(e) => {
            let ops = edit_ops(e)?;
            apply_ops(&dir, &ops, false, json)?;
        }
        Cmd::Rename { from, to } => apply_ops(&dir, &[Op::Rename { from, to }], false, json)?,
        Cmd::Rm { id, force } => apply_ops(&dir, &[Op::RmNode { id, force }], false, json)?,
        Cmd::Apply {
            file,
            dry_run,
            continue_on_error,
        } => {
            let src = if file == "-" {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                std::fs::read_to_string(&file)?
            };
            let ops: Vec<Op> =
                serde_json::from_str(&src).context("could not parse JSON op list")?;
            apply_ops_ext(&dir, &ops, dry_run, continue_on_error, json)?;
        }
        Cmd::Diff { rev1, rev2, stat } => {
            let a = graph_at_or_dir(&dir, &rev1)?;
            let b = match rev2 {
                Some(r) => graph_at_or_dir(&dir, &r)?,
                None => Graph::load(&dir)?,
            };
            let d = diff(&a, &b);
            if stat {
                let st = d.stat();
                if json {
                    println!("{}", serde_json::to_string_pretty(&st)?);
                } else {
                    print!("{}", st.to_text());
                }
            } else if json {
                println!("{}", serde_json::to_string_pretty(&d)?);
            } else {
                print!("{}", d.to_text());
            }
        }
        Cmd::Export { diff: base } => {
            let g = Graph::load(&dir)?;
            let s = export_json(&dir, &g, base.as_deref())?;
            println!("{s}");
        }
        Cmd::Ui(UiCmd::Build { out, diff: base }) => {
            let g = Graph::load(&dir)?;
            let data = export_json(&dir, &g, base.as_deref())?;
            let html = ui::render(&data);
            std::fs::write(&out, html)?;
            eprintln!("wrote {} ({} nodes)", out.display(), g.nodes.len());
        }
        Cmd::Ui(UiCmd::Serve { addr }) => ui::serve(&dir, &addr)?,
        Cmd::Completions { shell, print } => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .context("HOME not set")?;
            let (line, file) = match shell.as_str() {
                "fish" => (
                    "COMPLETE=fish duflow | source",
                    home.join(".config/fish/completions/duflow.fish"),
                ),
                "zsh" => ("source <(COMPLETE=zsh duflow)", home.join(".zshrc")),
                _ => ("source <(COMPLETE=bash duflow)", home.join(".bashrc")),
            };
            if print {
                println!("# add to {}:\n{line}", file.display());
                return Ok(());
            }
            let existing = std::fs::read_to_string(&file).unwrap_or_default();
            if existing.lines().any(|l| l.trim() == line) {
                println!("already installed in {}", file.display());
            } else if shell == "fish" {
                std::fs::create_dir_all(file.parent().unwrap())?;
                std::fs::write(&file, format!("{line}\n"))?;
                println!("wrote {}", file.display());
            } else {
                let sep = if existing.is_empty() || existing.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                std::fs::write(
                    &file,
                    format!("{existing}{sep}\n# duflow autocomplete\n{line}\n"),
                )?;
                println!("appended to {} (open a new shell)", file.display());
            }
        }
    }
    Ok(())
}

/// `duflow edit` argümanları → op listesi. Silmeler eklemelerden ÖNCE: aynı komutta
/// "kenarı sil, case= ile geri ekle" yapılabilsin (aksi halde ekleme siliniyordu).
fn edit_ops(e: EditArgs) -> Result<Vec<Op>> {
    let mut ops = vec![];
    for t in e.rm_edges {
        ops.push(Op::RmEdge {
            id: e.id.clone(),
            to: t,
        });
    }
    for rc in e.rm_children {
        let (name, arg) = rc
            .split_once(':')
            .with_context(|| format!("--rm-child `{rc}`: expected name:arg"))?;
        ops.push(Op::RmChild {
            id: e.id.clone(),
            name: name.into(),
            arg: arg.into(),
        });
    }
    if let Some(kind) = e.kind {
        ops.push(Op::SetKind {
            id: e.id.clone(),
            kind,
        });
    }
    for kv in e.sets {
        let (k, v) = kv
            .split_once('=')
            .with_context(|| format!("--set `{kv}`: expected key=value"))?;
        ops.push(Op::SetAttr {
            id: e.id.clone(),
            key: k.into(),
            value: v.into(),
        });
    }
    for c in e.children {
        ops.push(Op::AddChild {
            id: e.id.clone(),
            line: c,
        });
    }
    if ops.is_empty() {
        bail!("no operations given");
    }
    Ok(ops)
}

/// `--attr key` ya da `--attr key=value` eşleşmesi.
fn attr_matches(n: &duflow_core::model::Node, spec: &str) -> bool {
    match spec.split_once('=') {
        Some((k, v)) => n.attrs.get(k).is_some_and(|x| x == v),
        None => n.attrs.contains_key(spec),
    }
}

/// Rev bir dizinse oradan yükle (gitignore'lu flows kopyası), değilse git rev.
fn graph_at_or_dir(dir: &Path, rev: &str) -> Result<Graph> {
    let p = Path::new(rev);
    if p.is_dir() {
        return Ok(Graph::load(p)?);
    }
    graph_at(dir, rev)
}

fn apply_ops(dir: &Path, ops: &[Op], dry_run: bool, json: bool) -> Result<()> {
    apply_ops_ext(dir, ops, dry_run, false, json)
}

/// Op'ları uygular. Varsayılan atomik: bir op bile düşerse hiçbir şey yazılmaz, tüm düşenler
/// listelenir (`--dry-run` da aynı listeyi verir). `continue_on_error`: geçenler yazılır.
fn apply_ops_ext(
    dir: &Path,
    ops: &[Op],
    dry_run: bool,
    continue_on_error: bool,
    json: bool,
) -> Result<()> {
    let mut ws = Workspace::open(dir)?;
    let errs = ws.apply_all(ops);
    if !errs.is_empty() && (!continue_on_error || dry_run) {
        let list: Vec<String> = errs
            .iter()
            .map(|(i, e)| format!("op {i} ({}): {e}", op_name(&ops[*i])))
            .collect();
        if json {
            println!(
                "{}",
                serde_json::json!({"written": [], "failed": errs.iter().map(|(i, e)| serde_json::json!({"op": i, "error": e.to_string()})).collect::<Vec<_>>()})
            );
        }
        if !continue_on_error {
            bail!(
                "{} of {} ops failed, nothing written:\n  {}",
                errs.len(),
                ops.len(),
                list.join("\n  ")
            );
        }
        for l in &list {
            eprintln!("skipped {l}");
        }
    }
    if dry_run {
        for (f, content) in ws.preview() {
            println!("--- {f}\n{content}");
        }
        return Ok(());
    }
    let written = ws.commit()?;
    // yazdıktan sonra lint: hata varsa uyar ama geri alma (kullanıcı görsün)
    let g = Graph::load(dir)?;
    let d = lint(&g);
    let errors: Vec<_> = d.iter().filter(|x| x.level == Level::Error).collect();
    if json {
        println!(
            "{}",
            serde_json::json!({"written": written, "errors": errors})
        );
    } else {
        for f in &written {
            println!("wrote: {f}");
        }
        for e in &errors {
            println!("{e}");
        }
    }
    Ok(())
}

fn op_name(op: &Op) -> String {
    serde_json::to_value(op)
        .ok()
        .and_then(|v| v.get("op").and_then(|o| o.as_str()).map(String::from))
        .unwrap_or_default()
}

/// Kategori × namespace (ID'nin ilk `depth` segmenti, yoksa dosya) tablosu.
fn lint_summary(d: &[duflow_core::lint::Diagnostic], depth: usize) -> String {
    use std::collections::{BTreeMap, BTreeSet};
    let ns_of = |x: &duflow_core::lint::Diagnostic| -> String {
        x.id.as_deref()
            .map(|i| i.split('.').take(depth).collect::<Vec<_>>().join("."))
            .unwrap_or_else(|| {
                x.file
                    .split('/')
                    .next()
                    .unwrap_or(&x.file)
                    .trim_end_matches(".kdl")
                    .to_string()
            })
    };
    let mut table: BTreeMap<&str, BTreeMap<String, usize>> = BTreeMap::new();
    let mut spaces: BTreeSet<String> = BTreeSet::new();
    for x in d {
        let ns = ns_of(x);
        spaces.insert(ns.clone());
        *table.entry(x.code).or_default().entry(ns).or_default() += 1;
    }
    let spaces: Vec<String> = spaces.into_iter().collect();
    let w = spaces.iter().map(|n| n.len()).max().unwrap_or(0).clamp(8, 16);
    let mut s = format!("{:<22}", "code");
    for ns in &spaces {
        s.push_str(&format!(" {:>w$}", if ns.len() > w { &ns[..w] } else { ns }));
    }
    s.push_str("    total\n");
    for (code, row) in &table {
        s.push_str(&format!("{code:<22}"));
        let mut total = 0;
        for ns in &spaces {
            let n = row.get(ns).copied().unwrap_or(0);
            total += n;
            s.push_str(&format!(
                " {:>w$}",
                if n == 0 {
                    "·".to_string()
                } else {
                    n.to_string()
                }
            ));
        }
        s.push_str(&format!(" {total:>8}\n"));
    }
    s
}

/// `src=` attr'ları için repo kökü: git toplevel, yoksa flows'un üst dizini.
fn repo_root(dir: &Path) -> PathBuf {
    git(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .map(|s| PathBuf::from(s.trim()))
        .or_else(|| dir.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Bir git rev'indeki `flows/` dizininden graf.
pub fn graph_at(dir: &Path, rev: &str) -> Result<Graph> {
    let repo_root = git(dir, &["rev-parse", "--show-toplevel"])?
        .trim()
        .to_string();
    let rel = dir
        .canonicalize()?
        .strip_prefix(Path::new(&repo_root).canonicalize()?)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default();
    let list = git(dir, &["ls-tree", "-r", "--name-only", rev, "--", &rel])?;
    let mut sources = vec![];
    for path in list.lines().filter(|l| l.ends_with(".kdl")) {
        let src = git(dir, &["show", &format!("{rev}:{path}")])?;
        let inner = path
            .strip_prefix(&format!("{rel}/"))
            .unwrap_or(path)
            .to_string();
        sources.push((inner, src));
    }
    Ok(Graph::from_sources(&sources)?)
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .context("could not run git")?;
    if !out.status.success() {
        bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// `base` verilirse diff overlay'li export. `a..b` biçimi de kabul.
pub fn export_json(dir: &Path, g: &Graph, base: Option<&str>) -> Result<String> {
    let d;
    let old;
    let mut exp = match base {
        Some(spec) => {
            let (r1, r2) = spec
                .split_once("..")
                .map(|(a, b)| (a, Some(b)))
                .unwrap_or((spec, None));
            old = graph_at(dir, r1)?;
            let newer;
            let new_ref = match r2 {
                Some(r) => {
                    newer = graph_at(dir, r)?;
                    &newer
                }
                None => g,
            };
            d = diff(&old, new_ref);
            let mut e = export::export(new_ref, Some(&d));
            export::fill_removed_sources(&mut e, &old);
            return Ok(serde_json::to_string(&e)?);
        }
        None => export::export(g, None),
    };
    exp.diff = None;
    Ok(serde_json::to_string(&exp)?)
}

// --- Autocomplete ---
// Shell'den her Tab'da binary çağrılır; grafı o an yükleyip ID'leri döneriz. `-d` görülmez,
// cwd'den (ya da DUFLOW_DIR) bulunur. Graf yüklenemezse sessizce boş liste.

fn complete_graph() -> Option<Graph> {
    let dir = std::env::var_os("DUFLOW_DIR").map(PathBuf::from);
    find_dir(dir).ok().and_then(|d| Graph::load(&d).ok())
}

fn cand(id: &str, help: Option<&str>) -> CompletionCandidate {
    CompletionCandidate::new(id).help(help.map(|h| h.to_string().into()))
}

fn complete_node(current: &OsStr) -> Vec<CompletionCandidate> {
    let cur = current.to_string_lossy();
    let Some(g) = complete_graph() else {
        return vec![];
    };
    g.nodes
        .values()
        .filter(|n| n.id.starts_with(cur.as_ref()))
        .map(|n| cand(&n.id, n.desc.as_deref()))
        .collect()
}

fn complete_var(current: &OsStr) -> Vec<CompletionCandidate> {
    let cur = current.to_string_lossy();
    let Some(g) = complete_graph() else {
        return vec![];
    };
    g.vars
        .values()
        .filter(|v| v.id.starts_with(cur.as_ref()))
        .map(|v| cand(&v.id, Some(&v.ty)))
        .collect()
}

fn complete_layer(current: &OsStr) -> Vec<CompletionCandidate> {
    let cur = current.to_string_lossy();
    let layers = complete_graph()
        .map(|g| g.config.layers.clone())
        .unwrap_or_else(|| vec!["ui".into(), "api".into(), "domain".into()]);
    layers
        .iter()
        .filter(|l| l.starts_with(cur.as_ref()))
        .map(|l| cand(l, None))
        .collect()
}

fn complete_rev(current: &OsStr) -> Vec<CompletionCandidate> {
    let cur = current.to_string_lossy();
    let Ok(dir) = find_dir(std::env::var_os("DUFLOW_DIR").map(PathBuf::from)) else {
        return vec![];
    };
    let Ok(out) = git(
        &dir,
        &[
            "for-each-ref",
            "--format=%(refname:short)",
            "refs/heads",
            "refs/tags",
        ],
    ) else {
        return vec![];
    };
    let mut v: Vec<CompletionCandidate> = out
        .lines()
        .filter(|l| l.starts_with(cur.as_ref()))
        .map(|l| cand(l, None))
        .collect();
    if "HEAD".starts_with(cur.as_ref()) {
        v.push(cand("HEAD", None));
    }
    v
}

/// `validate` başında günde bir kez GitHub'a bakar; yeni sürüm varsa stderr'e tek satır.
/// Sonuç `$TMPDIR/duflow-update-check` dosyasında (zaman + son sürüm) saklanır; ağ hatası sessiz.
/// `DUFLOW_NO_UPDATE_CHECK=1` kapatır. Eski binary sessizce eski lint koşmasın diye.
fn update_hint() {
    if std::env::var_os("DUFLOW_NO_UPDATE_CHECK").is_some() {
        return;
    }
    let current = env!("CARGO_PKG_VERSION");
    let cache = std::env::temp_dir().join("duflow-update-check");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let cached = std::fs::read_to_string(&cache).ok().and_then(|s| {
        let (t, v) = s.trim().split_once(' ')?;
        Some((t.parse::<u64>().ok()?, v.to_string()))
    });
    let latest = match cached {
        Some((t, v)) if now.saturating_sub(t) < 24 * 3600 => v,
        _ => {
            let fetched = std::thread::spawn(fetch_latest_version)
                .join()
                .ok()
                .flatten()
                .unwrap_or_else(|| current.to_string());
            let _ = std::fs::write(&cache, format!("{now} {fetched}"));
            fetched
        }
    };
    if version_newer(&latest, current) {
        eprintln!("note: duflow {latest} is available (you run {current}; lint rules may be outdated) — run `duflow self-update`");
    }
}

fn fetch_latest_version() -> Option<String> {
    use self_update::backends::github::Update;
    let u = Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("duflow")
        .current_version(env!("CARGO_PKG_VERSION"))
        .build()
        .ok()?;
    let r = u.get_latest_release().ok()?;
    Some(r.latest()?.version().trim_start_matches('v').to_string())
}

/// `a` sürümü `b`'den yeni mi (sayısal parça karşılaştırması).
fn version_newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.trim_start_matches('v')
            .split('.')
            .map(|p| p.parse().unwrap_or(0))
            .collect()
    };
    parse(a) > parse(b)
}

/// GitHub Releases'tan güncelle. `check`: sadece sürüm karşılaştır.
fn self_update(check: bool) -> Result<()> {
    use self_update::backends::github::Update;
    let current = env!("CARGO_PKG_VERSION");
    let mut b = Update::configure();
    b.repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("duflow")
        // release.yml: tar.gz içinde `duflow-v<ver>-<target>/duflow`
        .bin_path_in_archive("duflow-v{{ version }}-{{ target }}/duflow")
        .current_version(current)
        .show_output(false)
        .no_confirm(true)
        .show_download_progress(true);
    let u = b.build().context("configure updater")?;
    let releases = u.get_latest_release().context("fetch latest release")?;
    if !releases.is_update_available().context("compare versions")? {
        println!("duflow {current} is up to date");
        return Ok(());
    }
    if check {
        let latest = releases.latest().map(|r| r.version()).unwrap_or("?");
        println!("duflow {current} -> {latest} available (run `duflow self-update`)");
        return Ok(());
    }
    let status = u.update().context("update")?;
    if status.is_updated() {
        println!("updated duflow {current} -> {}", status.version());
    } else {
        println!("duflow {current} is up to date");
    }
    Ok(())
}

/// SKILL.md'yi agent skill dizinlerine yaz. `.agents/skills` ortak convention (Codex, Cursor,
/// Cline...), `.claude/skills` Claude Code'un native yolu; ikisine de yazılır.
fn skill_install(project: bool) -> Result<()> {
    let base = if project {
        std::env::current_dir()?
    } else {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .context("HOME not set")?
    };
    for d in [".agents/skills/duflow", ".claude/skills/duflow"] {
        let dir = base.join(d);
        std::fs::create_dir_all(&dir)?;
        let f = dir.join("SKILL.md");
        let same = std::fs::read_to_string(&f).is_ok_and(|s| s == SKILL_MD);
        if !same {
            std::fs::write(&f, SKILL_MD)?;
        }
        println!(
            "{} {}",
            if same { "up to date" } else { "wrote" },
            f.display()
        );
    }
    if project {
        println!("commit these so teammates and CI agents get the skill too");
    }
    println!("Claude Code users: prefer the plugin (skill + sync hooks):");
    println!(
        "  claude plugin marketplace add {REPO_OWNER}/{REPO_NAME} && claude plugin install duflow@duflow"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_compare_is_numeric() {
        assert!(version_newer("0.5.10", "0.5.2"));
        assert!(!version_newer("0.5.2", "0.5.2"));
        assert!(!version_newer("v0.4.9", "0.5.0"));
    }

    #[test]
    fn edit_removals_come_before_additions() {
        let e = EditArgs {
            id: "a.b".into(),
            sets: vec!["desc=x".into()],
            kind: Some("state".into()),
            children: vec!["-> \"a.c\" case=\"go\"".into()],
            rm_edges: vec!["a.c".into()],
            rm_children: vec!["sets:n".into()],
        };
        let names: Vec<String> = edit_ops(e).unwrap().iter().map(op_name).collect();
        assert_eq!(
            names,
            vec!["rm_edge", "rm_child", "set_kind", "set_attr", "add_child"]
        );
    }
}
