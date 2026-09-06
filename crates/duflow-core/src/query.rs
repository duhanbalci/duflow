//! Sorgular: brief, prereq, search.

use crate::graph::{Graph, ResolvedEdge};
use crate::model::*;
use serde::Serialize;
use std::collections::{BTreeSet, VecDeque};

/// AI için tek parça özet.
#[derive(Debug, Serialize)]
pub struct Brief {
    pub node: Node,
    /// Root'tan en kısa yol (varsa)
    pub path_from_root: Option<Vec<String>>,
    pub incoming: Vec<ResolvedEdge>,
    pub outgoing: Vec<ResolvedEdge>,
    pub reads: Vec<String>,
    pub writes: Vec<SetVar>,
    pub checks: Vec<CheckDef>,
    pub group_siblings: Vec<String>,
}

pub fn brief(g: &Graph, id: &str) -> Option<Brief> {
    let node = g.nodes.get(id)?.clone();
    let mut reads: BTreeSet<String> = BTreeSet::new();
    for e in &node.edges {
        if let Some(w) = &e.when {
            reads.extend(crate::expr::idents(w));
        }
    }
    let checks: Vec<CheckDef> = node
        .checks
        .iter()
        .filter_map(|c| g.checks.get(&c.name).cloned())
        .collect();
    for c in &checks {
        reads.extend(c.reads.iter().cloned());
    }
    let grp = group_of(id);
    let group_siblings = g
        .nodes
        .keys()
        .filter(|k| *k != id && group_of(k) == grp)
        .cloned()
        .collect();
    Some(Brief {
        path_from_root: g.path_from_roots(id),
        incoming: g.incoming_edges(id).into_iter().cloned().collect(),
        outgoing: g.outgoing(id).to_vec(),
        reads: reads.into_iter().collect(),
        writes: node.sets.clone(),
        checks,
        group_siblings,
        node,
    })
}

impl Brief {
    /// Markdown metni; `--json` yoksa bu basılır.
    pub fn to_markdown(&self, g: &Graph) -> String {
        let n = &self.node;
        let mut s = String::new();
        s.push_str(&format!("# {} ({} · {})\n", n.id, n.kind.as_str(), n.layer));
        if let Some(d) = &n.desc {
            s.push_str(&format!("{d}\n"));
        }
        s.push_str(&format!("{}:{}\n", n.file, n.line));
        for (k, v) in &n.attrs {
            s.push_str(&format!("- {k}: {v}\n"));
        }
        if let Some(p) = &self.path_from_root {
            s.push_str(&format!(
                "\n## How to get here ({} steps)\n{}\n",
                p.len() - 1,
                p.join(" → ")
            ));
        } else {
            s.push_str("\n## How to get here\n(not reachable from a root)\n");
        }
        if !self.incoming.is_empty() {
            s.push_str("\n## Incoming\n");
            for e in &self.incoming {
                s.push_str(&format!("- {} {}\n", e.from, tag(&e.label)));
            }
        }
        s.push_str("\n## Outgoing\n");
        if self.outgoing.is_empty() {
            s.push_str("(none)\n");
        }
        for e in &self.outgoing {
            let d = g
                .nodes
                .get(&e.to)
                .and_then(|t| t.desc.clone())
                .unwrap_or_default();
            s.push_str(&format!("- {}→ {} — {d}\n", tag_post(&e.label), e.to));
        }
        if !self.checks.is_empty() || !n.checks.is_empty() {
            s.push_str("\n## Checks\n");
            for c in &n.checks {
                let def = self.checks.iter().find(|d| d.id == c.name);
                let desc = def.and_then(|d| d.desc.clone()).unwrap_or_default();
                let fail = match (c.fail, &c.code) {
                    (Some(f), Some(code)) => format!(" ✗ {f} {code}"),
                    (Some(f), None) => format!(" ✗ {f}"),
                    _ => String::new(),
                };
                s.push_str(&format!("- {}{fail} — {desc}\n", c.name));
            }
        }
        if !self.reads.is_empty() || !self.writes.is_empty() {
            s.push_str("\n## Variables\n");
            for r in &self.reads {
                let src = g
                    .vars
                    .get(r)
                    .map(|v| v.source.clone().unwrap_or_else(|| "sets".into()))
                    .unwrap_or_else(|| "?".into());
                s.push_str(&format!("- reads {r} ({src})\n"));
            }
            for w in &self.writes {
                s.push_str(&format!("- sets {} {}\n", w.var, w.value));
            }
        }
        if !self.group_siblings.is_empty() {
            s.push_str(&format!(
                "\n## Same group\n{}\n",
                self.group_siblings.join(", ")
            ));
        }
        s
    }
}

