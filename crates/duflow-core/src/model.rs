//! Graf modeli: node, kenar, değişken, check tanımları.
//! Dosyadan parse edilen ham yapı; graph.rs bunun üstüne indeks kurar.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Node türü. `check`/`var`/`root`/`view` graf düğümü değil, yardımcı tanımlardır;
/// ayrı koleksiyonlarda tutulur.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    State,
    Action,
    Call,
    Event,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        Some(match s {
            "state" => Kind::State,
            "action" => Kind::Action,
            "call" => Kind::Call,
            "event" => Kind::Event,
            _ => return None,
        })
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::State => "state",
            Kind::Action => "action",
            Kind::Call => "call",
            Kind::Event => "event",
        }
    }
}

/// Kenarın nereden geldiği; UI etiketi ve lint bunu kullanır.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum EdgeKind {
    /// `-> "x"`
    Plain,
    /// `on "event" -> "x"`
    On { event: String },
    /// `returns 202 -> "x"` (code opsiyonel); iç çağrılarda `returns ok -> "x"`
    Returns {
        status: Option<u16>,
        code: Option<String>,
    },
    /// `calls "call.id"`
    Calls,
    /// `check "name" ... -> "x"`: check başarısız olunca gidilen yer
    CheckFail {
        check: String,
        status: Option<u16>,
        code: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    pub to: String,
    #[serde(flatten)]
    pub kind: EdgeKind,
    /// Guard ifadesi; yorumlanmaz, içindeki var isimleri lint'lenir.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when: Option<String>,
    /// Dal etiketi (`case="pool_exhausted"`): guard'sız gerçek dallanmayı adlandırır;
    /// `case` ya da `when` taşıyan kenar `ambiguous_transition` sayılmaz.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub case: Option<String>,
    /// Sıra numarası (`seq=1`): dallanma değil, hepsi sırayla koşan adımlar. `seq` taşıyan
    /// kenar da `ambiguous_transition` sayılmaz; aynı node'da tekrar eden `seq` uyarıdır.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub seq: Option<u32>,
    pub line: usize,
}

impl Edge {
    /// UI ve brief için tek satırlık etiket.
    pub fn label(&self) -> String {
        let base = match &self.kind {
            EdgeKind::Plain => String::new(),
            EdgeKind::On { event } => format!("on {event}"),
            EdgeKind::Returns { status, code } => match (status, code) {
                (Some(s), Some(c)) => format!("{s} {c}"),
                (Some(s), None) => s.to_string(),
                (None, Some(c)) => c.clone(),
                (None, None) => "ok".into(),
            },
            EdgeKind::Calls => "calls".into(),
            EdgeKind::CheckFail { check, status, .. } => match status {
                Some(s) => format!("{check} ✗ {s}"),
                None => format!("{check} ✗"),
            },
        };
        let mut parts: Vec<String> = vec![];
        if !base.is_empty() {
            parts.push(base);
        }
        if let Some(n) = self.seq {
            parts.push(format!("#{n}"));
        }
        if let Some(c) = &self.case {
            parts.push(format!("case {c}"));
        }
        if let Some(w) = &self.when {
            parts.push(format!("when {w}"));
        }
        parts.join(" · ")
    }

    /// UI sınıfı: `fail` (hata dalı), `guard`, ya da boş.
    pub fn class(&self) -> &'static str {
        match &self.kind {
            EdgeKind::CheckFail { .. } => "fail",
            EdgeKind::Returns {
                status: Some(s), ..
            } if *s >= 400 => "fail",
            _ if self.when.is_some() => "guard",
            _ => "",
        }
    }
}

/// Bir `call` içindeki `check` kullanımı.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckUse {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// `-> "x"` verilmişse hedef; yoksa check tanımındaki `fail_to`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    /// Node'suz son (`outcome="toast: ..."`): fail'de gidilen yer bir state değil, serbest metin.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<String>,
    pub line: usize,
}

/// Hedef node'u olmayan terminal çıkış: `returns 409 code="x" outcome="..."` ya da
/// `-> outcome="..." when="..."` (`arrow`: `->` ile yazıldı, status/code yok).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub when: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub case: Option<String>,
    #[serde(default)]
    pub arrow: bool,
    pub line: usize,
}

