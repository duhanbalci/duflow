//! UI'ın gömdüğü JSON şeması. Versiyonlu; UI yalnız bunu bilir.

use crate::diff::GraphDiff;
use crate::graph::Graph;
use crate::model::*;
use serde::Serialize;

pub const SCHEMA_VERSION: u32 = 2;

#[derive(Serialize)]
pub struct Export<'a> {
    pub schema: u32,
    pub project: &'a ProjectConfig,
    pub nodes: Vec<ExportNode<'a>>,
    pub vars: Vec<&'a VarDef>,
    pub checks: Vec<&'a CheckDef>,
    pub roots: Vec<&'a str>,
    /// Periyodik root'lar: id → `every`
    pub every: std::collections::BTreeMap<&'a str, &'a str>,
    pub perms: Vec<&'a PermDef>,
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
    /// Gelen kenarlar (UI tersini hesaplamasın)
    #[serde(rename = "in")]
    pub incoming: Vec<ExportIn<'a>>,
    pub checks: Vec<&'a CheckUse>,
    pub sets: Vec<&'a SetVar>,
    /// Node'suz sonlar: check `outcome=` ve `returns ... outcome=`
    pub outcomes: Vec<ExportOutcome>,
    pub file: String,
}

#[derive(Serialize)]
pub struct ExportIn<'a> {
    pub from: &'a str,
    pub label: &'a str,
    pub class: &'static str,
}

#[derive(Serialize)]
pub struct ExportOutcome {
    pub label: String,
    pub text: String,
    pub class: &'static str,
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
            out: g
                .outgoing(&n.id)
                .iter()
                .map(|e| ExportEdge {
                    to: &e.to,
                    label: &e.label,
                    class: e.class,
                    when: e.when.as_deref(),
                })
                .collect(),
            incoming: g
                .incoming_edges(&n.id)
                .into_iter()
                .map(|e| ExportIn {
                    from: &e.from,
                    label: &e.label,
                    class: e.class,
                })
                .collect(),
            checks: n.checks.iter().collect(),
            sets: n.sets.iter().collect(),
            outcomes: n
                .checks
                .iter()
                .filter_map(|c| {
                    c.outcome.as_ref().map(|t| ExportOutcome {
                        label: match c.fail {
                            Some(f) => format!("{} ✗ {f}", c.name),
                            None => format!("{} ✗", c.name),
                        },
                        text: t.clone(),
                        class: "fail",
                    })
                })
                .chain(n.outcomes.iter().map(|o| ExportOutcome {
                    label: o.label(),
                    text: o.text.clone(),
                    class: if o.status.is_some_and(|s| s >= 400) {
                        "fail"
                    } else {
                        ""
                    },
                }))
                .collect(),
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
        edges_removed: d
            .changed
            .iter()
            .flat_map(|c| {
                c.edges_removed
                    .iter()
                    .map(move |e| (c.id.clone(), e.to.clone(), e.label()))
            })
            .collect(),
        edges_added: d
            .changed
            .iter()
            .flat_map(|c| {
                c.edges_added
                    .iter()
                    .map(move |e| (c.id.clone(), e.to.clone(), e.label()))
            })
            .collect(),
    });
    Export {
        schema: SCHEMA_VERSION,
        project: &g.config,
        nodes,
        vars: g.vars.values().collect(),
        checks: g.checks.values().collect(),
        roots: g.roots.iter().map(|r| r.id.as_str()).collect(),
        every: g
            .roots
            .iter()
            .filter_map(|r| r.every.as_deref().map(|e| (r.id.as_str(), e)))
            .collect(),
        perms: g.perms.values().collect(),
        views: g.views.iter().collect(),
        diff,
    }
}

/// Diff overlay için: eski grafta silinen node'lara giden kenarları doldurur.
pub fn fill_removed_sources(exp: &mut Export<'_>, old: &Graph) {
    if let Some(d) = exp.diff.as_mut() {
        for r in &mut d.removed {
            r.from = old
                .incoming_edges(&r.id)
                .into_iter()
                .map(|e| (e.from.clone(), e.label.clone()))
                .collect();
        }
        // silinen kenarlar da: eski grafta var, yenide yok — changed listesinde zaten
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_has_incoming_outcomes_and_periodic_roots() {
        let g = Graph::from_sources(&[(
            "a.kdl".into(),
            "root \"a.start\" every=\"30s\"\nstate \"a.start\" desc=\"s\" { -> \"a.end\" case=\"go\" }\ncall \"a.end\" desc=\"e\" {\n  check \"c\" fail=409 outcome=\"toast: no\"\n  returns 500 outcome=\"boom\"\n  returns 200 -> \"a.start\"\n}\ncheck \"c\"\n".into(),
        )])
        .unwrap();
        let e = export(&g, None);
        assert_eq!(e.schema, 2);
        let v = serde_json::to_value(&e).unwrap();
        let end = v["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == "a.end")
            .unwrap();
        assert_eq!(end["in"][0]["from"], "a.start");
        assert_eq!(end["in"][0]["label"], "case go");
        let outs = end["outcomes"].as_array().unwrap();
        assert_eq!(outs.len(), 2);
        assert!(
            outs.iter()
                .any(|o| o["label"] == "c ✗ 409" && o["text"] == "toast: no"),
            "{outs:?}"
        );
        assert!(
            outs.iter()
                .any(|o| o["label"] == "500" && o["text"] == "boom")
        );
        assert_eq!(v["every"]["a.start"], "30s");
        assert_eq!(v["roots"][0], "a.start");
    }
}
