//! Biçim koruyan düzenleme: dosyalar `kdl` ile parse edilir, yalnız ilgili node değişir,
//! yorumlar ve boşluklar korunur. Tüm işlemler önce bellekte, sonra tek seferde diske.

use crate::model::{file_for_id, is_valid_id};
use kdl::{KdlDocument, KdlEntry, KdlEntryFormat, KdlNode, KdlNodeFormat, KdlValue};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EditError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{file}: KDL parse hatası: {msg}")]
    Parse { file: String, msg: String },
    #[error("`{0}` bulunamadı")]
    NotFound(String),
    #[error("`{0}` zaten var ({1})")]
    Exists(String, String),
    #[error("geçersiz ID `{0}`")]
    InvalidId(String),
    #[error("`{0}` şuralardan referans alıyor: {1} (--force ile referanslar da silinir)")]
    Referenced(String, String),
    #[error("{0}")]
    Bad(String),
}

/// JSON ile de verilebilen işlem listesi (`duflow apply -`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    /// Yeni node. `kind`: state|action|call|event. `attrs`: layer, desc, method, path...
    AddNode {
        kind: String,
        id: String,
        #[serde(default)]
        attrs: BTreeMap<String, String>,
        /// Çocuk satırları ham KDL: `-> "x" when="a > 1"`, `check "perm" fail=404`, `sets "v" "+1"`
        #[serde(default)]
        children: Vec<String>,
    },
    /// Attribute ata (`desc`, `layer`, `method`...). Boş değer siler.
    SetAttr { id: String, key: String, value: String },
    /// Ham KDL çocuk satırı ekle.
    AddChild { id: String, line: String },
    /// Hedefi `to` olan çocukları sil (`->`, `on`, `returns`, `calls`, `check ... -> to`).
    RmEdge { id: String, to: String },
    /// Belirli bir çocuk satırını (ad + ilk argüman) sil: örn. `check` + `perm:x`, `sets` + `v`.
    RmChild { id: String, name: String, arg: String },
    /// Node/var/check ID'sini yeniden adlandır; tüm referanslar güncellenir, gerekiyorsa dosya taşınır.
    Rename { from: String, to: String },
    /// Node sil. Referans varsa `force` olmadan reddedilir.
    RmNode {
        id: String,
        #[serde(default)]
        force: bool,
    },
    AddVar {
        id: String,
        #[serde(default)]
        attrs: BTreeMap<String, String>,
    },
    AddCheck {
        id: String,
        #[serde(default)]
        attrs: BTreeMap<String, String>,
    },
    AddRoot { id: String },
}

/// Bellekte tutulan dosya kümesi.
pub struct Workspace {
    dir: PathBuf,
    docs: BTreeMap<String, KdlDocument>,
    dirty: BTreeMap<String, bool>,
}

const INDENT: &str = "  ";
const NODE_KINDS: [&str; 4] = ["state", "action", "call", "event"];

impl Workspace {
    pub fn open(dir: &Path) -> Result<Self, EditError> {
        let mut docs = BTreeMap::new();
        let mut files = vec![];
        if dir.is_dir() {
            walk(dir, &mut files)?;
        }
        for f in files {
            let rel = f.strip_prefix(dir).unwrap_or(&f).to_string_lossy().replace('\\', "/");
            let src = std::fs::read_to_string(&f)?;
            let doc: KdlDocument = src.parse().map_err(|e: kdl::KdlError| EditError::Parse { file: rel.clone(), msg: e.diagnostics.first().and_then(|d| d.message.clone()).unwrap_or_default() })?;
            docs.insert(rel, doc);
        }
        Ok(Self { dir: dir.into(), docs, dirty: BTreeMap::new() })
    }

    pub fn apply_all(&mut self, ops: &[Op]) -> Result<(), EditError> {
        for op in ops {
            self.apply(op)?;
        }
        Ok(())
    }

