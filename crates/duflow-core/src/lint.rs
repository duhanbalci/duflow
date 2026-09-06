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
        if !reachable.contains(&n.id) && !root_set.contains(n.id.as_str()) {
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
            if let EdgeKind::On { event } = &e.kind {
                if !g.nodes.get(event).is_some_and(|ev| ev.kind == Kind::Event) {
                    push(
                        &mut d,
                        Level::Warning,
                        "unknown_event",
                        &n.file,
                        e.line,
                        Some(&n.id),
                        format!("`on {event}`: not defined as an `event`"),
                    );
                }
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
                for v in expr::idents(w) {
                    if !g.vars.contains_key(&v) {
                        push(
                            &mut d,
                            Level::Error,
                            "unknown_var",
                            &n.file,
                            e.line,
                            Some(&n.id),
                            format!("guard `{w}`: `{v}` is not a defined var"),
                        );
                    }
                }
            }
        }
        // check kullanımları
        for c in &n.checks {
            if n.kind != Kind::Call {
                push(
                    &mut d,
                    Level::Warning,
                    "check_outside_call",
                    &n.file,
                    c.line,
                    Some(&n.id),
                    "`check` only makes sense inside a `call`".into(),
                );
            }
            let def = g.checks.get(&c.name);
            if def.is_none() {
                push(
                    &mut d,
                    Level::Error,
                    "unknown_check",
                    &n.file,
                    c.line,
                    Some(&n.id),
                    format!("`check {}`: not defined", c.name),
                );
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
            if !g.vars.contains_key(&s.var) {
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
                if n.edges
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
            _ => {}
        }
        // guard'sız birden fazla koşulsuz `->`
        let plain_unguarded = n
            .edges
            .iter()
            .filter(|e| matches!(e.kind, EdgeKind::Plain) && e.when.is_none())
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
    // check tanımları
    for c in g.checks.values() {
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
        }
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
