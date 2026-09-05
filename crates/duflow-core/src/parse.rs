//! KDL dosyası → `FileItems`. Yalnız sözdizimi ve şekil; referans tutarlılığı lint'te.

use crate::model::*;
use kdl::{KdlDocument, KdlNode, KdlValue};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{file}:{line}: KDL sözdizimi: {msg}")]
    Syntax { file: String, line: usize, msg: String },
    #[error("{file}:{line}: {msg}")]
    Shape { file: String, line: usize, msg: String },
}

/// Bayt ofsetinden satır numarası (1 tabanlı).
pub fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())].bytes().filter(|&b| b == b'\n').count() + 1
}

struct Ctx<'a> {
    file: &'a str,
    src: &'a str,
}

impl Ctx<'_> {
    fn err(&self, node: &KdlNode, msg: impl Into<String>) -> ParseError {
        ParseError::Shape { file: self.file.into(), line: line_of(self.src, node.span().offset()), msg: msg.into() }
    }
    fn line(&self, node: &KdlNode) -> usize {
        line_of(self.src, node.span().offset())
    }
}

fn val_str(v: &KdlValue) -> String {
    match v {
        KdlValue::String(s) => s.clone(),
        KdlValue::Integer(i) => i.to_string(),
        KdlValue::Float(f) => f.to_string(),
        KdlValue::Bool(b) => b.to_string(),
        KdlValue::Null => "null".into(),
    }
}

/// Konumsal argümanlar (isimsiz entry'ler).
fn args(node: &KdlNode) -> Vec<&KdlValue> {
    node.entries().iter().filter(|e| e.name().is_none()).map(|e| e.value()).collect()
}

fn prop<'a>(node: &'a KdlNode, name: &str) -> Option<&'a KdlValue> {
    node.entries().iter().find(|e| e.name().map(|n| n.value()) == Some(name)).map(|e| e.value())
}

fn prop_str(node: &KdlNode, name: &str) -> Option<String> {
    prop(node, name).map(val_str)
}

fn prop_u16(node: &KdlNode, name: &str) -> Option<u16> {
    prop(node, name).and_then(|v| v.as_integer()).and_then(|i| u16::try_from(i).ok())
}

/// `... -> "x"` kuyruğunu bulur: `->` argümanından sonraki string.
fn arrow_target(node: &KdlNode) -> Option<String> {
    let a = args(node);
    let i = a.iter().position(|v| v.as_string() == Some("->"))?;
    a.get(i + 1).map(|v| val_str(v))
}

fn child_str(node: &KdlNode, name: &str) -> Option<String> {
    node.children()?.get(name).and_then(|c| args(c).first().map(|v| val_str(v)))
}

pub fn parse_file(file: &str, src: &str) -> Result<FileItems, ParseError> {
    let doc: KdlDocument = src.parse().map_err(|e: kdl::KdlError| {
        let (line, msg) = e
            .diagnostics
            .first()
            .map(|d| (line_of(src, d.span.offset()), d.message.clone().unwrap_or_else(|| "geçersiz KDL".into())))
            .unwrap_or((1, "geçersiz KDL".into()));
        ParseError::Syntax { file: file.into(), line, msg }
    })?;
    let cx = Ctx { file, src };
    let mut items = FileItems::default();
    for node in doc.nodes() {
        let name = node.name().value();
        match name {
            "state" | "action" | "call" | "event" => items.nodes.push(parse_node(&cx, node)?),
            "var" => items.vars.push(parse_var(&cx, node)?),
            "check" => items.checks.push(parse_check(&cx, node)?),
            "root" => items.roots.push(RootDef {
                id: first_id(&cx, node)?,
                desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
                file: file.into(),
                line: cx.line(node),
            }),
            "view" => items.views.push(ViewDef {
                id: first_id(&cx, node)?,
                from: prop_str(node, "from").ok_or_else(|| cx.err(node, "view için `from` gerekli"))?,
                to: prop_str(node, "to").ok_or_else(|| cx.err(node, "view için `to` gerekli"))?,
                desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
                file: file.into(),
                line: cx.line(node),
            }),
            "project" => {
                let mut cfg = ProjectConfig::default();
                if let Some(n) = args(node).first() {
                    cfg.name = val_str(n);
                }
                if let Some(l) = node.children().and_then(|c| c.get("layers")) {
                    cfg.layers = args(l).iter().map(|v| val_str(v)).collect();
                }
                items.config = Some(cfg);
            }
            other => return Err(cx.err(node, format!("bilinmeyen üst düzey node `{other}`"))),
        }
    }
    Ok(items)
}

