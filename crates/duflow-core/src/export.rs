//! UI'ın gömdüğü JSON şeması. Versiyonlu; UI yalnız bunu bilir.

use crate::diff::GraphDiff;
use crate::graph::Graph;
use crate::model::*;
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Serialize)]
pub struct Export<'a> {
    pub schema: u32,
    pub project: &'a ProjectConfig,
    pub nodes: Vec<ExportNode<'a>>,
    pub vars: Vec<&'a VarDef>,
    pub checks: Vec<&'a CheckDef>,
    pub roots: Vec<&'a str>,
    pub views: Vec<&'a ViewDef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diff: Option<ExportDiff>,
}

#[derive(Serialize)]
pub struct ExportNode<'a> {
    pub id: &'a str,
    pub kind: Kind,
    pub layer: &'a str,
    pub desc: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<&'a str>,
    pub attrs: &'a std::collections::BTreeMap<String, String>,
    pub out: Vec<ExportEdge<'a>>,
    pub checks: Vec<&'a CheckUse>,
    pub sets: Vec<&'a SetVar>,
    pub file: String,
}

#[derive(Serialize)]
pub struct ExportEdge<'a> {
    pub to: &'a str,
    pub label: &'a str,
    pub class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<&'a str>,
}

#[derive(Serialize, Default)]
pub struct ExportDiff {
    pub added: Vec<String>,
    pub removed: Vec<RemovedNode>,
    pub changed: Vec<String>,
    /// Silinen kenarlar: (from, to, label)
    pub edges_removed: Vec<(String, String, String)>,
    pub edges_added: Vec<(String, String, String)>,
}

#[derive(Serialize)]
pub struct RemovedNode {
    pub id: String,
    pub kind: Kind,
    pub layer: String,
    pub desc: String,
    /// Silinen node'a giden eski kenarlar (from, label), UI "artık yok" adayı çizer
    pub from: Vec<(String, String)>,
}

pub fn export<'a>(g: &'a Graph, diff: Option<&GraphDiff>) -> Export<'a> {
    let nodes = g
        .nodes
        .values()
        .map(|n| ExportNode {
            id: &n.id,
            kind: n.kind,
            layer: &n.layer,
            desc: n.desc.as_deref().unwrap_or(""),
            doc: n.doc.as_deref(),
            attrs: &n.attrs,
            out: g.outgoing(&n.id).iter().map(|e| ExportEdge { to: &e.to, label: &e.label, class: e.class, when: e.when.as_deref() }).collect(),
            checks: n.checks.iter().collect(),
            sets: n.sets.iter().collect(),
            file: format!("{}:{}", n.file, n.line),
        })
        .collect();
    let diff = diff.map(|d| ExportDiff {
        added: d.added.iter().map(|n| n.id.clone()).collect(),
        removed: d
            .removed
            .iter()
            .map(|n| RemovedNode {
                id: n.id.clone(),
                kind: n.kind,
                layer: n.layer.clone(),
                desc: n.desc.clone().unwrap_or_default(),
                from: vec![],
            })
            .collect(),
        changed: d.changed.iter().map(|c| c.id.clone()).collect(),
        edges_removed: d.changed.iter().flat_map(|c| c.edges_removed.iter().map(move |e| (c.id.clone(), e.to.clone(), e.label()))).collect(),
        edges_added: d.changed.iter().flat_map(|c| c.edges_added.iter().map(move |e| (c.id.clone(), e.to.clone(), e.label()))).collect(),
    });
    Export { schema: SCHEMA_VERSION, project: &g.config, nodes, vars: g.vars.values().collect(), checks: g.checks.values().collect(), roots: g.roots.iter().map(|r| r.id.as_str()).collect(), views: g.views.iter().collect(), diff }
}

/// Diff overlay için: eski grafta silinen node'lara giden kenarları doldurur.
pub fn fill_removed_sources(exp: &mut Export<'_>, old: &Graph) {
    if let Some(d) = exp.diff.as_mut() {
        for r in &mut d.removed {
            r.from = old.incoming_edges(&r.id).into_iter().map(|e| (e.from.clone(), e.label.clone())).collect();
        }
        // silinen kenarlar da: eski grafta var, yenide yok — changed listesinde zaten
    }
}
