//! Claude Code hook'ları (`duflow hook <event>`). Plugin'deki `scripts/hook.sh` bunları çağırır;
//! stdin'den hook JSON'u okur, stdout'a hook JSON'u basar. `flows/` yoksa ya da `watch` boşsa
//! sessizce çıkar; plugin global kurulunca alakasız repoları rahatsız etmez.
//!
//! Katmanlar:
//! - `session-start`: oturum başı git baseline'ı (HEAD + kirli dosyaların blob hash'i) yazılır,
//!   modele proje bağlamı verilir.
//! - `post-tool` (Edit/Write): dosya `watch` kapsamındaysa bir kez hedefli hatırlatma.
//! - `stop`: watch kapsamında dosya değişmiş ama `flows/` değişmemişse ya da lint hatası varsa
//!   durmayı engeller, modeli devam ettirir. `stop_hook_active` ile ikinci gelişte geçirir.
//! - `pre-bash` (`git commit`): lint hatası varsa komutu reddeder; drift varsa uyarı ekler.

use anyhow::{Context, Result};
use duflow_core::Graph;
use duflow_core::lint::{Level, lint};
use globset::{Glob, GlobSet, GlobSetBuilder};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Event {
    SessionStart,
    PostTool,
    Stop,
    PreBash,
}

/// Oturum başındaki durum: `$TMPDIR/duflow-hook/<session>.json`.
#[derive(Default, Serialize, Deserialize)]
struct Baseline {
    head: String,
    /// Oturum başında zaten kirli olan dosyalar → o anki içerik hash'i.
    dirty: BTreeMap<String, String>,
    /// post-tool hatırlatması yapılan dosyalar (bir kez).
    reminded: Vec<String>,
    /// flows/ altındaki .kdl dosyaları → içerik hash'i. Git'ten bağımsız: flows/ gitignore'lu
    /// (deneme alanı) olsa da değişim yakalanır.
    #[serde(default)]
    flows: BTreeMap<String, u64>,
}

struct Repo {
    root: PathBuf,
    /// flows dizini, repo köküne göre (`flows` ya da `x/flows`)
    flows_rel: String,
}

pub fn run(event: Event, dir: Option<PathBuf>) -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).ok();
    let input: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let cwd = input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir()?);
    // flows/ yoksa sessiz
    let Some(dir) = dir.or_else(|| find_flows(&cwd)) else {
        return Ok(());
    };
    let session = input
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("default")
        .to_string();
    let Ok(repo) = repo_of(&dir) else {
        return Ok(());
    };
    let out = match event {
        Event::SessionStart => session_start(&dir, &repo, &session)?,
        Event::PostTool => post_tool(&dir, &repo, &session, &input)?,
        Event::Stop => stop(&dir, &repo, &session, &input)?,
        Event::PreBash => pre_bash(&dir, &repo, &session, &input)?,
    };
    if let Some(v) = out {
        println!("{v}");
    }
    Ok(())
}

fn find_flows(cwd: &Path) -> Option<PathBuf> {
    let mut cur = cwd.to_path_buf();
    loop {
        let c = cur.join("flows");
        if c.is_dir() {
            return Some(c);
        }
        if !cur.pop() {
            return None;
        }
    }
}

fn repo_of(dir: &Path) -> Result<Repo> {
    let root = PathBuf::from(git(dir, &["rev-parse", "--show-toplevel"])?.trim());
    let flows_rel = dir
        .canonicalize()?
        .strip_prefix(root.canonicalize()?)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| "flows".into());
    Ok(Repo { root, flows_rel })
}