fn tag(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("[{label}]")
    }
}
fn tag_post(label: &str) -> String {
    if label.is_empty() {
        String::new()
    } else {
        format!("[{label}] ")
    }
}

/// Ön koşul zinciri: hedefe geriye doğru BFS; her adımda geçilmesi gereken guard/check'ler.
#[derive(Debug, Serialize)]
pub struct Prereq {
    pub target: String,
    /// Root'tan en kısa yol
    pub path: Option<Vec<String>>,
    /// Yol üstündeki her kenar için gereken koşul
    pub steps: Vec<PrereqStep>,
    /// Hedefe geriye doğru `depth` derinlikte tüm öncüller
    pub ancestors: Vec<String>,
    /// Yol üstünde okunan değişkenler ve onları kim yazıyor
    pub vars: Vec<VarChain>,
}

#[derive(Debug, Serialize)]
pub struct PrereqStep {
    pub from: String,
    pub to: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct VarChain {
    pub var: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    pub writers: Vec<String>,
}

pub fn prereq(g: &Graph, id: &str, depth: usize) -> Option<Prereq> {
    g.nodes.get(id)?;
    let path = g.path_from_roots(id);
    let mut steps = vec![];
    let mut vars: BTreeSet<String> = BTreeSet::new();
    if let Some(p) = &path {
        for w in p.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            // aynı hedefe giden kenarlardan en az kısıtlı olanı (guard'sız varsa o)
            let edges: Vec<&ResolvedEdge> = g.outgoing(a).iter().filter(|e| &e.to == b).collect();
            let e = edges
                .iter()
                .find(|e| e.when.is_none())
                .or(edges.first())
                .copied()?;
            let checks: Vec<String> = g.nodes[a].checks.iter().map(|c| c.name.clone()).collect();
            if let Some(wh) = &e.when {
                vars.extend(crate::expr::idents(wh));
            }
            for c in &checks {
                if let Some(d) = g.checks.get(c) {
                    vars.extend(d.reads.iter().cloned());
                }
            }
            steps.push(PrereqStep {
                from: a.clone(),
                to: b.clone(),
                label: e.label.clone(),
                when: e.when.clone(),
                checks,
            });
        }
    }
    // geriye BFS
    let mut ancestors = BTreeSet::new();
    let mut q = VecDeque::from([(id.to_string(), 0usize)]);
    while let Some((cur, d)) = q.pop_front() {
        if d >= depth {
            continue;
        }
        for e in g.incoming_edges(&cur) {
            if ancestors.insert(e.from.clone()) {
                q.push_back((e.from.clone(), d + 1));
            }
        }
    }
    let vars = vars
        .into_iter()
        .map(|v| VarChain {
            source: g.vars.get(&v).and_then(|d| d.source.clone()),
            writers: g
                .var_writers(&v)
                .into_iter()
                .map(|(n, s)| format!("{} ({})", n.id, s.value))
                .collect(),
            var: v,
        })
        .collect();
    Some(Prereq {
        target: id.into(),
        path,
        steps,
        ancestors: ancestors.into_iter().collect(),
        vars,
    })
}