    pub fn apply(&mut self, op: &Op) -> Result<(), EditError> {
        match op {
            Op::AddNode { kind, id, attrs, children } => {
                if !NODE_KINDS.contains(&kind.as_str()) {
                    return Err(EditError::Bad(format!("bilinmeyen kind `{kind}`")));
                }
                if !is_valid_id(id) {
                    return Err(EditError::InvalidId(id.clone()));
                }
                if let Some((f, _)) = self.find_top(&NODE_KINDS, id) {
                    return Err(EditError::Exists(id.clone(), f));
                }
                let mut node = new_top(kind, id, attrs);
                if !children.is_empty() {
                    let mut ch = KdlDocument::new();
                    for line in children {
                        ch.nodes_mut().push(child_from_str(line)?);
                    }
                    node.set_children(ch);
                }
                fix_indent(&mut node);
                let file = file_for_id(id, |f| self.docs.contains_key(f));
                self.append_top(&file, node);
                Ok(())
            }
            Op::SetAttr { id, key, value } => {
                let (file, node) = self.get_node_mut(id)?;
                node.entries_mut().retain(|e| e.name().map(|n| n.value()) != Some(key.as_str()));
                // çocuk olarak yazılmış `desc` de varsa onu da düşür
                if let Some(ch) = node.children_mut() {
                    ch.nodes_mut().retain(|c| c.name().value() != key.as_str());
                }
                normalize_children(node);
                if !value.is_empty() {
                    node.entries_mut().push(str_prop(key, value));
                }
                self.dirty.insert(file, true);
                Ok(())
            }
            Op::AddChild { id, line } => {
                let child = child_from_str(line)?;
                let (file, node) = self.get_node_mut(id)?;
                push_child(node, child);
                self.dirty.insert(file, true);
                Ok(())
            }
            Op::RmEdge { id, to } => {
                let (file, node) = self.get_node_mut(id)?;
                let mut removed = 0;
                if let Some(ch) = node.children_mut() {
                    let before = ch.nodes().len();
                    ch.nodes_mut().retain(|c| !edge_targets(c).contains(&to.as_str()));
                    removed = before - ch.nodes().len();
                }
                normalize_children(node);
                if removed == 0 {
                    return Err(EditError::NotFound(format!("{id} → {to}")));
                }
                self.dirty.insert(file, true);
                Ok(())
            }
            Op::RmChild { id, name, arg } => {
                let (file, node) = self.get_node_mut(id)?;
                let mut removed = 0;
                if let Some(ch) = node.children_mut() {
                    let before = ch.nodes().len();
                    ch.nodes_mut().retain(|c| !(c.name().value() == name && first_arg(c).as_deref() == Some(arg.as_str())));
                    removed = before - ch.nodes().len();
                }
                normalize_children(node);
                if removed == 0 {
                    return Err(EditError::NotFound(format!("{id} › {name} {arg}")));
                }
                self.dirty.insert(file, true);
                Ok(())
            }
            Op::Rename { from, to } => self.rename(from, to),
            Op::RmNode { id, force } => self.rm_node(id, *force),
            Op::AddVar { id, attrs } => self.add_def("var", "vars.kdl", id, attrs),
            Op::AddCheck { id, attrs } => self.add_def("check", "checks.kdl", id, attrs),
            Op::AddRoot { id } => self.add_def("root", "roots.kdl", id, &BTreeMap::new()),
        }
    }

    fn add_def(&mut self, kind: &str, default_file: &str, id: &str, attrs: &BTreeMap<String, String>) -> Result<(), EditError> {
        if let Some((f, _)) = self.find_top(&[kind], id) {
            return Err(EditError::Exists(id.into(), f));
        }
        let node = new_top(kind, id, attrs);
        // aynı türden tanımların olduğu ilk dosyaya, yoksa varsayılana
        let file = self.docs.iter().find(|(_, d)| d.nodes().iter().any(|n| n.name().value() == kind)).map(|(f, _)| f.clone()).unwrap_or_else(|| default_file.into());
        self.append_top(&file, node);
        Ok(())
    }

    fn rename(&mut self, from: &str, to: &str) -> Result<(), EditError> {
        if !is_valid_id(to) && !crate::model::is_valid_check_id(to) {
            return Err(EditError::InvalidId(to.into()));
        }
        let Some((old_file, kind)) = self.find_top(&["state", "action", "call", "event", "var", "check"], from).map(|(f, n)| (f, n.name().value().to_string())) else {
            return Err(EditError::NotFound(from.into()));
        };
        if self.find_top(&[kind.as_str()], to).is_some() {
            return Err(EditError::Exists(to.into(), kind));
        }
        let is_var = kind == "var";
        // tüm dosyalarda string referansları değiştir
        for (file, doc) in self.docs.iter_mut() {
            let mut touched = false;
            for n in doc.nodes_mut() {
                touched |= rename_in_node(n, from, to, is_var);
            }
            if touched {
                self.dirty.insert(file.clone(), true);
            }
        }
        // node ise dosya taşıması gerekebilir
        if NODE_KINDS.contains(&kind.as_str()) {
            let new_file = file_for_id(to, |f| self.docs.contains_key(f));
            if new_file != old_file && !crate::model::allowed_files(to).contains(&old_file) {
                let node = {
                    let doc = self.docs.get_mut(&old_file).unwrap();
                    let idx = doc.nodes().iter().position(|n| first_arg(n).as_deref() == Some(to)).unwrap();
                    doc.nodes_mut().remove(idx)
                };
                self.dirty.insert(old_file, true);
                self.append_top(&new_file, node);
            }
        }
        Ok(())
    }