fn git(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(dir).output()?;
    if !out.status.success() {
        anyhow::bail!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}

fn baseline_path(session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    std::env::temp_dir()
        .join("duflow-hook")
        .join(format!("{safe}.json"))
}

/// Baseline yoksa (oturum SessionStart hook'suz başladı) kapı kapalı kalır: `None` döner,
/// çağıranlar sessiz geçer. Yanlış alarm, kaçırılan drift'ten kötü.
fn load_baseline(session: &str) -> Option<Baseline> {
    let s = std::fs::read_to_string(baseline_path(session)).ok()?;
    serde_json::from_str(&s).ok()
}

fn save_baseline(session: &str, b: &Baseline) -> Result<()> {
    let p = baseline_path(session);
    std::fs::create_dir_all(p.parent().unwrap())?;
    std::fs::write(&p, serde_json::to_string(b)?)?;
    Ok(())
}

/// Çalışma ağacında HEAD'e göre değişmiş + izlenmeyen dosyalar → içerik hash'i.
fn dirty_files(repo: &Repo) -> Result<BTreeMap<String, String>> {
    let out = git(
        &repo.root,
        &["status", "--porcelain", "-uall", "--no-renames"],
    )?;
    let mut m = BTreeMap::new();
    for line in out.lines() {
        if line.len() < 4 {
            continue;
        }
        let path = line[3..].trim().trim_matches('"').to_string();
        let full = repo.root.join(&path);
        let hash = if full.is_file() {
            git(&repo.root, &["hash-object", "--", &path]).unwrap_or_default()
        } else {
            "deleted".into()
        };
        m.insert(path, hash.trim().to_string());
    }
    Ok(m)
}

fn flows_snapshot(dir: &Path) -> BTreeMap<String, u64> {
    use std::hash::{Hash, Hasher};
    let mut m = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "kdl") {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                std::fs::read(&p).unwrap_or_default().hash(&mut h);
                m.insert(p.to_string_lossy().into_owned(), h.finish());
            }
        }
    }
    m
}

fn snapshot(dir: &Path, repo: &Repo) -> Result<Baseline> {
    Ok(Baseline {
        head: git(&repo.root, &["rev-parse", "HEAD"])
            .unwrap_or_default()
            .trim()
            .to_string(),
        dirty: dirty_files(repo)?,
        reminded: vec![],
        flows: flows_snapshot(dir),
    })
}

/// Oturum içinde değişen dosyalar (repo köküne göre): baseline'a göre commit'lenenler +
/// baseline'da olmayan ya da içeriği değişen kirli dosyalar.
fn changed_since(repo: &Repo, base: &Baseline) -> Result<Vec<String>> {
    let mut files = std::collections::BTreeSet::new();
    if !base.head.is_empty()
        && let Ok(out) = git(&repo.root, &["diff", "--name-only", &base.head, "HEAD"])
    {
        files.extend(out.lines().map(String::from));
    }
    for (path, hash) in dirty_files(repo)? {
        if base.dirty.get(&path) != Some(&hash) {
            files.insert(path);
        }
    }
    Ok(files.into_iter().collect())
}

fn globs(patterns: &[String]) -> Result<GlobSet> {
    let mut b = GlobSetBuilder::new();
    for p in patterns {
        b.add(Glob::new(p).with_context(|| format!("bad watch glob `{p}`"))?);
    }
    Ok(b.build()?)
}

fn watched(dir: &Path) -> Result<(GlobSet, Vec<String>)> {
    let g = Graph::load(dir)?;
    Ok((globs(&g.config.watch)?, g.config.watch))
}

/// Lint hataları; `only` verilirse yalnız o (flows'a göreli) dosyalardakiler. Paralel çalışan
/// ajanlar birbirinin namespace hatasıyla bloklanmasın diye Stop hook'u oturumda değişen
/// dosyalarla sınırlar.
fn lint_errors(dir: &Path, only: Option<&BTreeSet<String>>) -> Vec<String> {
    match Graph::load(dir) {
        Ok(g) => scoped_errors(lint(&g), only),
        Err(e) => vec![format!("{e:#}")],
    }
}

fn scoped_errors(
    diags: Vec<duflow_core::lint::Diagnostic>,
    only: Option<&BTreeSet<String>>,
) -> Vec<String> {
    diags
        .into_iter()
        .filter(|d| d.level == Level::Error)
        .filter(|d| only.is_none_or(|set| set.contains(&d.file)))
        .map(|d| d.to_string())
        .collect()
}

/// Baseline'a göre değişen flows dosyaları (flows'a göreli yol).
fn changed_flow_files(dir: &Path, base: &Baseline) -> BTreeSet<String> {
    let now = flows_snapshot(dir);
    let rel = |p: &str| {
        Path::new(p)
            .strip_prefix(dir)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| p.to_string())
    };
    let mut out = BTreeSet::new();
    for (p, h) in &now {
        if base.flows.get(p) != Some(h) {
            out.insert(rel(p));
        }
    }
    for p in base.flows.keys() {
        if !now.contains_key(p) {
            out.insert(rel(p));
        }
    }
    out
}

fn rel_to_root(repo: &Repo, path: &str) -> String {
    let p = Path::new(path);
    if p.is_absolute() {
        p.strip_prefix(&repo.root)
            .map(|r| r.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| path.to_string())
    } else {
        path.to_string()
    }
}

