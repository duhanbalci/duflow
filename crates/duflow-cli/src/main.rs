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
    Validate,
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
    /// List nodes (filters: --kind, --layer, --prefix)
    Ls {
        #[arg(long, value_parser = ["state", "action", "call", "event"])]
        kind: Option<String>,
        #[arg(long, add = ArgValueCompleter::new(complete_layer))]
        layer: Option<String>,
        #[arg(long, add = ArgValueCompleter::new(complete_node))]
        prefix: Option<String>,
    },
    /// New node: duflow add state deploy.done --layer domain --desc "..." --child '-> "x"'
    Add(AddArgs),
    /// Edit a node: --set key=value, --child '<kdl line>', --rm-edge <target>, --rm-child name:arg
    Edit(EditArgs),
    /// Rename an ID (all references + file move)
    Rename {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        from: String,
        to: String,
    },
    /// Remove a node (--force required if referenced)
    Rm {
        #[arg(add = ArgValueCompleter::new(complete_node))]
        id: String,
        #[arg(long)]
        force: bool,
    },
    /// Apply a JSON op list from stdin (`-`) or a file
    Apply {
        #[arg(default_value = "-")]
        file: String,
        /// Preview without writing
        #[arg(long)]
        dry_run: bool,
    },
    /// Graph diff between two git revs (working tree if rev2 omitted)
    Diff {
        #[arg(add = ArgValueCompleter::new(complete_rev))]
        rev1: String,
        #[arg(add = ArgValueCompleter::new(complete_rev))]
        rev2: Option<String>,
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
    /// Update duflow to the latest GitHub release (--check only reports)
    SelfUpdate {
        /// Only check, do not install
        #[arg(long)]
        check: bool,
    },
}

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

/// Binary'ye gömülü skill; kaynak repo'daki `.claude/skills/duflow/SKILL.md`
const SKILL_MD: &str = include_str!("../../../.claude/skills/duflow/SKILL.md");

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
    /// state | action | call | event | var | check | root
    #[arg(value_parser = ["state", "action", "call", "event", "var", "check", "root"])]
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
        _ => {}
    }
    let dir = match &cli.cmd {
        Cmd::Completions { .. } => PathBuf::new(),
        _ => find_dir(cli.dir.clone())?,
    };
    match cli.cmd {
        Cmd::SelfUpdate { .. } | Cmd::Skill(_) => unreachable!(),
        Cmd::Validate => {
            let g = Graph::load(&dir)?;
            let d = lint(&g);
            if json {
                println!("{}", serde_json::to_string_pretty(&d)?);
            } else {
                for x in &d {
                    println!("{x}");
                }
                let e = d.iter().filter(|x| x.level == Level::Error).count();
                let w = d.len() - e;
                println!(
                    "{} nodes, {} vars, {} checks · {e} errors, {w} warnings",
                    g.nodes.len(),
                    g.vars.len(),
                    g.checks.len()
                );
            }
            if d.iter().any(|x| x.level == Level::Error) {
                std::process::exit(1);
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
        } => {
            let g = Graph::load(&dir)?;
            let list: Vec<_> = g
                .nodes
                .values()
                .filter(|n| kind.as_deref().is_none_or(|k| n.kind.as_str() == k))
                .filter(|n| layer.as_deref().is_none_or(|l| n.layer == l))
                .filter(|n| prefix.as_deref().is_none_or(|p| n.id.starts_with(p)))
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
                "root" => Op::AddRoot { id: a.id },
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
            let mut ops = vec![];
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
            if ops.is_empty() {
                bail!("no operations given");
            }
            apply_ops(&dir, &ops, false, json)?;
        }
        Cmd::Rename { from, to } => apply_ops(&dir, &[Op::Rename { from, to }], false, json)?,
        Cmd::Rm { id, force } => apply_ops(&dir, &[Op::RmNode { id, force }], false, json)?,
        Cmd::Apply { file, dry_run } => {
            let src = if file == "-" {
                let mut s = String::new();
                std::io::stdin().read_to_string(&mut s)?;
                s
            } else {
                std::fs::read_to_string(&file)?
            };
            let ops: Vec<Op> =
                serde_json::from_str(&src).context("could not parse JSON op list")?;
            apply_ops(&dir, &ops, dry_run, json)?;
        }
        Cmd::Diff { rev1, rev2 } => {
            let a = graph_at(&dir, &rev1)?;
            let b = match rev2 {
                Some(r) => graph_at(&dir, &r)?,
                None => Graph::load(&dir)?,
            };
            let d = diff(&a, &b);
            if json {
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

fn apply_ops(dir: &Path, ops: &[Op], dry_run: bool, json: bool) -> Result<()> {
    let mut ws = Workspace::open(dir)?;
    ws.apply_all(ops)?;
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
        println!("{} {}", if same { "up to date" } else { "wrote" }, f.display());
    }
    if project {
        println!("commit these so teammates and CI agents get the skill too");
    }
    Ok(())
}