impl Prereq {
    pub fn to_markdown(&self) -> String {
        let mut s = format!("# Prerequisites: {}\n", self.target);
        match &self.path {
            Some(p) => s.push_str(&format!(
                "\nPath ({} steps): {}\n",
                p.len() - 1,
                p.join(" → ")
            )),
            None => s.push_str("\nNot reachable from a root.\n"),
        }
        if !self.steps.is_empty() {
            s.push_str("\n## Steps\n");
            for (i, st) in self.steps.iter().enumerate() {
                let mut line = format!("{}. {} → {}", i + 1, st.from, st.to);
                if !st.label.is_empty() {
                    line.push_str(&format!(" [{}]", st.label));
                }
                if !st.checks.is_empty() {
                    line.push_str(&format!(" · must pass: {}", st.checks.join(", ")));
                }
                s.push_str(&line);
                s.push('\n');
            }
        }
        if !self.vars.is_empty() {
            s.push_str("\n## Required variables\n");
            for v in &self.vars {
                let src = v
                    .source
                    .clone()
                    .map(|x| format!("source {x}"))
                    .unwrap_or_else(|| format!("set by: {}", v.writers.join(", ")));
                s.push_str(&format!("- {} — {src}\n", v.var));
            }
        }
        if !self.ancestors.is_empty() {
            s.push_str(&format!("\n## Ancestors\n{}\n", self.ancestors.join(", ")));
        }
        s
    }
}

#[derive(Debug, Serialize)]
pub struct Hit {
    pub kind: String,
    pub id: String,
    pub desc: String,
    pub score: i64,
    /// Atlanacak node (check → kullanan call, var → yazan node)
    pub go: Option<String>,
}

/// Basit subsequence fuzzy: bitişik eşleşme + prefix bonusu.
pub fn fuzzy(hay: &str, needle: &str) -> Option<i64> {
    let hay: Vec<char> = hay.to_lowercase().chars().collect();
    let mut i = 0usize;
    let mut score = 0i64;
    let mut last: i64 = -2;
    for ch in needle.to_lowercase().chars() {
        let j = hay[i..].iter().position(|&c| c == ch)? + i;
        score += if j as i64 == last + 1 { 3 } else { 1 };
        if j == 0 {
            score += 4;
        }
        last = j as i64;
        i = j + 1;
    }
    Some(score * 10 - hay.len() as i64 / 4)
}

pub fn search(g: &Graph, q: &str, limit: usize) -> Vec<Hit> {
    let mut hits = vec![];
    for n in g.nodes.values() {
        let a = fuzzy(&n.id, q).map(|s| s + 20);
        let b = n.desc.as_deref().and_then(|d| fuzzy(d, q));
        if let Some(s) = a.into_iter().chain(b).max() {
            hits.push(Hit {
                kind: n.kind.as_str().into(),
                id: n.id.clone(),
                desc: n.desc.clone().unwrap_or_default(),
                score: s,
                go: Some(n.id.clone()),
            });
        }
    }
    for c in g.checks.values() {
        if let Some(s) = fuzzy(&c.id, q) {
            hits.push(Hit {
                kind: "check".into(),
                id: c.id.clone(),
                desc: c.desc.clone().unwrap_or_default(),
                score: s + 10,
                go: g.check_users(&c.id).first().map(|n| n.id.clone()),
            });
        }
    }
    for v in g.vars.values() {
        if let Some(s) = fuzzy(&v.id, q) {
            hits.push(Hit {
                kind: "var".into(),
                id: v.id.clone(),
                desc: v.source.clone().unwrap_or_else(|| "sets".into()),
                score: s + 10,
                go: g.var_writers(&v.id).first().map(|(n, _)| n.id.clone()),
            });
        }
    }
    hits.sort_by(|a, b| b.score.cmp(&a.score).then(a.id.cmp(&b.id)));
    hits.truncate(limit);
    hits
}
