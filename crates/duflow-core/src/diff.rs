//! İki graf arası fark. ID bazlı; UI diff overlay ve `duflow diff` bunu kullanır.

use crate::graph::Graph;
use crate::model::*;
use serde::Serialize;
use std::collections::BTreeSet;

#[derive(Debug, Default, Serialize)]
pub struct GraphDiff {
    pub added: Vec<Node>,
    pub removed: Vec<Node>,
    pub changed: Vec<NodeChange>,
    pub vars_added: Vec<String>,
    pub vars_removed: Vec<String>,
    pub checks_added: Vec<String>,
    pub checks_removed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct NodeChange {
    pub id: String,
    pub edges_added: Vec<Edge>,
    pub edges_removed: Vec<Edge>,
    pub fields: Vec<String>,
    pub before: Node,
    pub after: Node,
}

pub fn diff(a: &Graph, b: &Graph) -> GraphDiff {
    let mut d = GraphDiff::default();
    let ida: BTreeSet<&String> = a.nodes.keys().collect();
    let idb: BTreeSet<&String> = b.nodes.keys().collect();
    for id in idb.difference(&ida) {
        d.added.push(b.nodes[*id].clone());
    }
    for id in ida.difference(&idb) {
        d.removed.push(a.nodes[*id].clone());
    }
    for id in ida.intersection(&idb) {
        let (x, y) = (&a.nodes[*id], &b.nodes[*id]);
        let same_edges = |e: &Edge, f: &Edge| e.to == f.to && e.kind == f.kind && e.when == f.when;
        let edges_added: Vec<Edge> = y.edges.iter().filter(|e| !x.edges.iter().any(|f| same_edges(e, f))).cloned().collect();
        let edges_removed: Vec<Edge> = x.edges.iter().filter(|e| !y.edges.iter().any(|f| same_edges(e, f))).cloned().collect();
        let mut fields = vec![];
        if x.kind != y.kind {
            fields.push("kind".into());
        }
        if x.layer != y.layer {
            fields.push("layer".into());
        }
        if x.desc != y.desc {
            fields.push("desc".into());
        }
        if x.attrs != y.attrs {
            fields.push("attrs".into());
        }
        if x.checks.iter().map(|c| (&c.name, c.fail, &c.code, &c.to)).ne(y.checks.iter().map(|c| (&c.name, c.fail, &c.code, &c.to))) {
            fields.push("checks".into());
        }
        if x.sets.iter().map(|s| (&s.var, &s.value)).ne(y.sets.iter().map(|s| (&s.var, &s.value))) {
            fields.push("sets".into());
        }
        if !edges_added.is_empty() || !edges_removed.is_empty() || !fields.is_empty() {
            d.changed.push(NodeChange { id: (*id).clone(), edges_added, edges_removed, fields, before: x.clone(), after: y.clone() });
        }
    }
    let va: BTreeSet<&String> = a.vars.keys().collect();
    let vb: BTreeSet<&String> = b.vars.keys().collect();
    d.vars_added = vb.difference(&va).map(|s| (*s).clone()).collect();
    d.vars_removed = va.difference(&vb).map(|s| (*s).clone()).collect();
    let ca: BTreeSet<&String> = a.checks.keys().collect();
    let cb: BTreeSet<&String> = b.checks.keys().collect();
    d.checks_added = cb.difference(&ca).map(|s| (*s).clone()).collect();
    d.checks_removed = ca.difference(&cb).map(|s| (*s).clone()).collect();
    d
}

impl GraphDiff {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.changed.is_empty() && self.vars_added.is_empty() && self.vars_removed.is_empty() && self.checks_added.is_empty() && self.checks_removed.is_empty()
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for n in &self.added {
            s.push_str(&format!("+ {} ({}) {}\n", n.id, n.kind.as_str(), n.desc.clone().unwrap_or_default()));
        }
        for n in &self.removed {
            s.push_str(&format!("- {} ({})\n", n.id, n.kind.as_str()));
        }
        for c in &self.changed {
            s.push_str(&format!("~ {}", c.id));
            if !c.fields.is_empty() {
                s.push_str(&format!(" [{}]", c.fields.join(", ")));
            }
            s.push('\n');
            for e in &c.edges_added {
                s.push_str(&format!("    + → {} {}\n", e.to, e.label()));
            }
            for e in &c.edges_removed {
                s.push_str(&format!("    - → {} {}\n", e.to, e.label()));
            }
        }
        for v in &self.vars_added {
            s.push_str(&format!("+ var {v}\n"));
        }
        for v in &self.vars_removed {
            s.push_str(&format!("- var {v}\n"));
        }
        for c in &self.checks_added {
            s.push_str(&format!("+ check {c}\n"));
        }
        for c in &self.checks_removed {
            s.push_str(&format!("- check {c}\n"));
        }
        if s.is_empty() {
            s.push_str("fark yok\n");
        }
        s
    }
}