impl Outcome {
    pub fn label(&self) -> String {
        let base = match (self.status, &self.code) {
            (Some(s), Some(c)) => format!("{s} {c}"),
            (Some(s), None) => s.to_string(),
            (None, Some(c)) => c.clone(),
            (None, None) if self.arrow => String::new(),
            (None, None) => "ok".into(),
        };
        let mut parts: Vec<String> = vec![];
        if !base.is_empty() {
            parts.push(base);
        }
        if let Some(c) = &self.case {
            parts.push(format!("case {c}"));
        }
        if let Some(w) = &self.when {
            parts.push(format!("when {w}"));
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SetVar {
    pub var: String,
    /// Yorumlanmayan değer ifadesi: `"+1"`, `"0"`, `"true"`.
    pub value: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    pub layer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// Serbest attribute'lar: `method`, `path`, `kind` (action alt türü) vb.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub attrs: BTreeMap<String, String>,
    pub edges: Vec<Edge>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub checks: Vec<CheckUse>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub sets: Vec<SetVar>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub outcomes: Vec<Outcome>,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VarDef {
    pub id: String,
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// Dış kaynak: `session`, `config:duploy.toml#...`. Yoksa `sets` ile yazılmalı.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub values: Vec<String>,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckDef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// Okuduğu değişkenler (boşlukla ayrılmış liste parse edilir).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub reads: Vec<String>,
    /// Varsayılan fail hedefi.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_to: Option<String>,
    /// Varsayılan node'suz son (`outcome="toast: ..."`); kullanım `->`/`outcome=` yazmazsa bu.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<String>,
    pub file: String,
    pub line: usize,
}

/// İzin tanımı. `requires "x"` kullanımı `perm:x` adlı sentetik bir check'e çözülür
/// (`Graph::reindex`); `deny` fail kodu, `fail_to` varsayılan hedef.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermDef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// `project | org | system` gibi serbest kapsam etiketi
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fail_to: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub outcome: Option<String>,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootDef {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    /// Periyodik giriş (reconciler, canlılık turu): `every="30s"`. Sahte kenar yerine root.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub every: Option<String>,
    pub file: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ViewDef {
    pub id: String,
    pub from: String,
    pub to: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub file: String,
    pub line: usize,
}

/// Proje ayarı (`flows/flow.kdl`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub name: String,
    pub layers: Vec<String>,
    /// Grafı etkileyebilecek kaynak dosya glob'ları (repo köküne göre). Editor hook'ları
    /// bu dosyalar değişince `flows/` güncellenmiş mi diye bakar.
    pub watch: Vec<String>,
    /// Bu sayının üstünde root → `too_many_roots` uyarısı (root uydurmayı frenler).
    #[serde(default = "default_max_roots")]
    pub max_roots: usize,
}

fn default_max_roots() -> usize {
    10
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            name: "flows".into(),
            layers: vec!["ui".into(), "api".into(), "domain".into()],
            watch: vec![],
            max_roots: default_max_roots(),
        }
    }
}

/// Tek bir dosyadan parse edilen her şey.
#[derive(Debug, Default, Clone)]
pub struct FileItems {
    pub nodes: Vec<Node>,
    pub vars: Vec<VarDef>,
    pub checks: Vec<CheckDef>,
    pub perms: Vec<PermDef>,
    pub roots: Vec<RootDef>,
    pub views: Vec<ViewDef>,
    pub config: Option<ProjectConfig>,
}

/// `requires "x"` kullanımının çözüldüğü check adı.
pub fn perm_check_id(perm: &str) -> String {
    format!("perm:{perm}")
}

/// ID sözdizimi: `[a-z0-9_]+(\.[a-z0-9_]+)*`; check adlarında `:` de olur (`perm:deploy.trigger`).
pub fn is_valid_id(s: &str) -> bool {
    !s.is_empty()
        && s.split('.').all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

pub fn is_valid_check_id(s: &str) -> bool {
    !s.is_empty()
        && s.split([':', '.']).all(|seg| {
            !seg.is_empty()
                && seg
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        })
}

/// ID'nin yaşayabileceği dosyalar. `a` → `a.kdl`; `a.b` → `a.kdl` ya da `a/b.kdl`;
/// `a.b.c…` → `a/b.kdl` ya da `a.kdl`. İlk eleman tercih edilen.
pub fn allowed_files(id: &str) -> Vec<String> {
    let segs: Vec<&str> = id.split('.').collect();
    match segs.len() {
        0 | 1 => vec![format!("{id}.kdl")],
        2 => vec![
            format!("{}.kdl", segs[0]),
            format!("{}/{}.kdl", segs[0], segs[1]),
        ],
        _ => vec![
            format!("{}/{}.kdl", segs[0], segs[1]),
            format!("{}.kdl", segs[0]),
        ],
    }
}

/// Yeni node için dosya. `exists(f)`: dosya var mı; `has_group(f, prefix)`: dosyada
/// `prefix` ile başlayan bir ID tanımlı mı.
///
/// 3+ segment (`a.b.c`) her zaman `a/b.kdl`; tek istisna `a.kdl` zaten `a.b.*` grubunu
/// barındırıyorsa (eski düzen) oraya devam edilir. Aksi halde `a.kdl` ikinci segmentli
/// ID'lerle (`a.b`) bir kez oluşunca tüm `a.*` oraya yığılıyordu.
/// 2 segment (`a.b`): `a/b.kdl` varsa oraya, yoksa `a.kdl`.
pub fn file_for_id(
    id: &str,
    exists: impl Fn(&str) -> bool,
    has_group: impl Fn(&str, &str) -> bool,
) -> String {
    let allowed = allowed_files(id);
    let segs: Vec<&str> = id.split('.').collect();
    if segs.len() >= 3 {
        let grouped = format!("{}/{}.kdl", segs[0], segs[1]);
        let flat = format!("{}.kdl", segs[0]);
        let prefix = format!("{}.{}.", segs[0], segs[1]);
        if !exists(&grouped) && exists(&flat) && has_group(&flat, &prefix) {
            return flat;
        }
        return grouped;
    }
    allowed
        .iter()
        .find(|f| exists(f))
        .cloned()
        .unwrap_or_else(|| allowed[0].clone())
}

/// UI gruplama anahtarı: ilk iki segment (ya da tek segment).
pub fn group_of(id: &str) -> String {
    id.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// Yerel sayaç kapsamı: son noktaya kadarki prefix (`restore.snapshot` → `restore`,
/// `deploy.rolling.retry` → `deploy.rolling`). Noktasız ID kendi kapsamıdır.
pub fn local_group(id: &str) -> &str {
    id.rsplit_once('.').map(|(p, _)| p).unwrap_or(id)
}

/// `entry="api"` gibi bir attr taşıyan node dış istemcinin doğrudan çağırdığı giriştir:
/// root sayılmadan erişilebilir kabul edilir, `too_many_roots` sayımına girmez.
pub fn is_entry(n: &Node) -> bool {
    n.attrs.get("entry").is_some_and(|v| !v.is_empty())
}