    fn rm_node(&mut self, id: &str, force: bool) -> Result<(), EditError> {
        let Some((file, _)) = self.find_top(&NODE_KINDS, id) else { return Err(EditError::NotFound(id.into())) };
        // referanslar
        let mut refs = vec![];
        for (f, doc) in &self.docs {
            for n in doc.nodes() {
                let nid = first_arg(n).unwrap_or_default();
                if nid == id {
                    continue;
                }
                let name = n.name().value();
                if (name == "root" || name == "view") && (nid == id || prop_eq(n, "from", id) || prop_eq(n, "to", id)) {
                    refs.push(format!("{f}: {name} {nid}"));
                }
                if name == "check" && prop_or_arrow(n) == Some(id.into()) {
                    refs.push(format!("{f}: check {nid}"));
                }
                if let Some(ch) = n.children() {
                    if ch.nodes().iter().any(|c| edge_targets(c).contains(&id)) {
                        refs.push(format!("{f}: {nid}"));
                    }
                }
            }
        }
        if !refs.is_empty() && !force {
            return Err(EditError::Referenced(id.into(), refs.join(", ")));
        }
        if force {
            for (f, doc) in self.docs.iter_mut() {
                let mut touched = false;
                doc.nodes_mut().retain(|n| {
                    let name = n.name().value();
                    let drop = (name == "root" && first_arg(n).as_deref() == Some(id)) || (name == "view" && (prop_eq(n, "from", id) || prop_eq(n, "to", id)));
                    touched |= drop;
                    !drop
                });
                for n in doc.nodes_mut() {
                    if let Some(ch) = n.children_mut() {
                        let before = ch.nodes().len();
                        ch.nodes_mut().retain(|c| !edge_targets(c).contains(&id));
                        touched |= before != ch.nodes().len();
                    }
                    normalize_children(n);
                }
                if touched {
                    self.dirty.insert(f.clone(), true);
                }
            }
        }
        let doc = self.docs.get_mut(&file).unwrap();
        doc.nodes_mut().retain(|n| !(NODE_KINDS.contains(&n.name().value()) && first_arg(n).as_deref() == Some(id)));
        self.dirty.insert(file, true);
        Ok(())
    }

    fn find_top(&self, kinds: &[&str], id: &str) -> Option<(String, &KdlNode)> {
        for (f, doc) in &self.docs {
            for n in doc.nodes() {
                if kinds.contains(&n.name().value()) && first_arg(n).as_deref() == Some(id) {
                    return Some((f.clone(), n));
                }
            }
        }
        None
    }

    fn get_node_mut(&mut self, id: &str) -> Result<(String, &mut KdlNode), EditError> {
        for (f, doc) in self.docs.iter_mut() {
            if let Some(n) = doc.nodes_mut().iter_mut().find(|n| NODE_KINDS.contains(&n.name().value()) && first_arg(n).as_deref() == Some(id)) {
                return Ok((f.clone(), n));
            }
        }
        Err(EditError::NotFound(id.into()))
    }

    fn append_top(&mut self, file: &str, mut node: KdlNode) {
        let doc = self.docs.entry(file.into()).or_default();
        // önceki node ile arada boş satır
        if let Some(fmt) = node.format_mut() {
            fmt.leading = if doc.nodes().is_empty() { String::new() } else { "\n".into() };
            if !fmt.terminator.ends_with('\n') {
                fmt.terminator = "\n".into();
            }
        }
        doc.nodes_mut().push(node);
        self.dirty.insert(file.into(), true);
    }