fn first_id(cx: &Ctx, node: &KdlNode) -> Result<String, ParseError> {
    args(node)
        .first()
        .and_then(|v| v.as_string())
        .map(String::from)
        .ok_or_else(|| cx.err(node, format!("`{}` için ilk argüman ID olmalı", node.name().value())))
}

fn parse_node(cx: &Ctx, node: &KdlNode) -> Result<Node, ParseError> {
    let kind = Kind::parse(node.name().value()).unwrap();
    let id = first_id(cx, node)?;
    if !is_valid_id(&id) {
        return Err(cx.err(node, format!("geçersiz ID `{id}` (izin: a-z 0-9 _ ve nokta)")));
    }
    let mut n = Node {
        id,
        kind,
        layer: prop_str(node, "layer").unwrap_or_else(|| default_layer(kind).into()),
        desc: prop_str(node, "desc"),
        doc: None,
        attrs: Default::default(),
        edges: vec![],
        checks: vec![],
        sets: vec![],
        file: cx.file.into(),
        line: cx.line(node),
    };
    for e in node.entries() {
        if let Some(k) = e.name().map(|k| k.value()) {
            if k != "layer" && k != "desc" {
                n.attrs.insert(k.into(), val_str(e.value()));
            }
        }
    }
    let Some(children) = node.children() else { return Ok(n) };
    for c in children.nodes() {
        let line = cx.line(c);
        let cname = c.name().value();
        let when = prop_str(c, "when");
        match cname {
            "desc" => n.desc = args(c).first().map(|v| val_str(v)),
            "doc" => n.doc = args(c).first().map(|v| val_str(v)),
            "->" => {
                let to = args(c).first().map(|v| val_str(v)).ok_or_else(|| cx.err(c, "`->` hedef ister"))?;
                n.edges.push(Edge { to, kind: EdgeKind::Plain, when, line });
            }
            "on" => {
                let event = args(c).first().map(|v| val_str(v)).ok_or_else(|| cx.err(c, "`on` olay ID'si ister"))?;
                let to = arrow_target(c).ok_or_else(|| cx.err(c, "`on \"ev\" -> \"hedef\"` bekleniyor"))?;
                n.edges.push(Edge { to, kind: EdgeKind::On { event }, when, line });
            }
            "returns" => {
                // `returns 202 code="x" -> "h"` ya da `returns ok -> "h"`
                let first = args(c).first().copied().ok_or_else(|| cx.err(c, "`returns <status|etiket> -> \"hedef\"` bekleniyor"))?;
                let (status, tag) = match first {
                    KdlValue::Integer(i) => (Some(u16::try_from(*i).map_err(|_| cx.err(c, "geçersiz status"))?), None),
                    KdlValue::String(s) if s != "->" => (None, Some(s.clone())),
                    _ => return Err(cx.err(c, "`returns` ilk argümanı status ya da etiket olmalı")),
                };
                let to = arrow_target(c).ok_or_else(|| cx.err(c, "`returns` için `-> \"hedef\"` gerekli"))?;
                let code = prop_str(c, "code").or(tag);
                n.edges.push(Edge { to, kind: EdgeKind::Returns { status, code }, when, line });
            }
            "calls" => {
                for v in args(c) {
                    n.edges.push(Edge { to: val_str(v), kind: EdgeKind::Calls, when: when.clone(), line });
                }
            }
            "check" => {
                let name = args(c).first().map(|v| val_str(v)).ok_or_else(|| cx.err(c, "`check` ad ister"))?;
                n.checks.push(CheckUse { name, fail: prop_u16(c, "fail"), code: prop_str(c, "code"), to: arrow_target(c), line });
            }
            "sets" => {
                let a = args(c);
                let var = a.first().map(|v| val_str(v)).ok_or_else(|| cx.err(c, "`sets \"var\" \"değer\"` bekleniyor"))?;
                let value = a.get(1).map(|v| val_str(v)).unwrap_or_default();
                n.sets.push(SetVar { var, value, line });
            }
            other => {
                // bilinmeyen çocuk: attr olarak sakla (ör. `method "POST"`)
                if let Some(v) = args(c).first() {
                    n.attrs.insert(other.into(), val_str(v));
                } else {
                    return Err(cx.err(c, format!("bilinmeyen çocuk `{other}`")));
                }
            }
        }
    }
    Ok(n)
}

