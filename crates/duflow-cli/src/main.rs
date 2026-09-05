//! `duflow` CLI. Tüm sorgular `--json` alır; insan çıktısı markdown/metin.

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use duflow_core::diff::diff;
use duflow_core::edit::{Op, Workspace};
use duflow_core::export;
use duflow_core::lint::{Level, lint};
use duflow_core::query;
use duflow_core::Graph;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

mod ui;

#[derive(Parser)]
#[command(name = "duflow", version, about = "Sistem akış grafı: KDL dosyaları, sorgu, düzenleme, UI")]
struct Cli {
    /// flows dizini (varsayılan: ./flows, yoksa üst dizinlerde aranır)
    #[arg(short = 'd', long, global = true)]
    dir: Option<PathBuf>,
    /// JSON çıktı
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Lint: kopuk referans, erişilemeyen node, tanımsız var... (hata varsa exit 1)
    Validate,
    /// Bir node'un AI için tek parça özeti
    Brief {
        id: String,
        /// Geriye/ileriye derinlik (öncül listesi)
        #[arg(long, default_value_t = 1)]
        depth: usize,
    },
    /// Bir node'a gelmek için gereken adımlar, check'ler, değişkenler
    Prereq {
        id: String,
        #[arg(long, default_value_t = 3)]
        depth: usize,
    },
    /// İki node arası yollar
    Path {
        from: String,
        to: String,
        /// En kısa yerine tüm basit yollar (en fazla --max)
        #[arg(long)]
        all: bool,
        #[arg(long, default_value_t = 5)]
        max: usize,
        #[arg(long, default_value_t = 30)]
        depth: usize,
    },
    /// Bir node'dan ileriye erişilebilen küme
    Reach { id: String },
    /// Bir değişkeni kim yazıyor, kim okuyor
    Var { id: String },
    /// ID/desc/check/var üstünde fuzzy arama
    Search {
        query: String,
        #[arg(long, default_value_t = 15)]
        limit: usize,
    },
    /// Node listesi (filtre: --kind, --layer, --prefix)
    Ls {
        #[arg(long)]
        kind: Option<String>,
        #[arg(long)]
        layer: Option<String>,
        #[arg(long)]
        prefix: Option<String>,
    },
    /// Yeni node: duflow add state deploy.done --layer domain --desc "..." --child '-> "x"'
    Add(AddArgs),
    /// Node düzenle: --set key=value, --child '<kdl satırı>', --rm-edge <hedef>, --rm-child name:arg
    Edit(EditArgs),
    /// ID yeniden adlandır (tüm referanslar + dosya taşıma)
    Rename { from: String, to: String },
    /// Node sil (referans varsa --force gerekir)
    Rm {
        id: String,
        #[arg(long)]
        force: bool,
    },
    /// stdin'den JSON işlem listesi uygula (`-`), ya da dosyadan
    Apply {
        #[arg(default_value = "-")]
        file: String,
        /// Yazmadan önizle
        #[arg(long)]
        dry_run: bool,
    },
    /// İki git rev'i arası graf farkı (rev2 verilmezse çalışma dizini)
    Diff {
        rev1: String,
        rev2: Option<String>,
    },
    /// Grafı UI JSON şemasında bas
    Export {
        /// Diff overlay için taban rev
        #[arg(long)]
        diff: Option<String>,
    },
    /// UI
    #[command(subcommand)]
    Ui(UiCmd),
}

#[derive(Subcommand)]
enum UiCmd {
    /// Tek dosya statik site üret
    Build {
        #[arg(short, long, default_value = "duflow.html")]
        out: PathBuf,
        /// Diff overlay: `main` ya da `a..b`
        #[arg(long)]
        diff: Option<String>,
    },
    /// Yerel sunucu; her istekte yeniden derler (F5 yeter)
    Serve {
        #[arg(long, default_value = "127.0.0.1:4646")]
        addr: String,
    },
}

#[derive(Args)]
struct AddArgs {
    /// state | action | call | event | var | check | root
    kind: String,
    id: String,
    #[arg(long)]
    layer: Option<String>,
    #[arg(long)]
    desc: Option<String>,
    /// Ek attribute: --attr method=POST --attr path=/x
    #[arg(long = "attr")]
    attrs: Vec<String>,
    /// Çocuk satırı (ham KDL), tekrarlanabilir
    #[arg(long = "child", allow_hyphen_values = true)]
    children: Vec<String>,
}

