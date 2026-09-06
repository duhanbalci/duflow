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
        let same_edges = |e: &Edge, f: &Edge| {
            e.to == f.to && e.kind == f.kind && e.when == f.when && e.case == f.case && e.seq == f.seq
        };
        let edges_added: Vec<Edge> = y
            .edges
            .iter()
            .filter(|e| !x.edges.iter().any(|f| same_edges(e, f)))
            .cloned()
            .collect();
        let edges_removed: Vec<Edge> = x
            .edges
            .iter()
            .filter(|e| !y.edges.iter().any(|f| same_edges(e, f)))
            .cloned()
            .collect();
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
        if x.checks
            .iter()
            .map(|c| (&c.name, c.fail, &c.code, &c.to))
            .ne(y.checks.iter().map(|c| (&c.name, c.fail, &c.code, &c.to)))
        {
            fields.push("checks".into());
        }
        if x.sets
            .iter()
            .map(|s| (&s.var, &s.value))
            .ne(y.sets.iter().map(|s| (&s.var, &s.value)))
        {
            fields.push("sets".into());
        }
        if !edges_added.is_empty() || !edges_removed.is_empty() || !fields.is_empty() {
            d.changed.push(NodeChange {
                id: (*id).clone(),
                edges_added,
                edges_removed,
                fields,
                before: x.clone(),
                after: y.clone(),
            });
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

/// `duflow diff --stat`: sayılar, namespace bazlı. Ajan kendi kaybettiği kenarı burada görür.
#[derive(Debug, Default, Serialize)]
pub struct DiffStat {
    pub nodes_added: usize,
    pub nodes_removed: usize,
    pub nodes_changed: usize,
    pub edges_added: usize,
    pub edges_removed: usize,
    /// ns → (node +, node −, kenar +, kenar −)
    pub by_ns: std::collections::BTreeMap<String, (usize, usize, usize, usize)>,
}

impl DiffStat {
    pub fn to_text(&self) -> String {
        let mut s = format!(
            "nodes +{} -{} ~{} · edges +{} -{}\n",
            self.nodes_added,
            self.nodes_removed,
            self.nodes_changed,
            self.edges_added,
            self.edges_removed
        );
        if !self.by_ns.is_empty() {
            s.push_str(&format!(
                "{:<16} {:>7} {:>7} {:>7} {:>7}\n",
                "ns", "node+", "node-", "edge+", "edge-"
            ));
            for (ns, (na, nr, ea, er)) in &self.by_ns {
                s.push_str(&format!("{ns:<16} {na:>7} {nr:>7} {ea:>7} {er:>7}\n"));
            }
        }
        s
    }
}

fn ns_of(id: &str) -> String {
    id.split('.').next().unwrap_or(id).to_string()
}

impl GraphDiff {
    pub fn stat(&self) -> DiffStat {
        let mut st = DiffStat::default();
        for n in &self.added {
            st.nodes_added += 1;
            st.edges_added += n.edges.len();
            let e = st.by_ns.entry(ns_of(&n.id)).or_default();
            e.0 += 1;
            e.2 += n.edges.len();
        }
        for n in &self.removed {
            st.nodes_removed += 1;
            st.edges_removed += n.edges.len();
            let e = st.by_ns.entry(ns_of(&n.id)).or_default();
            e.1 += 1;
            e.3 += n.edges.len();
        }
        for c in &self.changed {
            st.nodes_changed += 1;
            st.edges_added += c.edges_added.len();
            st.edges_removed += c.edges_removed.len();
            let e = st.by_ns.entry(ns_of(&c.id)).or_default();
            e.2 += c.edges_added.len();
            e.3 += c.edges_removed.len();
        }
        st
    }

    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && self.vars_added.is_empty()
            && self.vars_removed.is_empty()
            && self.checks_added.is_empty()
            && self.checks_removed.is_empty()
    }

    pub fn to_text(&self) -> String {
        let mut s = String::new();
        for n in &self.added {
            s.push_str(&format!(
                "+ {} ({}) {}\n",
                n.id,
                n.kind.as_str(),
                n.desc.clone().unwrap_or_default()
            ));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_counts_nodes_and_edges_per_namespace() {
        let a = Graph::from_sources(&[(
            "a.kdl".into(),
            "root \"a.s\"\nstate \"a.s\" desc=\"s\" { -> \"a.t\" }\nstate \"a.t\" desc=\"t\" { -> \"b.x\" }\nstate \"b.x\" desc=\"x\"\n".into(),
        )])
        .unwrap();
        let b = Graph::from_sources(&[(
            "a.kdl".into(),
            "root \"a.s\"\nstate \"a.s\" desc=\"s\" { -> \"a.t\" case=\"go\" }\nstate \"a.t\" desc=\"t\"\nstate \"b.y\" desc=\"y\" { -> \"a.s\" }\n".into(),
        )])
        .unwrap();
        let st = diff(&a, &b).stat();
        assert_eq!((st.nodes_added, st.nodes_removed, st.nodes_changed), (1, 1, 2));
        // a.s: kenar case= kazandı (−1 +1); a.t: b.x kenarı gitti; b.x silindi (0 kenar); b.y geldi (1 kenar)
        assert_eq!((st.edges_added, st.edges_removed), (2, 2));
        assert_eq!(st.by_ns["a"], (0, 0, 1, 2));
        assert_eq!(st.by_ns["b"], (1, 1, 1, 0));
        assert!(st.to_text().starts_with("nodes +1 -1 ~2 · edges +2 -2\n"));
    }
}