fn default_layer(kind: Kind) -> &'static str {
    match kind {
        Kind::State | Kind::Action => "ui",
        Kind::Call => "api",
        Kind::Event => "domain",
    }
}

fn parse_var(cx: &Ctx, node: &KdlNode) -> Result<VarDef, ParseError> {
    let id = first_id(cx, node)?;
    if !is_valid_id(&id) {
        return Err(cx.err(node, format!("geçersiz var ID `{id}`")));
    }
    Ok(VarDef {
        id,
        ty: prop_str(node, "type").unwrap_or_else(|| "string".into()),
        desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
        source: prop_str(node, "source"),
        values: prop_str(node, "values").map(|s| s.split_whitespace().map(String::from).collect()).unwrap_or_default(),
        file: cx.file.into(),
        line: cx.line(node),
    })
}

fn parse_check(cx: &Ctx, node: &KdlNode) -> Result<CheckDef, ParseError> {
    let id = first_id(cx, node)?;
    if !is_valid_check_id(&id) {
        return Err(cx.err(node, format!("geçersiz check ID `{id}`")));
    }
    Ok(CheckDef {
        id,
        desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
        reads: prop_str(node, "reads").map(|s| s.split_whitespace().map(String::from).collect()).unwrap_or_default(),
        fail_to: arrow_target(node).or_else(|| prop_str(node, "fail_to")),
        file: cx.file.into(),
        line: cx.line(node),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_shapes() {
        let src = r#"
state "deploy.rolling.retry" layer="domain" {
  desc "Deneme sayacı artar"
  sets "deploy.attempts" "+1"
  -> "deploy.rolling.create_instance" when="deploy.attempts < 3"
  on "instance.failed" -> "deploy.rolling.retry"
}
call "api.deploys.create" method="POST" path="/x" {
  check "perm:deploy.trigger" fail=404
  check "has_build" fail=422 code="no_build" -> "ui.toast"
  returns 202 -> "deploy.queued"
}
action "ui.submit" { calls "api.deploys.create" }
var "deploy.attempts" type="int"
var "role" type="enum" source="session" values="admin member"
check "perm:deploy.trigger" reads="role"
root "ui.login"
view "v" from="ui.login" to="deploy.done"
"#;
        let it = parse_file("t.kdl", src).unwrap();
        assert_eq!(it.nodes.len(), 3);
        let retry = &it.nodes[0];
        assert_eq!(retry.edges.len(), 2);
        assert_eq!(retry.edges[0].when.as_deref(), Some("deploy.attempts < 3"));
        assert!(matches!(retry.edges[1].kind, EdgeKind::On { .. }));
        assert_eq!(retry.sets[0].value, "+1");
        let call = &it.nodes[1];
        assert_eq!(call.attrs["method"], "POST");
        assert_eq!(call.checks[1].to.as_deref(), Some("ui.toast"));
        assert!(matches!(call.edges[0].kind, EdgeKind::Returns { status: Some(202), .. }));
        assert_eq!(it.vars[1].values, vec!["admin", "member"]);
        assert_eq!(it.checks[0].reads, vec!["role"]);
        assert_eq!(it.roots[0].id, "ui.login");
        assert_eq!(it.views[0].to, "deploy.done");
        assert_eq!(retry.line, 2);
    }
}