#[derive(Args)]
struct EditArgs {
    id: String,
    #[arg(long = "set", allow_hyphen_values = true)]
    sets: Vec<String>,
    #[arg(long = "child", allow_hyphen_values = true)]
    children: Vec<String>,
    #[arg(long = "rm-edge")]
    rm_edges: Vec<String>,
    /// `name:arg`, örn. `check:perm:x` ya da `sets:deploy.attempts`
    #[arg(long = "rm-child")]
    rm_children: Vec<String>,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("hata: {e:#}");
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
            bail!("`flows/` dizini bulunamadı (--dir ile ver)");
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let dir = find_dir(cli.dir.clone())?;
    let json = cli.json;
    match cli.cmd {
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
                println!("{} node, {} var, {} check · {e} hata, {w} uyarı", g.nodes.len(), g.vars.len(), g.checks.len());
            }
            if d.iter().any(|x| x.level == Level::Error) {
                std::process::exit(1);
            }
        }
        Cmd::Brief { id, depth } => {
            let g = Graph::load(&dir)?;
            let b = query::brief(&g, &id).with_context(|| format!("`{id}` yok"))?;
            let _ = depth;
            if json {
                println!("{}", serde_json::to_string_pretty(&b)?);
            } else {
                print!("{}", b.to_markdown(&g));
            }
        }
        Cmd::Prereq { id, depth } => {
            let g = Graph::load(&dir)?;
            let p = query::prereq(&g, &id, depth).with_context(|| format!("`{id}` yok"))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&p)?);
            } else {
                print!("{}", p.to_markdown());
            }
        }
        Cmd::Path { from, to, all, max, depth } => {
            let g = Graph::load(&dir)?;
            for x in [&from, &to] {
                if !g.nodes.contains_key(x) {
                    bail!("`{x}` yok");
                }
            }
            let paths: Vec<Vec<String>> = if all { g.all_paths(&from, &to, max, depth) } else { g.shortest_path(&from, &to).into_iter().collect() };
            if json {
                println!("{}", serde_json::to_string_pretty(&paths)?);
            } else if paths.is_empty() {
                println!("yol yok");
                std::process::exit(1);
            } else {
                for p in &paths {
                    println!("{} adım: {}", p.len() - 1, p.join(" → "));
                }
            }
        }
        Cmd::Reach { id } => {
            let g = Graph::load(&dir)?;
            if !g.nodes.contains_key(&id) {
                bail!("`{id}` yok");
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
            let v = g.vars.get(&id).with_context(|| format!("var `{id}` yok"))?;
            let writers: Vec<String> = g.var_writers(&id).into_iter().map(|(n, s)| format!("{} {} ({}:{})", n.id, s.value, n.file, s.line)).collect();
            let readers: Vec<String> = g.var_readers(&id).into_iter().map(|(n, w)| format!("{n} [{w}]")).collect();
            if json {
                println!("{}", serde_json::json!({"var": v, "writers": writers, "readers": readers}));
            } else {
                println!("# {} ({}){}", v.id, v.ty, v.source.as_ref().map(|s| format!(" · kaynak {s}")).unwrap_or_default());
                println!("\n## Yazan\n{}", if writers.is_empty() { "(yok)".into() } else { writers.join("\n") });
                println!("\n## Okuyan\n{}", if readers.is_empty() { "(yok)".into() } else { readers.join("\n") });
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
        Cmd::Ls { kind, layer, prefix } => {
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
                    println!("{:<7} {:<7} {}  {}", n.kind.as_str(), n.layer, n.id, n.desc.clone().unwrap_or_default());
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
                let (k, v) = kv.split_once('=').with_context(|| format!("--attr `{kv}`: key=value bekleniyor"))?;
                attrs.insert(k.into(), v.into());
            }
            let op = match a.kind.as_str() {
                "var" => Op::AddVar { id: a.id, attrs },
                "check" => Op::AddCheck { id: a.id, attrs },
                "root" => Op::AddRoot { id: a.id },
                k => Op::AddNode { kind: k.into(), id: a.id, attrs, children: a.children },
            };
            apply_ops(&dir, &[op], false, json)?;
        }
        Cmd::Edit(e) => {
            let mut ops = vec![];
            for kv in e.sets {
                let (k, v) = kv.split_once('=').with_context(|| format!("--set `{kv}`: key=value bekleniyor"))?;
                ops.push(Op::SetAttr { id: e.id.clone(), key: k.into(), value: v.into() });
            }
            for c in e.children {
                ops.push(Op::AddChild { id: e.id.clone(), line: c });
            }
            for t in e.rm_edges {
                ops.push(Op::RmEdge { id: e.id.clone(), to: t });
            }
            for rc in e.rm_children {
                let (name, arg) = rc.split_once(':').with_context(|| format!("--rm-child `{rc}`: name:arg bekleniyor"))?;
                ops.push(Op::RmChild { id: e.id.clone(), name: name.into(), arg: arg.into() });
            }
            if ops.is_empty() {
                bail!("hiç işlem verilmedi");
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
            let ops: Vec<Op> = serde_json::from_str(&src).context("JSON işlem listesi parse edilemedi")?;
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
            eprintln!("{} yazıldı ({} node)", out.display(), g.nodes.len());
        }
        Cmd::Ui(UiCmd::Serve { addr }) => ui::serve(&dir, &addr)?,
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
        println!("{}", serde_json::json!({"written": written, "errors": errors}));
    } else {
        for f in &written {
            println!("yazıldı: {f}");
        }
        for e in &errors {
            println!("{e}");
        }
    }
    Ok(())
}

/// Bir git rev'indeki `flows/` dizininden graf.
pub fn graph_at(dir: &Path, rev: &str) -> Result<Graph> {
    let repo_root = git(dir, &["rev-parse", "--show-toplevel"])?.trim().to_string();
    let rel = dir.canonicalize()?.strip_prefix(Path::new(&repo_root).canonicalize()?).map(|p| p.to_string_lossy().replace('\\', "/")).unwrap_or_default();
    let list = git(dir, &["ls-tree", "-r", "--name-only", rev, "--", &rel])?;
    let mut sources = vec![];
    for path in list.lines().filter(|l| l.ends_with(".kdl")) {
        let src = git(dir, &["show", &format!("{rev}:{path}")])?;
        let inner = path.strip_prefix(&format!("{rel}/")).unwrap_or(path).to_string();
        sources.push((inner, src));
    }
    Ok(Graph::from_sources(&sources)?)
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output().context("git çalıştırılamadı")?;
    if !out.status.success() {
        bail!("git {}: {}", args.join(" "), String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8(out.stdout)?)
}

/// `base` verilirse diff overlay'li export. `a..b` biçimi de kabul.
pub fn export_json(dir: &Path, g: &Graph, base: Option<&str>) -> Result<String> {
    let d;
    let old;
    let mut exp = match base {
        Some(spec) => {
            let (r1, r2) = spec.split_once("..").map(|(a, b)| (a, Some(b))).unwrap_or((spec, None));
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