fn session_start(dir: &Path, repo: &Repo, session: &str) -> Result<Option<Value>> {
    save_baseline(session, &snapshot(dir, repo)?)?;
    let Ok(g) = Graph::load(dir) else {
        return Ok(None);
    };
    let mut ctx = format!(
        "duflow project `{}`: the system flow graph lives in `{}/` ({} nodes). \
         Before implementing anything that touches states, triggers, endpoints, checks or outcomes, \
         run `duflow brief <id>` / `duflow search <text>` for context; after the code change, update the \
         graph through the duflow CLI (never edit the .kdl files by hand) and run `duflow validate`.",
        g.config.name,
        repo.flows_rel,
        g.nodes.len()
    );
    if !g.config.watch.is_empty() {
        ctx.push_str(&format!(
            " Files under {} are watched: changing them without updating the graph blocks stopping and committing.",
            g.config.watch.iter().map(|w| format!("`{w}`")).collect::<Vec<_>>().join(", ")
        ));
    }
    Ok(Some(json!({
        "hookSpecificOutput": { "hookEventName": "SessionStart", "additionalContext": ctx }
    })))
}

fn post_tool(dir: &Path, repo: &Repo, session: &str, input: &Value) -> Result<Option<Value>> {
    let Some(path) = input
        .get("tool_input")
        .and_then(|t| t.get("file_path"))
        .and_then(Value::as_str)
    else {
        return Ok(None);
    };
    let rel = rel_to_root(repo, path);
    let (set, _) = watched(dir)?;
    if !set.is_match(&rel) {
        return Ok(None);
    }
    let mut base = load_baseline(session).unwrap_or_default();
    if base.reminded.contains(&rel) {
        return Ok(None);
    }
    base.reminded.push(rel.clone());
    save_baseline(session, &base)?;
    let ctx = format!(
        "`{rel}` is in duflow's watch scope. If this change adds/removes/alters a state, trigger, \
         endpoint, check, outcome or variable, reflect it in `{}/` via `duflow add|edit|rename|rm` \
         (see the duflow skill), then run `duflow validate`. If nothing in the flow changed, no action needed.",
        repo.flows_rel
    );
    Ok(Some(json!({
        "hookSpecificOutput": { "hookEventName": "PostToolUse", "additionalContext": ctx }
    })))
}

/// Oturumda değişen watch-kapsamı dosyalar ve flows altında değişiklik olup olmadığı.
fn drift(dir: &Path, repo: &Repo, base: &Baseline) -> Result<(Vec<String>, bool)> {
    let (set, patterns) = watched(dir)?;
    let changed = changed_since(repo, base)?;
    let flows_prefix = format!("{}/", repo.flows_rel);
    let flows_changed =
        changed.iter().any(|f| f.starts_with(&flows_prefix)) || flows_snapshot(dir) != base.flows;
    let code: Vec<String> = if patterns.is_empty() {
        vec![]
    } else {
        changed.into_iter().filter(|f| set.is_match(f)).collect()
    };
    Ok((code, flows_changed))
}

fn stop(dir: &Path, repo: &Repo, session: &str, input: &Value) -> Result<Option<Value>> {
    // ikinci geliş: model açıklamasını yaptı ya da düzeltti, geçir
    if input.get("stop_hook_active").and_then(Value::as_bool) == Some(true) {
        return Ok(None);
    }
    let Some(base) = load_baseline(session) else {
        return Ok(None);
    };
    let (code, flows_changed) = drift(dir, repo, &base)?;
    let errors = if flows_changed {
        stop_blocking(lint_errors(dir, Some(&changed_flow_files(dir, &base))))
    } else {
        vec![]
    };
    if code.is_empty() && errors.is_empty() {
        return Ok(None);
    }
    let mut reason = String::new();
    if !errors.is_empty() {
        reason.push_str("`duflow validate` reports errors in files you changed this session; fix them before stopping:\n");
        for e in errors.iter().take(20) {
            reason.push_str(&format!("  {e}\n"));
        }
    }
    if !code.is_empty() && !flows_changed {
        reason.push_str(&format!(
            "Watched source files changed this session but `{}/` did not:\n",
            repo.flows_rel
        ));
        for f in code.iter().take(20) {
            reason.push_str(&format!("  {f}\n"));
        }
        reason.push_str(
            "Either update the flow graph through the duflow CLI (`duflow brief`/`search` for context, \
             `duflow add|edit|rename|rm` to change, `duflow validate` to check) or state in one sentence \
             why the graph is unaffected, then stop.",
        );
    }
    if reason.is_empty() {
        return Ok(None);
    }
    Ok(Some(json!({ "decision": "block", "reason": reason })))
}