    /// Değişen dosyaları yazar; döndürdüğü liste yazılan dosyalar.
    pub fn commit(&mut self) -> Result<Vec<String>, EditError> {
        let mut written = vec![];
        for (file, dirty) in std::mem::take(&mut self.dirty) {
            if !dirty {
                continue;
            }
            let path = self.dir.join(&file);
            let doc = self.docs.get(&file).unwrap();
            if doc.nodes().is_empty() {
                if path.exists() {
                    std::fs::remove_file(&path)?;
                }
                continue;
            }
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p)?;
            }
            let mut s = doc.to_string();
            if !s.ends_with('\n') {
                s.push('\n');
            }
            std::fs::write(&path, s)?;
            written.push(file);
        }
        Ok(written)
    }

    /// Yazmadan önizleme: dosya → içerik.
    pub fn preview(&self) -> BTreeMap<String, String> {
        self.dirty.iter().filter(|(_, d)| **d).map(|(f, _)| (f.clone(), self.docs.get(f).map(|d| d.to_string()).unwrap_or_default())).collect()
    }
}

fn child_from_str(line: &str) -> Result<KdlNode, EditError> {
    let mut n = KdlNode::parse(line.trim()).map_err(|e| EditError::Bad(format!("çocuk satırı parse edilemedi `{line}`: {}", e.diagnostics.first().and_then(|d| d.message.clone()).unwrap_or_default())))?;
    let mut f = n.format().cloned().unwrap_or_default();
    f.leading = INDENT.into();
    f.terminator = "\n".into();
    f.trailing = String::new();
    n.set_format(f);
    Ok(n)
}

fn quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Tırnaklı string argümanı.
fn str_arg(v: &str) -> KdlEntry {
    let mut e = KdlEntry::new(KdlValue::String(v.into()));
    e.set_format(KdlEntryFormat { value_repr: quoted(v), leading: " ".into(), ..Default::default() });
    e
}

/// Tırnaklı `key="value"` prop'u.
fn str_prop(key: &str, v: &str) -> KdlEntry {
    let mut e = KdlEntry::new_prop(key, KdlValue::String(v.into()));
    e.set_format(KdlEntryFormat { value_repr: quoted(v), leading: " ".into(), ..Default::default() });
    e
}

/// Var olan entry'nin string değerini repr ile birlikte değiştirir.
fn set_str(e: &mut KdlEntry, v: &str) {
    e.set_value(KdlValue::String(v.into()));
    match e.format_mut() {
        Some(f) => f.value_repr = quoted(v),
        None => e.set_format(KdlEntryFormat { value_repr: quoted(v), leading: " ".into(), ..Default::default() }),
    }
}

fn new_top(kind: &str, id: &str, attrs: &BTreeMap<String, String>) -> KdlNode {
    let mut node = KdlNode::new(kind);
    node.set_format(KdlNodeFormat { terminator: "\n".into(), before_children: " ".into(), ..Default::default() });
    node.push(str_arg(id));
    for (k, v) in attrs {
        node.push(str_prop(k, v));
    }
    node
}

/// Çocuk silindikten/eklendikten sonra satır yapısı bozulmasın: her çocuk kendi satırında,
/// `}` kendi satırında; fazladan boş satır üretilmez.
fn normalize_children(node: &mut KdlNode) {
    let Some(ch) = node.children_mut() else { return };
    if ch.nodes().is_empty() {
        node.clear_children();
        if let Some(f) = node.format_mut() {
            f.before_children = String::new();
        }
        return;
    }
    // bir önceki parçanın sonu newline ile bitiyor mu?
    let mut prev_ends_nl = ch.format().is_some_and(|f| f.leading.ends_with('\n'));
    for c in ch.nodes_mut() {
        let f = c.format_mut().expect("parsed node has format");
        if !prev_ends_nl && !f.leading.contains('\n') {
            f.leading = format!("\n{}", f.leading);
        }
        if !f.terminator.contains('\n') {
            f.terminator = "\n".into();
        }
        prev_ends_nl = f.trailing.ends_with('\n') || (f.trailing.is_empty() && f.terminator.ends_with('\n'));
    }
    if let Some(f) = ch.format_mut() {
        f.trailing = f.trailing.trim_end_matches(' ').to_string();
    }
}

fn push_child(node: &mut KdlNode, child: KdlNode) {
    let had = node.children().is_some();
    let ch = node.ensure_children();
    ch.nodes_mut().push(child);
    if !had {
        ch.set_format(kdl::KdlDocumentFormat { leading: "\n".into(), trailing: String::new() });
    }
    normalize_children(node);
    if let Some(f) = node.format_mut() {
        if f.before_children.is_empty() {
            f.before_children = " ".into();
        }
    }
}

