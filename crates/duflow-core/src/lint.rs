//! Lint kuralları (spec §5).

use crate::expr;
use crate::graph::Graph;
use crate::model::*;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct Diagnostic {
    pub level: Level,
    pub code: &'static str,
    pub message: String,
    pub file: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl std::fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let lvl = match self.level {
            Level::Error => "error",
            Level::Warning => "warn ",
        };
        write!(
            f,
            "{lvl} {}:{} [{}] {}",
            self.file, self.line, self.code, self.message
        )
    }
}

pub fn lint(g: &Graph) -> Vec<Diagnostic> {
    let mut d = vec![];
    let push = |d: &mut Vec<Diagnostic>,
                level,
                code,
                file: &str,
                line,
                id: Option<&str>,
                message: String| {
        d.push(Diagnostic {
            level,
            code,
            message,
            file: file.into(),
            line,
            id: id.map(String::from),
        });
    };

    for (id, file, line) in &g.duplicates {
        push(
            &mut d,
            Level::Error,
            "duplicate_id",
            file,
            *line,
            Some(id),
            format!("`{id}` defined twice"),
        );
    }

    let layers: BTreeSet<&str> = g.config.layers.iter().map(String::as_str).collect();
    let reachable = g.reachable_from_roots();
    let root_set: BTreeSet<&str> = g.root_ids().into_iter().collect();
    // guard ifadesi (kenar ya da hedefsiz `-> outcome=`): var tanımlı mı, yerel sayaç yazılıyor mu
    let guard_vars = |d: &mut Vec<Diagnostic>, n: &Node, w: &str, line: usize| {
        for v in expr::idents(w) {
            if g.vars.contains_key(&v) {
                continue;
            }
            if g.is_local_var(&v) {
                if g.local_var_writers(local_group(&n.id), &v).is_empty() {
                    push(
                        d,
                        Level::Error,
                        "local_var_never_set",
                        &n.file,
                        line,
                        Some(&n.id),
                        format!(
                            "guard `{w}`: local `{v}` is never `sets` in group `{}` (declare a `var` if it is global)",
                            local_group(&n.id)
                        ),
                    );
                }
            } else {
                push(
                    d,
                    Level::Error,
                    "unknown_var",
                    &n.file,
                    line,
                    Some(&n.id),
                    format!("guard `{w}`: `{v}` is not a defined var"),
                );
            }
        }
    };

    for n in g.nodes.values() {
        // ID ↔ dosya yolu
        let allowed = allowed_files(&n.id);
        if !allowed.contains(&n.file) {
            push(
                &mut d,
                Level::Error,
                "id_path_mismatch",
                &n.file,
                n.line,
                Some(&n.id),
                format!("`{}` must live in one of: {}", n.id, allowed.join(" | ")),
            );
        }
        if !layers.contains(n.layer.as_str()) {
            push(
                &mut d,
                Level::Error,
                "unknown_layer",
                &n.file,
                n.line,
                Some(&n.id),
                format!(
                    "unknown layer `{}` (defined: {})",
                    n.layer,
                    g.config.layers.join(", ")
                ),
            );
        }
        if n.desc.is_none() {
            push(
                &mut d,
                Level::Warning,
                "missing_desc",
                &n.file,
                n.line,
                Some(&n.id),
                format!("`{}` has no `desc`", n.id),
            );
        }
        if !reachable.contains(&n.id) && !root_set.contains(n.id.as_str()) && !is_entry(n) {
            push(
                &mut d,
                Level::Error,
                "unreachable",
                &n.file,
                n.line,
                Some(&n.id),
                format!("`{}` is not reachable from any root", n.id),
            );
        }
        // kenar hedefleri
        for e in &n.edges {
            if !g.nodes.contains_key(&e.to) {
                push(
                    &mut d,
                    Level::Error,
                    "dangling_ref",
                    &n.file,
                    e.line,
                    Some(&n.id),
                    format!("`{}` → `{}`: target not defined", n.id, e.to),
                );
            }
            // `on` herhangi bir node'u dinleyebilir (state'e giriş de olaydır); yalnız yoksa uyar
            if let EdgeKind::On { event } = &e.kind
                && !g.nodes.contains_key(event)
            {
                push(
                    &mut d,
                    Level::Warning,
                    "unknown_event",
                    &n.file,
                    e.line,
                    Some(&n.id),
                    format!("`on {event}`: no such node"),
                );
            }
            if let EdgeKind::Calls = e.kind {
                if g.nodes.get(&e.to).is_some_and(|t| t.kind != Kind::Call) {
                    push(
                        &mut d,
                        Level::Error,
                        "calls_non_call",
                        &n.file,
                        e.line,
                        Some(&n.id),
                        format!("`calls {}`: target is not a `call`", e.to),
                    );
                }
            }
            if let Some(w) = &e.when {
                guard_vars(&mut d, n, w, e.line);
            }
        }
        for o in &n.outcomes {
            if let Some(w) = &o.when {
                guard_vars(&mut d, n, w, o.line);
            }
        }
        // check kullanımları
        for c in &n.checks {
            let def = g.checks.get(&c.name);
            if def.is_none() {
                match c.name.strip_prefix("perm:") {
                    Some(p) => push(
                        &mut d,
                        Level::Error,
                        "unknown_perm",
                        &n.file,
                        c.line,
                        Some(&n.id),
                        format!("`requires {p}`: no such `perm`"),
                    ),
                    None => push(
                        &mut d,
                        Level::Error,
                        "unknown_check",
                        &n.file,
                        c.line,
                        Some(&n.id),
                        format!("`check {}`: not defined", c.name),
                    ),
                }
            }
            if g.effective_outcome(c).is_some() {
                continue;
            }
            let target = c.to.clone().or_else(|| def.and_then(|x| x.fail_to.clone()));
            match target {
                Some(t) if !g.nodes.contains_key(&t) => push(
                    &mut d,
                    Level::Error,
                    "dangling_ref",
                    &n.file,
                    c.line,
                    Some(&n.id),
                    format!("check `{}` fail target `{t}` not defined", c.name),
                ),
                None => push(
                    &mut d,
                    Level::Warning,
                    "check_no_fail_target",
                    &n.file,
                    c.line,
                    Some(&n.id),
                    format!("check `{}` has no fail target", c.name),
                ),
                _ => {}
            }
        }
        for s in &n.sets {
            // noktasız ve tanımsız → yerel sayaç, serbest
            if !g.vars.contains_key(&s.var) && !g.is_local_var(&s.var) {
                push(
                    &mut d,
                    Level::Error,
                    "unknown_var",
                    &n.file,
                    s.line,
                    Some(&n.id),
                    format!("`sets {}`: not a defined var", s.var),
                );
            }
        }
        match n.kind {
            Kind::Call
                if n.outcomes.is_empty()
                    && n.edges
                        .iter()
                        .all(|e| !matches!(e.kind, EdgeKind::Returns { .. })) =>
            {
                push(
                    &mut d,
                    Level::Error,
                    "call_no_returns",
                    &n.file,
                    n.line,
                    Some(&n.id),
                    format!("`{}` has no `returns`", n.id),
                );
            }
            Kind::Action if n.edges.is_empty() => {
                push(
                    &mut d,
                    Level::Warning,
                    "action_no_effect",
                    &n.file,
                    n.line,
                    Some(&n.id),
                    format!("`{}` triggers nothing", n.id),
                );
            }
            // olay node'u yalnız dinlenir (`on`); teslimat kenarı domain'den değil altyapıdan çıkar
            Kind::Event
                if n
                    .edges
                    .iter()
                    .any(|e| !matches!(e.kind, EdgeKind::On { .. })) =>
            {
                push(
                    &mut d,
                    Level::Warning,
                    "event_has_transition",
                    &n.file,
                    n.line,
                    Some(&n.id),
                    format!(
                        "event `{}` has outgoing transitions; events are consumed via `on`, model delivery once in the infrastructure chain",
                        n.id
                    ),
                );
            }
            _ => {}
        }
        // guard'sız ve etiketsiz birden fazla koşulsuz `->` (case="..." dallanmayı, seq= sırayı adlandırır)
        let plain_unguarded = n
            .edges
            .iter()
            .filter(|e| {
                matches!(e.kind, EdgeKind::Plain)
                    && e.when.is_none()
                    && e.case.is_none()
                    && e.seq.is_none()
            })
            .count();
        if plain_unguarded > 1 {
            push(
                &mut d,
                Level::Warning,
                "ambiguous_transition",
                &n.file,
                n.line,
                Some(&n.id),
                format!("`{}` has {plain_unguarded} unguarded transitions", n.id),
            );
        }
        // aynı node'da tekrar eden case= / seq= sessizce geçmesin
        let mut seen_case: BTreeSet<&str> = BTreeSet::new();
        let cases = n
            .edges
            .iter()
            .filter_map(|e| e.case.as_deref().map(|c| (c, e.line)))
            .chain(n.outcomes.iter().filter_map(|o| o.case.as_deref().map(|c| (c, o.line))));
        for (c, line) in cases {
            if !seen_case.insert(c) {
                push(
                    &mut d,
                    Level::Warning,
                    "duplicate_case",
                    &n.file,
                    line,
                    Some(&n.id),
                    format!("`{}`: case `{c}` used more than once", n.id),
                );
            }
        }
        // aynı node'da hem sıra hem dal: dallanma son seq adımının node'una ait
        let has_seq = n.edges.iter().any(|e| e.seq.is_some());
        let has_case = n
            .edges
            .iter()
            .any(|e| matches!(e.kind, EdgeKind::Plain) && e.case.is_some() && e.seq.is_none());
        if has_seq && has_case {
            push(
                &mut d,
                Level::Warning,
                "mixed_branching",
                &n.file,
                n.line,
                Some(&n.id),
                format!(
                    "`{}` mixes seq= steps with case= branches; move the branching to the last step's node",
                    n.id
                ),
            );
        }
        let mut seen_seq: BTreeSet<u32> = BTreeSet::new();
        for e in &n.edges {
            if let Some(q) = e.seq
                && !seen_seq.insert(q)
            {
                push(
                    &mut d,
                    Level::Warning,
                    "duplicate_seq",
                    &n.file,
                    e.line,
                    Some(&n.id),
                    format!("`{}`: seq {q} used more than once", n.id),
                );
            }
        }
    }

    // var'lar
    for v in g.vars.values() {
        let writers = g.var_writers(&v.id);
        let readers = g.var_readers(&v.id);
        if writers.is_empty() && v.source.is_none() {
            let lvl = if readers.is_empty() {
                Level::Warning
            } else {
                Level::Error
            };
            push(
                &mut d,
                lvl,
                "var_never_set",
                &v.file,
                v.line,
                Some(&v.id),
                format!("`{}` is never set and has no `source`", v.id),
            );
        }
        if readers.is_empty() {
            push(
                &mut d,
                Level::Warning,
                "var_never_read",
                &v.file,
                v.line,
                Some(&v.id),
                format!("`{}` is never read by a guard/check", v.id),
            );
        }
    }
    // check tanımları (perm'den türeyenler `perm_unused` altında)
    for c in g.checks.values() {
        if c.id.starts_with("perm:") {
            continue;
        }
        if g.check_users(&c.id).is_empty() {
            push(
                &mut d,
                Level::Warning,
                "check_unused",
                &c.file,
                c.line,
                Some(&c.id),
                format!("check `{}` is never used", c.id),
            );
        }
        for r in &c.reads {
            if !g.vars.contains_key(r) {
                push(
                    &mut d,
                    Level::Error,
                    "unknown_var",
                    &c.file,
                    c.line,
                    Some(&c.id),
                    format!("check `{}` reads `{r}`: not defined", c.id),
                );
            }
        }
        if let Some(t) = &c.fail_to {
            if !g.nodes.contains_key(t) {
                push(
                    &mut d,
                    Level::Error,
                    "dangling_ref",
                    &c.file,
                    c.line,
                    Some(&c.id),
                    format!("check `{}` fail target `{t}` not defined", c.id),
                );
            }
        }
    }
    for p in g.perms.values() {
        if g.perm_users(&p.id).is_empty() {
            push(
                &mut d,
                Level::Warning,
                "perm_unused",
                &p.file,
                p.line,
                Some(&p.id),
                format!("perm `{}` is never required", p.id),
            );
        }
        if let Some(t) = &p.fail_to
            && !g.nodes.contains_key(t)
        {
            push(
                &mut d,
                Level::Error,
                "dangling_ref",
                &p.file,
                p.line,
                Some(&p.id),
                format!("perm `{}` fail target `{t}` not defined", p.id),
            );
        }
    }
    for r in &g.roots {
        if !g.nodes.contains_key(&r.id) {
            push(
                &mut d,
                Level::Error,
                "dangling_ref",
                &r.file,
                r.line,
                Some(&r.id),
                format!("root `{}` not defined", r.id),
            );
            continue;
        }
        // giren kenarı olan node root olmamalı (uydurma root freni); self-loop sayılmaz
        let incoming: Vec<_> = g
            .incoming_edges(&r.id)
            .into_iter()
            .filter(|e| e.from != r.id)
            .collect();
        if !incoming.is_empty() {
            push(
                &mut d,
                Level::Warning,
                "root_has_incoming",
                &r.file,
                r.line,
                Some(&r.id),
                format!(
                    "root `{}` already has incoming edges from {}; is it really an entry point?",
                    r.id,
                    incoming
                        .iter()
                        .take(3)
                        .map(|e| e.from.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            );
        }
    }
    // periyodik root'lar (`every=`) şemanın istediği şey, sayıma girmez
    let entry_roots = g.roots.iter().filter(|r| r.every.is_none()).count();
    if entry_roots > g.config.max_roots {
        push(
            &mut d,
            Level::Warning,
            "too_many_roots",
            "flows",
            0,
            None,
            format!(
                "{entry_roots} non-periodic roots (max_roots {}); entry points should be few, prefer edges or `entry=` on calls",
                g.config.max_roots
            ),
        );
    }
    if g.roots.is_empty() && !g.nodes.is_empty() {
        push(
            &mut d,
            Level::Error,
            "no_roots",
            "flows",
            0,
            None,
            "no `root` defined".into(),
        );
    }
    for v in &g.views {
        for x in [&v.from, &v.to] {
            if !g.nodes.contains_key(x) {
                push(
                    &mut d,
                    Level::Error,
                    "dangling_ref",
                    &v.file,
                    v.line,
                    Some(&v.id),
                    format!("view `{}`: `{x}` not defined", v.id),
                );
            }
        }
    }

    d.sort_by(|a, b| {
        b.level
            .cmp(&a.level)
            .then(a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    d
}

pub fn has_errors(d: &[Diagnostic]) -> bool {
    d.iter().any(|x| x.level == Level::Error)
}

/// `src="path[#symbol|:line]"` attr'ı repoya karşı: dosya var mı, sembol adı dosyada tam kelime
/// olarak geçiyor mu. Kod dosyasına sembolsüz/satırsız işaret `src_symbol_unchecked` uyarısı
/// (sessiz geçmesin). Graf saf kaldığı için ayrı fonksiyon; CLI `validate` repo köküyle çağırır.
pub fn src_lint(g: &Graph, repo_root: &std::path::Path) -> Vec<Diagnostic> {
    const CODE_EXT: [&str; 8] = ["rs", "ts", "tsx", "js", "jsx", "vue", "go", "py"];
    let mut d = vec![];
    for n in g.nodes.values() {
        let Some(src) = n.attrs.get("src") else {
            continue;
        };
        let (path, symbol, line) = match src.split_once('#') {
            Some((p, s)) => (p, Some(s), None),
            None => match src.rsplit_once(':') {
                Some((p, l)) if l.parse::<usize>().is_ok() => (p, None, Some(l)),
                _ => (src.as_str(), None, None),
            },
        };
        let full = repo_root.join(path);
        let Ok(text) = std::fs::read_to_string(&full) else {
            d.push(Diagnostic {
                level: Level::Warning,
                code: "src_missing",
                message: format!("`{}`: src file `{path}` not found", n.id),
                file: n.file.clone(),
                line: n.line,
                id: Some(n.id.clone()),
            });
            continue;
        };
        match symbol {
            Some(sym) if !contains_word(&text, sym) => d.push(Diagnostic {
                level: Level::Warning,
                code: "src_symbol_missing",
                message: format!("`{}`: `{sym}` not found in `{path}`", n.id),
                file: n.file.clone(),
                line: n.line,
                id: Some(n.id.clone()),
            }),
            None if line.is_none()
                && std::path::Path::new(path)
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| CODE_EXT.contains(&e)) =>
            {
                d.push(Diagnostic {
                    level: Level::Warning,
                    code: "src_symbol_unchecked",
                    message: format!(
                        "`{}`: src `{path}` names no `#symbol` (or `:line`), nothing to verify",
                        n.id
                    ),
                    file: n.file.clone(),
                    line: n.line,
                    id: Some(n.id.clone()),
                });
            }
            _ => {}
        }
    }
    d
}

/// `word` metinde tam kelime olarak (tanımlayıcı karakterleriyle çevrili değil) geçiyor mu.
fn contains_word(text: &str, word: &str) -> bool {
    let is_ident = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(i) = text[from..].find(word) {
        let start = from + i;
        let end = start + word.len();
        let before_ok = start == 0 || !is_ident(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !is_ident(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(files: &[(&str, &str)]) -> Graph {
        let src: Vec<(String, String)> = files
            .iter()
            .map(|(f, s)| (f.to_string(), s.to_string()))
            .collect();
        Graph::from_sources(&src).unwrap()
    }
    fn codes(d: &[Diagnostic]) -> Vec<&str> {
        d.iter().map(|x| x.code).collect()
    }

    const BASE: &str = r#"
root "a.start"
state "a.start" desc="s" { -> "a.mid" }
"#;

    #[test]
    fn check_allowed_in_state_and_produces_fail_edge() {
        let g = mk(&[
            (
                "a.kdl",
                &format!(
                    "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  check \"has_ip\" -> \"a.fail\"\n  -> \"a.ok\"\n}}\nstate \"a.ok\" desc=\"o\"\nstate \"a.fail\" desc=\"f\"\n"
                ),
            ),
            ("checks.kdl", "check \"has_ip\" desc=\"ip\"\n"),
        ]);
        let d = lint(&g);
        assert!(!codes(&d).contains(&"check_outside_call"), "{d:?}");
        assert!(!has_errors(&d), "{d:?}");
        assert!(
            g.outgoing("a.mid")
                .iter()
                .any(|e| e.to == "a.fail" && e.class == "fail")
        );
    }

    #[test]
    fn case_labelled_fanout_is_not_ambiguous() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  -> \"a.x\" case=\"x\"\n  -> \"a.y\" case=\"y\"\n}}\nstate \"a.x\" desc=\"x\"\nstate \"a.y\" desc=\"y\"\n"
            ),
        )]);
        assert!(!codes(&lint(&g)).contains(&"ambiguous_transition"));
        let g2 = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  -> \"a.x\"\n  -> \"a.y\"\n}}\nstate \"a.x\" desc=\"x\"\nstate \"a.y\" desc=\"y\"\n"
            ),
        )]);
        assert!(codes(&lint(&g2)).contains(&"ambiguous_transition"));
    }

    #[test]
    fn on_may_listen_to_any_node_but_listening_does_not_make_states_reachable() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  on \"a.done\" -> \"a.start\"\n  on \"a.ev\" -> \"a.start\"\n  on \"a.ghost\" -> \"a.start\"\n}}\nstate \"a.done\" desc=\"d\"\nevent \"a.ev\" desc=\"e\"\n"
            ),
        )]);
        let d = lint(&g);
        let ev: Vec<_> = d.iter().filter(|x| x.code == "unknown_event").collect();
        assert_eq!(ev.len(), 1, "{d:?}");
        assert!(ev[0].message.contains("a.ghost"));
        // a.done bir state: dinleniyor diye erişilebilir olmaz; a.ev event: olur
        assert!(
            d.iter()
                .any(|x| x.code == "unreachable" && x.id.as_deref() == Some("a.done")),
            "{d:?}"
        );
        assert!(
            !d.iter()
                .any(|x| x.code == "unreachable" && x.id.as_deref() == Some("a.ev")),
            "{d:?}"
        );
    }

    #[test]
    fn local_vars_need_no_definition_but_need_a_writer_in_group() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  sets \"retry\" \"+1\"\n  -> \"a.start\" when=\"retry < 3\"\n  -> \"a.ok\" when=\"retry >= 3\"\n}}\nstate \"a.ok\" desc=\"o\" {{\n  -> \"a.start\" when=\"orphan == 1\"\n  sets \"b.global\" \"1\"\n}}\n"
            ),
        )]);
        let d = lint(&g);
        assert!(
            !d.iter()
                .any(|x| x.code == "unknown_var" && x.message.contains("retry")),
            "{d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.code == "local_var_never_set" && x.message.contains("orphan")),
            "{d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.code == "unknown_var" && x.message.contains("b.global")),
            "{d:?}"
        );
    }

    #[test]
    fn requires_resolves_to_perm_check_with_deny_code_and_target() {
        let g = mk(&[
            (
                "a.kdl",
                &format!(
                    "{BASE}\ncall \"a.mid\" desc=\"m\" method=\"POST\" path=\"/x\" {{\n  requires \"deploy.trigger\"\n  requires \"ghost\"\n  returns 202 -> \"a.ok\"\n}}\nstate \"a.ok\" desc=\"o\"\nstate \"a.nf\" desc=\"404\"\n"
                ),
            ),
            (
                "perms.kdl",
                "perm \"deploy.trigger\" scope=\"project\" deny=404 desc=\"izin\" -> \"a.nf\"\n",
            ),
        ]);
        let d = lint(&g);
        assert!(g.checks.contains_key("perm:deploy.trigger"));
        assert!(
            !d.iter()
                .any(|x| x.code == "unknown_check" && x.message.contains("deploy.trigger")),
            "{d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.code == "unknown_perm" && x.message.contains("ghost")),
            "{d:?}"
        );
        let e = g
            .outgoing("a.mid")
            .iter()
            .find(|e| e.to == "a.nf")
            .expect("perm fail edge");
        assert_eq!(e.class, "fail");
        assert!(e.label.contains("404"), "{}", e.label);
        assert_eq!(g.perm_users("deploy.trigger").len(), 1);
    }

    #[test]
    fn outcomes_satisfy_fail_target_and_returns_rules() {
        let g = mk(&[
            (
                "a.kdl",
                &format!(
                    "{BASE}\ncall \"a.mid\" desc=\"m\" {{\n  check \"has_ip\" fail=409 outcome=\"toast: no ip\"\n  returns 500 outcome=\"toast: boom\"\n}}\n"
                ),
            ),
            ("checks.kdl", "check \"has_ip\" desc=\"ip\"\n"),
        ]);
        let d = lint(&g);
        assert!(!codes(&d).contains(&"check_no_fail_target"), "{d:?}");
        assert!(!codes(&d).contains(&"call_no_returns"), "{d:?}");
        assert!(!has_errors(&d), "{d:?}");
    }

    #[test]
    fn root_rules() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{ -> \"a.ok\" }}\nstate \"a.ok\" desc=\"o\"\nroot \"a.mid\" every=\"30s\"\n"
            ),
        )]);
        let d = lint(&g);
        assert!(
            d.iter()
                .any(|x| x.code == "root_has_incoming" && x.id.as_deref() == Some("a.mid")),
            "{d:?}"
        );
        assert!(
            !d.iter()
                .any(|x| x.code == "root_has_incoming" && x.id.as_deref() == Some("a.start")),
            "{d:?}"
        );
        let mut many = String::from(BASE);
        many.push_str("state \"a.mid\" desc=\"m\"\n");
        for i in 0..11 {
            many.push_str(&format!("state \"a.r{i}\" desc=\"r\"\nroot \"a.r{i}\"\n"));
        }
        assert!(codes(&lint(&mk(&[("a.kdl", &many)]))).contains(&"too_many_roots"));
    }

    #[test]
    fn local_var_scope_is_the_prefix_up_to_the_last_dot() {
        let g = mk(&[(
            "restore.kdl",
            "root \"restore.snapshot\"\nstate \"restore.snapshot\" desc=\"s\" {\n  sets \"done\" \"true\"\n  -> \"restore.applying\"\n}\nstate \"restore.applying\" desc=\"a\" {\n  -> \"restore.snapshot\" when=\"done == false\"\n}\n",
        )]);
        let d = lint(&g);
        assert!(!codes(&d).contains(&"local_var_never_set"), "{d:?}");
    }

    #[test]
    fn seq_steps_are_not_ambiguous_but_duplicates_warn() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  -> \"a.x\" seq=1\n  -> \"a.y\" seq=2\n}}\nstate \"a.x\" desc=\"x\" {{\n  -> \"a.y\" case=\"ok\"\n  -> \"a.mid\" case=\"ok\"\n  -> \"a.mid\" seq=1\n  -> \"a.y\" seq=1\n}}\nstate \"a.y\" desc=\"y\"\n"
            ),
        )]);
        let d = lint(&g);
        assert!(!codes(&d).contains(&"ambiguous_transition"), "{d:?}");
        assert!(
            d.iter()
                .any(|x| x.code == "duplicate_case" && x.id.as_deref() == Some("a.x")),
            "{d:?}"
        );
        assert!(
            d.iter()
                .any(|x| x.code == "duplicate_seq" && x.id.as_deref() == Some("a.x")),
            "{d:?}"
        );
    }

    #[test]
    fn definition_outcome_is_the_default_and_usage_outcome_suppresses_the_edge() {
        let g = mk(&[
            (
                "a.kdl",
                &format!(
                    "{BASE}\ncall \"a.mid\" desc=\"m\" {{\n  check \"has_ip\"\n  check \"other\" outcome=\"toast: mine\"\n  returns 200 -> \"a.start\"\n}}\nstate \"a.nf\" desc=\"n\"\n"
                ),
            ),
            (
                "checks.kdl",
                "check \"has_ip\" desc=\"ip\" outcome=\"toast: no ip\"\ncheck \"other\" desc=\"o\" -> \"a.nf\"\n",
            ),
        ]);
        let d = lint(&g);
        assert!(!codes(&d).contains(&"check_no_fail_target"), "{d:?}");
        // other: tanımda fail_to var ama kullanım outcome= dedi → a.nf'ye kenar yok
        assert!(g.outgoing("a.mid").iter().all(|e| e.to != "a.nf"), "{:?}", g.outgoing("a.mid"));
        assert_eq!(
            g.effective_outcome(&g.nodes["a.mid"].checks[0]).as_deref(),
            Some("toast: no ip")
        );
    }

    #[test]
    fn mixing_seq_and_case_on_one_node_warns() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  -> \"a.x\" seq=1\n  -> \"a.y\" case=\"y\"\n}}\nstate \"a.x\" desc=\"x\"\nstate \"a.y\" desc=\"y\"\n"
            ),
        )]);
        assert!(codes(&lint(&g)).contains(&"mixed_branching"));
    }

    #[test]
    fn arrow_outcome_guards_are_linted_like_edges() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  -> outcome=\"toast\" when=\"ghost > 1\"\n}}\n"
            ),
        )]);
        let d = lint(&g);
        assert!(
            d.iter()
                .any(|x| x.code == "local_var_never_set" && x.message.contains("ghost")),
            "{d:?}"
        );
    }

    #[test]
    fn entry_nodes_are_reachable_without_roots_and_periodic_roots_are_not_counted() {
        let mut src = String::from(BASE);
        src.push_str("state \"a.mid\" desc=\"m\"\ncall \"a.admin\" desc=\"cli\" entry=\"cli\" method=\"POST\" path=\"/x\" { returns 200 -> \"a.mid\" }\n");
        for i in 0..11 {
            src.push_str(&format!("state \"a.r{i}\" desc=\"r\"\nroot \"a.r{i}\" every=\"30s\"\n"));
        }
        let d = lint(&mk(&[("a.kdl", &src)]));
        assert!(!codes(&d).contains(&"unreachable"), "{d:?}");
        assert!(!codes(&d).contains(&"too_many_roots"), "{d:?}");
    }

    #[test]
    fn event_nodes_should_not_carry_transitions() {
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" {{\n  on \"a.ev\" -> \"a.start\"\n}}\nevent \"a.ev\" desc=\"e\" {{\n  -> \"a.start\"\n}}\n"
            ),
        )]);
        let d = lint(&g);
        assert!(
            d.iter()
                .any(|x| x.code == "event_has_transition" && x.id.as_deref() == Some("a.ev")),
            "{d:?}"
        );
    }

    #[test]
    fn src_attr_is_checked_against_the_repo() {
        let dir = std::env::temp_dir().join(format!("duflow-src-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/api.rs"), "pub fn create_instance() {}\n").unwrap();
        std::fs::write(dir.join("src/View.vue"), "<script setup lang=\"ts\">\nasync function submit() {}\n</script>\n").unwrap();
        let g = mk(&[(
            "a.kdl",
            &format!(
                "{BASE}\nstate \"a.mid\" desc=\"m\" src=\"src/api.rs#create_instance\"\nstate \"a.x\" desc=\"x\" src=\"src/api.rs#gone\"\nstate \"a.y\" desc=\"y\" src=\"src/nope.rs\"\nstate \"a.z\" desc=\"z\" src=\"src/api.rs:1\"\nstate \"a.w\" desc=\"w\" src=\"src/api.rs#create\"\nstate \"a.v\" desc=\"v\" src=\"src/View.vue#submit\"\nstate \"a.u\" desc=\"u\" src=\"src/View.vue\"\n"
            ),
        )]);
        let d = src_lint(&g, &dir);
        let by_id = |id: &str| {
            d.iter()
                .find(|x| x.id.as_deref() == Some(id))
                .map(|x| x.code)
        };
        assert_eq!(by_id("a.mid"), None);
        assert_eq!(by_id("a.x"), Some("src_symbol_missing"));
        assert_eq!(by_id("a.y"), Some("src_missing"));
        assert_eq!(by_id("a.z"), None);
        // alt-dize değil tam kelime: `create` ≠ `create_instance`
        assert_eq!(by_id("a.w"), Some("src_symbol_missing"));
        assert_eq!(by_id("a.v"), None);
        // kod dosyası, sembol/satır yok → uyar (sessiz geçme)
        assert_eq!(by_id("a.u"), Some("src_symbol_unchecked"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
