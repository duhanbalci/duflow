//! Yükleme (`flows/` dizini) ve indeksli graf.

use crate::model::*;
use crate::parse::{ParseError, parse_file};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("{0}")]
    Parse(#[from] ParseError),
    #[error("{0}: {1}")]
    Io(PathBuf, std::io::Error),
    #[error("flows dizini yok: {0}")]
    NoDir(PathBuf),
}

/// Çözümlenmiş, indeksli graf. Node'lar ID sırasında (deterministik çıktı).
#[derive(Debug, Default, Clone, Serialize)]
pub struct Graph {
    pub config: ProjectConfig,
    pub nodes: BTreeMap<String, Node>,
    pub vars: BTreeMap<String, VarDef>,
    pub checks: BTreeMap<String, CheckDef>,
    pub roots: Vec<RootDef>,
    pub views: Vec<ViewDef>,
    /// Aynı ID'nin ikinci tanımları (lint için).
    #[serde(skip)]
    pub duplicates: Vec<(String, String, usize)>,
    /// Gelen kenar indeksi: hedef → (kaynak, kenar indeksi)
    #[serde(skip)]
    pub incoming: HashMap<String, Vec<(String, usize)>>,
    /// Check fail kenarları da dahil edilmiş "efektif" çıkışlar (kaynak → hedef listesi).
    #[serde(skip)]
    pub out: HashMap<String, Vec<ResolvedEdge>>,
}

/// Graf üstünde yürürken kullanılan tek biçimli kenar: check fail hedefleri de burada.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedEdge {
    pub from: String,
    pub to: String,
    pub label: String,
    pub class: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    pub line: usize,
}

impl Graph {
    /// `dir` altındaki tüm `.kdl` dosyalarını yükler.
    pub fn load(dir: &Path) -> Result<Graph, LoadError> {
        if !dir.is_dir() {
            return Err(LoadError::NoDir(dir.into()));
        }
        let mut files = vec![];
        walk(dir, &mut files).map_err(|e| LoadError::Io(dir.into(), e))?;
        files.sort();
        let mut sources = vec![];
        for f in files {
            let src = std::fs::read_to_string(&f).map_err(|e| LoadError::Io(f.clone(), e))?;
            let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            sources.push((rel, src));
        }
        Graph::from_sources(&sources)
    }

    /// Bellekteki kaynaklardan (git rev diff'i, testler).
    pub fn from_sources(sources: &[(String, String)]) -> Result<Graph, LoadError> {
        let mut g = Graph::default();
        for (rel, src) in sources {
            let items = parse_file(rel, src)?;
            g.absorb(items);
        }
        g.reindex();
        Ok(g)
    }

    fn absorb(&mut self, items: FileItems) {
        if let Some(c) = items.config {
            self.config = c;
        }
        for n in items.nodes {
            if let Some(prev) = self.nodes.get(&n.id) {
                self.duplicates.push((n.id.clone(), n.file.clone(), n.line));
                let _ = prev;
                continue;
            }
            self.nodes.insert(n.id.clone(), n);
        }
        for v in items.vars {
            if self.vars.contains_key(&v.id) {
                self.duplicates.push((v.id.clone(), v.file.clone(), v.line));
                continue;
            }
            self.vars.insert(v.id.clone(), v);
        }
        for c in items.checks {
            if self.checks.contains_key(&c.id) {
                self.duplicates.push((c.id.clone(), c.file.clone(), c.line));
                continue;
            }
            self.checks.insert(c.id.clone(), c);
        }
        self.roots.extend(items.roots);
        self.views.extend(items.views);
    }

    pub fn reindex(&mut self) {
        self.incoming.clear();
        self.out.clear();
        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        for id in ids {
            let edges = self.resolved_edges(&id);
            for (i, e) in edges.iter().enumerate() {
                self.incoming.entry(e.to.clone()).or_default().push((id.clone(), i));
            }
            self.out.insert(id, edges);
        }
    }

    /// Bir node'un çıkışları: gerçek kenarlar + check fail hedefleri (kullanım ya da tanımdaki).
    fn resolved_edges(&self, id: &str) -> Vec<ResolvedEdge> {
        let n = &self.nodes[id];
        let mut out: Vec<ResolvedEdge> = n
            .edges
            .iter()
            .map(|e| ResolvedEdge { from: id.into(), to: e.to.clone(), label: e.label(), class: e.class(), when: e.when.clone(), line: e.line })
            .collect();
        for c in &n.checks {
            let to = c.to.clone().or_else(|| self.checks.get(&c.name).and_then(|d| d.fail_to.clone()));
            if let Some(to) = to {
                let label = match (c.fail, &c.code) {
                    (Some(s), Some(code)) => format!("{} ✗ {s} {code}", c.name),
                    (Some(s), None) => format!("{} ✗ {s}", c.name),
                    _ => format!("{} ✗", c.name),
                };
                out.push(ResolvedEdge { from: id.into(), to, label, class: "fail", when: None, line: c.line });
            }
        }
        out
    }