/// Stop'ta bloklayan hatalar: `unreachable` dosyalar arası ve paralel yazımda geçici (başka ajan
/// kenarı henüz yazmadı), o yüzden Stop'ta değil yalnız `git commit`'te bloklar.
fn stop_blocking(errors: Vec<String>) -> Vec<String> {
    errors
        .into_iter()
        .filter(|e| !e.contains("[unreachable]"))
        .collect()
}

fn pre_bash(dir: &Path, repo: &Repo, session: &str, input: &Value) -> Result<Option<Value>> {
    let cmd = input
        .get("tool_input")
        .and_then(|t| t.get("command"))
        .and_then(Value::as_str)
        .unwrap_or("");
    if !is_git_commit(cmd) {
        return Ok(None);
    }
    let errors = lint_errors(dir, None);
    if !errors.is_empty() {
        let reason = format!(
            "`duflow validate` has errors; fix them before committing:\n  {}",
            errors
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n  ")
        );
        return Ok(Some(json!({
            "hookSpecificOutput": {
                "hookEventName": "PreToolUse",
                "permissionDecision": "deny",
                "permissionDecisionReason": reason
            }
        })));
    }
    let Some(base) = load_baseline(session) else {
        return Ok(None);
    };
    let (code, flows_changed) = drift(dir, repo, &base)?;
    if code.is_empty() || flows_changed {
        return Ok(None);
    }
    let ctx = format!(
        "duflow: watched files changed this session ({}) but `{}/` did not. Make sure the flow graph \
         is updated (or unaffected) before finishing.",
        code.iter().take(5).cloned().collect::<Vec<_>>().join(", "),
        repo.flows_rel
    );
    Ok(Some(json!({
        "hookSpecificOutput": { "hookEventName": "PreToolUse", "additionalContext": ctx }
    })))
}

fn is_git_commit(cmd: &str) -> bool {
    // `git commit`, `git -C x commit`, `git commit -am ...`; `git log`/`git status` değil
    let toks: Vec<&str> = cmd.split_whitespace().collect();
    toks.windows(2).any(|w| w[0] == "git" && w[1] == "commit")
        || toks
            .iter()
            .position(|t| *t == "git")
            .map(|i| toks[i + 1..].iter().take(4).any(|t| *t == "commit"))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_git_commit() {
        assert!(is_git_commit("git commit -m x"));
        assert!(is_git_commit("git -C ../x commit -am y"));
        assert!(is_git_commit("cargo test && git commit -m x"));
        assert!(!is_git_commit("git status"));
        assert!(!is_git_commit("git log --oneline | grep commit"));
    }

    #[test]
    fn stop_errors_are_scoped_to_changed_files() {
        use duflow_core::lint::{Diagnostic, Level};
        let d = |file: &str, level| Diagnostic {
            level,
            code: "x",
            message: "m".into(),
            file: file.into(),
            line: 1,
            id: None,
        };
        let diags = vec![
            d("deploy.kdl", Level::Error),
            d("edge/dns.kdl", Level::Error),
            d("deploy.kdl", Level::Warning),
        ];
        let only: BTreeSet<String> = ["deploy.kdl".to_string()].into();
        assert_eq!(scoped_errors(diags.clone(), Some(&only)).len(), 1);
        assert_eq!(scoped_errors(diags, None).len(), 2);
    }

    #[test]
    fn stop_ignores_unreachable_but_keeps_local_errors() {
        let errs = vec![
            "error a.kdl:1 [unreachable] `a.x` is not reachable from any root".to_string(),
            "error a.kdl:2 [dangling_ref] `a.x` → `a.y`: target not defined".to_string(),
        ];
        let kept = stop_blocking(errs);
        assert_eq!(kept.len(), 1);
        assert!(kept[0].contains("dangling_ref"));
    }

    #[test]
    fn glob_matches_repo_relative() {
        let set = globs(&["dorch/src/api/**".into(), "ui/src/views/**".into()]).unwrap();
        assert!(set.is_match("dorch/src/api/deploys.rs"));
        assert!(set.is_match("ui/src/views/projects/Deploys.vue"));
        assert!(!set.is_match("dorch/src/models/x.rs"));
    }
}