/// Yeni üst düzey node: çocuklar bir girinti, `}` kendi satırında.
fn fix_indent(node: &mut KdlNode) {
    if let Some(ch) = node.children_mut() {
        ch.set_format(kdl::KdlDocumentFormat { leading: "\n".into(), trailing: String::new() });
    }
    normalize_children(node);
}

fn first_arg(n: &KdlNode) -> Option<String> {
    n.entries().iter().find(|e| e.name().is_none()).map(|e| val_str(e.value()))
}

fn prop_eq(n: &KdlNode, key: &str, v: &str) -> bool {
    n.entries().iter().any(|e| e.name().map(|k| k.value()) == Some(key) && e.value().as_string() == Some(v))
}

fn prop_or_arrow(n: &KdlNode) -> Option<String> {
    let args: Vec<&KdlEntry> = n.entries().iter().filter(|e| e.name().is_none()).collect();
    if let Some(i) = args.iter().position(|e| e.value().as_string() == Some("->")) {
        return args.get(i + 1).map(|e| val_str(e.value()));
    }
    n.entries().iter().find(|e| e.name().map(|k| k.value()) == Some("fail_to")).map(|e| val_str(e.value()))
}

fn val_str(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Bir çocuk satırının işaret ettiği hedef ID'ler (`->`, `calls`, check `->`).
fn edge_targets(c: &KdlNode) -> Vec<&str> {
    let name = c.name().value();
    let args: Vec<&KdlEntry> = c.entries().iter().filter(|e| e.name().is_none()).collect();
    match name {
        "->" => args.first().and_then(|e| e.value().as_string()).into_iter().collect(),
        "calls" => args.iter().filter_map(|e| e.value().as_string()).collect(),
        "on" | "returns" | "check" => {
            let i = args.iter().position(|e| e.value().as_string() == Some("->"));
            i.and_then(|i| args.get(i + 1)).and_then(|e| e.value().as_string()).into_iter().collect()
        }
        _ => vec![],
    }
}

/// Node ve çocuklarında `from` → `to`. Var ise guard ifadelerinde de kelime bazlı değiştirir.
fn rename_in_node(n: &mut KdlNode, from: &str, to: &str, is_var: bool) -> bool {
    let mut touched = false;
    for e in n.entries_mut() {
        let is_when = e.name().map(|k| k.value()) == Some("when");
        match e.value() {
            KdlValue::String(s) if s == from && !is_var => {
                set_str(e, to);
                touched = true;
            }
            KdlValue::String(s) if is_var && (s == from || is_when) => {
                let new = if is_when { replace_word(s, from, to) } else { to.into() };
                if new != *s {
                    set_str(e, &new);
                    touched = true;
                }
            }
            _ => {}
        }
    }
    if let Some(ch) = n.children_mut() {
        for c in ch.nodes_mut() {
            touched |= rename_in_node(c, from, to, is_var);
        }
    }
    touched
}

fn replace_word(s: &str, from: &str, to: &str) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find(from) {
        let before_ok = i == 0 || !is_ident_char(rest.as_bytes()[i - 1]);
        let after = i + from.len();
        let after_ok = after >= rest.len() || !is_ident_char(rest.as_bytes()[after]);
        out.push_str(&rest[..i]);
        out.push_str(if before_ok && after_ok { to } else { from });
        rest = &rest[after..];
    }
    out.push_str(rest);
    out
}

fn is_ident_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b':'
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

#[cfg(test)]
mod tests {
    use super::*;