    pub fn outgoing(&self, id: &str) -> &[ResolvedEdge] {
        self.out.get(id).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn incoming_edges(&self, id: &str) -> Vec<&ResolvedEdge> {
        self.incoming.get(id).map(|v| v.iter().map(|(from, i)| &self.out[from][*i]).collect()).unwrap_or_default()
    }

    pub fn root_ids(&self) -> Vec<&str> {
        self.roots.iter().map(|r| r.id.as_str()).collect()
    }

    /// Root'lardan erişilebilen küme.
    pub fn reachable_from_roots(&self) -> BTreeSet<String> {
        self.reach_forward(&self.root_ids())
    }

    pub fn reach_forward(&self, starts: &[&str]) -> BTreeSet<String> {
        let mut seen = BTreeSet::new();
        let mut q: VecDeque<String> = starts.iter().map(|s| s.to_string()).collect();
        while let Some(id) = q.pop_front() {
            if !self.nodes.contains_key(&id) || !seen.insert(id.clone()) {
                continue;
            }
            for e in self.outgoing(&id) {
                q.push_back(e.to.clone());
            }
            for ev in self.listened_events(&id) {
                q.push_back(ev);
            }
        }
        seen
    }

    /// `on "ev" -> …` ile dinlenen event ID'leri. Dinleyen erişilebilirse event de erişilebilir sayılır.
    pub fn listened_events(&self, id: &str) -> Vec<String> {
        self.nodes.get(id).map(|n| n.edges.iter().filter_map(|e| match &e.kind { EdgeKind::On { event } => Some(event.clone()), _ => None }).collect()).unwrap_or_default()
    }

    /// `from`'dan `to`'ya en kısa yol (BFS). Bulunamazsa None.
    pub fn shortest_path(&self, from: &str, to: &str) -> Option<Vec<String>> {
        if from == to {
            return Some(vec![from.into()]);
        }
        let mut prev: HashMap<String, String> = HashMap::new();
        let mut q = VecDeque::from([from.to_string()]);
        let mut seen = BTreeSet::from([from.to_string()]);
        while let Some(id) = q.pop_front() {
            for e in self.outgoing(&id) {
                if seen.insert(e.to.clone()) {
                    prev.insert(e.to.clone(), id.clone());
                    if e.to == to {
                        let mut p = vec![to.to_string()];
                        let mut c = to.to_string();
                        while let Some(pp) = prev.get(&c) {
                            p.push(pp.clone());
                            c = pp.clone();
                        }
                        p.reverse();
                        return Some(p);
                    }
                    q.push_back(e.to.clone());
                }
            }
        }
        None
    }

    /// Herhangi bir root'tan hedefe en kısa yol.
    pub fn path_from_roots(&self, to: &str) -> Option<Vec<String>> {
        self.root_ids().iter().filter_map(|r| self.shortest_path(r, to)).min_by_key(|p| p.len())
    }

    /// İki node arası en fazla `max` basit yol (DFS, uzunluk sınırı `depth`).
    pub fn all_paths(&self, from: &str, to: &str, max: usize, depth: usize) -> Vec<Vec<String>> {
        let mut res = vec![];
        let mut stack: Vec<String> = vec![from.into()];
        let mut on: BTreeSet<String> = BTreeSet::from([from.to_string()]);
        self.dfs_paths(from, to, max, depth, &mut stack, &mut on, &mut res);
        res
    }

    fn dfs_paths(&self, cur: &str, to: &str, max: usize, depth: usize, stack: &mut Vec<String>, on: &mut BTreeSet<String>, res: &mut Vec<Vec<String>>) {
        if res.len() >= max || stack.len() > depth {
            return;
        }
        if cur == to && stack.len() > 1 {
            res.push(stack.clone());
            return;
        }
        // aynı hedefe birden çok kenar tek komşu sayılır
        let mut targets: Vec<&str> = self.outgoing(cur).iter().map(|e| e.to.as_str()).collect();
        targets.dedup();
        for t in targets {
            let e = ResolvedEdge { from: String::new(), to: t.into(), label: String::new(), class: "", when: None, line: 0 };
            if on.insert(e.to.clone()) {
                stack.push(e.to.clone());
                self.dfs_paths(&e.to, to, max, depth, stack, on, res);
                stack.pop();
                on.remove(&e.to);
            }
        }
    }

    /// Bir değişkeni yazan node'lar.
    pub fn var_writers(&self, var: &str) -> Vec<(&Node, &SetVar)> {
        self.nodes.values().flat_map(|n| n.sets.iter().filter(|s| s.var == var).map(move |s| (n, s))).collect()
    }

    /// Bir değişkeni okuyan yerler: (node, açıklama). Guard'lar ve check tanımları.
    pub fn var_readers(&self, var: &str) -> Vec<(String, String)> {
        let mut r = vec![];
        for n in self.nodes.values() {
            for e in &n.edges {
                if let Some(w) = &e.when {
                    if crate::expr::idents(w).iter().any(|i| i == var) {
                        r.push((n.id.clone(), format!("when {w}")));
                    }
                }
            }
        }
        for c in self.checks.values() {
            if c.reads.iter().any(|x| x == var) {
                r.push((format!("check:{}", c.id), "reads".into()));
            }
        }
        r
    }

    /// Bir check'i kullanan call'lar.
    pub fn check_users(&self, check: &str) -> Vec<&Node> {
        self.nodes.values().filter(|n| n.checks.iter().any(|c| c.name == check)).collect()
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let p = entry?.path();
        if p.is_dir() {
            walk(&p, out)?;
        } else if p.extension().is_some_and(|e| e == "kdl") {
            out.push(p);
        }
    }
    Ok(())
}