    fn ws(files: &[(&str, &str)]) -> (tempdir::Dir, Workspace) {
        let d = tempdir::Dir::new();
        for (f, s) in files {
            let p = d.path.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, s).unwrap();
        }
        let w = Workspace::open(&d.path).unwrap();
        (d, w)
    }

    mod tempdir {
        pub struct Dir {
            pub path: std::path::PathBuf,
        }
        impl Dir {
            pub fn new() -> Self {
                let p = std::env::temp_dir().join(format!("duflow-test-{}-{}", std::process::id(), rand()));
                std::fs::create_dir_all(&p).unwrap();
                Self { path: p }
            }
        }
        impl Drop for Dir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.path);
            }
        }
        fn rand() -> u128 {
            static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let seq = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128;
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() * 1000 + seq
        }
    }

    const ROLLING: &str = "// rolling akışı\nstate \"deploy.rolling.retry\" layer=\"domain\" {\n  desc \"Deneme sayacı artar\" // yorum\n  sets \"deploy.attempts\" \"+1\"\n  -> \"deploy.rolling.rollback\" when=\"deploy.attempts >= 3\"\n}\n";

    #[test]
    fn add_child_preserves_comments_and_indent() {
        let (d, mut w) = ws(&[("deploy/rolling.kdl", ROLLING)]);
        w.apply(&Op::AddChild { id: "deploy.rolling.retry".into(), line: "-> \"deploy.rolling.create_instance\" when=\"deploy.attempts < 3\"".into() }).unwrap();
        w.commit().unwrap();
        let out = std::fs::read_to_string(d.path.join("deploy/rolling.kdl")).unwrap();
        assert!(out.starts_with("// rolling akışı\n"), "{out}");
        assert!(out.contains("// yorum"), "{out}");
        assert!(out.contains("\n  -> \"deploy.rolling.create_instance\" when=\"deploy.attempts < 3\"\n}\n"), "{out}");
    }

    #[test]
    fn add_node_goes_to_id_file() {
        let (d, mut w) = ws(&[("deploy/rolling.kdl", ROLLING)]);
        let mut attrs = BTreeMap::new();
        attrs.insert("layer".into(), "domain".into());
        attrs.insert("desc".into(), "Bitti".into());
        w.apply(&Op::AddNode { kind: "state".into(), id: "deploy.done".into(), attrs, children: vec!["-> \"ui.x\"".into()] }).unwrap();
        w.commit().unwrap();
        let out = std::fs::read_to_string(d.path.join("deploy.kdl")).unwrap();
        assert_eq!(out, "state \"deploy.done\" desc=\"Bitti\" layer=\"domain\" {\n  -> \"ui.x\"\n}\n");
    }

    #[test]
    fn rename_updates_refs_and_moves_file() {
        let (d, mut w) = ws(&[("deploy/rolling.kdl", ROLLING), ("vars.kdl", "var \"deploy.attempts\" type=\"int\"\n")]);
        w.apply(&Op::Rename { from: "deploy.rolling.retry".into(), to: "deploy.again".into() }).unwrap();
        w.apply(&Op::Rename { from: "deploy.attempts".into(), to: "deploy.tries".into() }).unwrap();
        w.commit().unwrap();
        assert!(!d.path.join("deploy/rolling.kdl").exists());
        let out = std::fs::read_to_string(d.path.join("deploy.kdl")).unwrap();
        assert!(out.contains("state \"deploy.again\""), "{out}");
        assert!(out.contains("sets \"deploy.tries\" \"+1\""), "{out}");
        assert!(out.contains("when=\"deploy.tries >= 3\""), "{out}");
        assert!(out.contains("// yorum"), "{out}");
        let vars = std::fs::read_to_string(d.path.join("vars.kdl")).unwrap();
        assert!(vars.contains("var \"deploy.tries\""));
    }

    #[test]
    fn rm_node_refuses_when_referenced() {
        let (_d, mut w) = ws(&[("deploy/rolling.kdl", ROLLING), ("deploy.kdl", "state \"deploy.rolling.rollback\" layer=\"domain\"\n")]);
        let err = w.apply(&Op::RmNode { id: "deploy.rolling.rollback".into(), force: false }).unwrap_err();
        assert!(matches!(err, EditError::Referenced(..)));
        w.apply(&Op::RmNode { id: "deploy.rolling.rollback".into(), force: true }).unwrap();
        let prev = w.preview();
        assert!(!prev["deploy/rolling.kdl"].contains("rollback"));
    }

    #[test]
    fn set_attr_and_rm_edge() {
        let (_d, mut w) = ws(&[("deploy/rolling.kdl", ROLLING)]);
        w.apply(&Op::SetAttr { id: "deploy.rolling.retry".into(), key: "desc".into(), value: "Yeni".into() }).unwrap();
        w.apply(&Op::RmEdge { id: "deploy.rolling.retry".into(), to: "deploy.rolling.rollback".into() }).unwrap();
        let out = &w.preview()["deploy/rolling.kdl"];
        assert!(out.contains("desc=\"Yeni\""), "{out}");
        assert!(!out.contains("rollback"), "{out}");
        assert!(out.contains("sets"), "{out}");
    }
}
