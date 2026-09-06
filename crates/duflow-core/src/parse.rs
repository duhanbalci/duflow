//! KDL dosyası → `FileItems`. Yalnız sözdizimi ve şekil; referans tutarlılığı lint'te.

use crate::model::*;
use kdl::{KdlDocument, KdlNode, KdlValue};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("{file}:{line}: KDL syntax: {msg}")]
    Syntax {
        file: String,
        line: usize,
        msg: String,
    },
    #[error("{file}:{line}: {msg}")]
    Shape {
        file: String,
        line: usize,
        msg: String,
    },
}

/// Bayt ofsetinden satır numarası (1 tabanlı).
pub fn line_of(src: &str, offset: usize) -> usize {
    src[..offset.min(src.len())]
        .bytes()
        .filter(|&b| b == b'\n')
        .count()
        + 1
}

struct Ctx<'a> {
    file: &'a str,
    src: &'a str,
}

impl Ctx<'_> {
    fn err(&self, node: &KdlNode, msg: impl Into<String>) -> ParseError {
        ParseError::Shape {
            file: self.file.into(),
            line: line_of(self.src, node.span().offset()),
            msg: msg.into(),
        }
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
    node.entries()
        .iter()
        .filter(|e| e.name().is_none())
        .map(|e| e.value())
        .collect()
}

fn prop<'a>(node: &'a KdlNode, name: &str) -> Option<&'a KdlValue> {
    node.entries()
        .iter()
        .find(|e| e.name().map(|n| n.value()) == Some(name))
        .map(|e| e.value())
}

fn prop_str(node: &KdlNode, name: &str) -> Option<String> {
    prop(node, name).map(val_str)
}

fn prop_u16(node: &KdlNode, name: &str) -> Option<u16> {
    prop(node, name)
        .and_then(|v| v.as_integer())
        .and_then(|i| u16::try_from(i).ok())
}

/// `... -> "x"` kuyruğunu bulur: `->` argümanından sonraki string.
fn arrow_target(node: &KdlNode) -> Option<String> {
    let a = args(node);
    let i = a.iter().position(|v| v.as_string() == Some("->"))?;
    a.get(i + 1).map(|v| val_str(v))
}

fn child_str(node: &KdlNode, name: &str) -> Option<String> {
    node.children()?
        .get(name)
        .and_then(|c| args(c).first().map(|v| val_str(v)))
}

pub fn parse_file(file: &str, src: &str) -> Result<FileItems, ParseError> {
    let doc: KdlDocument = src.parse().map_err(|e: kdl::KdlError| {
        let (line, msg) = e
            .diagnostics
            .first()
            .map(|d| {
                (
                    line_of(src, d.span.offset()),
                    d.message.clone().unwrap_or_else(|| "invalid KDL".into()),
                )
            })
            .unwrap_or((1, "invalid KDL".into()));
        ParseError::Syntax {
            file: file.into(),
            line,
            msg,
        }
    })?;
    let cx = Ctx { file, src };
    let mut items = FileItems::default();
    for node in doc.nodes() {
        let name = node.name().value();
        match name {
            "state" | "action" | "call" | "event" => items.nodes.push(parse_node(&cx, node)?),
            "var" => items.vars.push(parse_var(&cx, node)?),
            "check" => items.checks.push(parse_check(&cx, node)?),
            "perm" => {
                let id = first_id(&cx, node)?;
                if !is_valid_id(&id) {
                    return Err(cx.err(node, format!("invalid perm ID `{id}`")));
                }
                items.perms.push(PermDef {
                    id,
                    desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
                    scope: prop_str(node, "scope"),
                    deny: prop_u16(node, "deny"),
                    fail_to: arrow_target(node).or_else(|| prop_str(node, "fail_to")),
                    file: file.into(),
                    line: cx.line(node),
                });
            }
            "root" => items.roots.push(RootDef {
                id: first_id(&cx, node)?,
                desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
                every: prop_str(node, "every"),
                file: file.into(),
                line: cx.line(node),
            }),
            "view" => items.views.push(ViewDef {
                id: first_id(&cx, node)?,
                from: prop_str(node, "from").ok_or_else(|| cx.err(node, "view requires `from`"))?,
                to: prop_str(node, "to").ok_or_else(|| cx.err(node, "view requires `to`"))?,
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
                // `watch "a/**" "b/**"` ya da birden çok `watch` satırı
                for w in node
                    .children()
                    .map(|c| c.nodes())
                    .unwrap_or_default()
                    .iter()
                    .filter(|n| n.name().value() == "watch")
                {
                    cfg.watch.extend(args(w).iter().map(|v| val_str(v)));
                }
                items.config = Some(cfg);
            }
            other => return Err(cx.err(node, format!("unknown top-level node `{other}`"))),
        }
    }
    Ok(items)
}

fn first_id(cx: &Ctx, node: &KdlNode) -> Result<String, ParseError> {
    args(node)
        .first()
        .and_then(|v| v.as_string())
        .map(String::from)
        .ok_or_else(|| {
            cx.err(
                node,
                format!("first argument of `{}` must be an ID", node.name().value()),
            )
        })
}

fn parse_node(cx: &Ctx, node: &KdlNode) -> Result<Node, ParseError> {
    let kind = Kind::parse(node.name().value()).unwrap();
    let id = first_id(cx, node)?;
    if !is_valid_id(&id) {
        return Err(cx.err(
            node,
            format!("invalid ID `{id}` (allowed: a-z 0-9 _ and dot)"),
        ));
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
        outcomes: vec![],
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
    let Some(children) = node.children() else {
        return Ok(n);
    };
    for c in children.nodes() {
        let line = cx.line(c);
        let cname = c.name().value();
        let when = prop_str(c, "when");
        let case = prop_str(c, "case");
        match cname {
            "desc" => n.desc = args(c).first().map(|v| val_str(v)),
            "doc" => n.doc = args(c).first().map(|v| val_str(v)),
            "->" => {
                let to = args(c)
                    .first()
                    .map(|v| val_str(v))
                    .ok_or_else(|| cx.err(c, "`->` hedef ister"))?;
                n.edges.push(Edge {
                    to,
                    kind: EdgeKind::Plain,
                    when,
                    case,
                    line,
                });
            }
            "on" => {
                let event = args(c)
                    .first()
                    .map(|v| val_str(v))
                    .ok_or_else(|| cx.err(c, "`on` olay ID'si ister"))?;
                let to = arrow_target(c)
                    .ok_or_else(|| cx.err(c, "`on \"ev\" -> \"hedef\"` bekleniyor"))?;
                n.edges.push(Edge {
                    to,
                    kind: EdgeKind::On { event },
                    when,
                    case,
                    line,
                });
            }
            "returns" => {
                // `returns 202 code="x" -> "h"` ya da `returns ok -> "h"`
                let first = args(c).first().copied().ok_or_else(|| {
                    cx.err(c, "`returns <status|etiket> -> \"hedef\"` bekleniyor")
                })?;
                let (status, tag) = match first {
                    KdlValue::Integer(i) => (
                        Some(u16::try_from(*i).map_err(|_| cx.err(c, "invalid status"))?),
                        None,
                    ),
                    KdlValue::String(s) if s != "->" => (None, Some(s.clone())),
                    _ => {
                        return Err(
                            cx.err(c, "first argument of `returns` must be a status or a tag")
                        );
                    }
                };
                let code = prop_str(c, "code").or(tag);
                match (arrow_target(c), prop_str(c, "outcome")) {
                    (Some(to), _) => n.edges.push(Edge {
                        to,
                        kind: EdgeKind::Returns { status, code },
                        when,
                        case,
                        line,
                    }),
                    (None, Some(text)) => n.outcomes.push(Outcome {
                        status,
                        code,
                        text,
                        line,
                    }),
                    (None, None) => {
                        return Err(
                            cx.err(c, "`returns` requires `-> \"target\"` or `outcome=\"...\"`")
                        );
                    }
                }
            }
            "calls" => {
                for v in args(c) {
                    n.edges.push(Edge {
                        to: val_str(v),
                        kind: EdgeKind::Calls,
                        when: when.clone(),
                        case: case.clone(),
                        line,
                    });
                }
            }
            "check" => {
                let name = args(c)
                    .first()
                    .map(|v| val_str(v))
                    .ok_or_else(|| cx.err(c, "`check` ad ister"))?;
                n.checks.push(CheckUse {
                    name,
                    fail: prop_u16(c, "fail"),
                    code: prop_str(c, "code"),
                    to: arrow_target(c),
                    outcome: prop_str(c, "outcome"),
                    line,
                });
            }
            "requires" => {
                // `requires "deploy.trigger" [-> "x"]`: perm tanımına bağlı sentetik check
                for v in args(c).iter().take_while(|v| v.as_string() != Some("->")) {
                    n.checks.push(CheckUse {
                        name: perm_check_id(&val_str(v)),
                        fail: prop_u16(c, "fail"),
                        code: prop_str(c, "code"),
                        to: arrow_target(c),
                        outcome: prop_str(c, "outcome"),
                        line,
                    });
                }
            }
            "sets" => {
                let a = args(c);
                let var = a
                    .first()
                    .map(|v| val_str(v))
                    .ok_or_else(|| cx.err(c, "expected `sets \"var\" \"value\"`"))?;
                let value = a.get(1).map(|v| val_str(v)).unwrap_or_default();
                n.sets.push(SetVar { var, value, line });
            }
            other => {
                // bilinmeyen çocuk: attr olarak sakla (ör. `method "POST"`)
                if let Some(v) = args(c).first() {
                    n.attrs.insert(other.into(), val_str(v));
                } else {
                    return Err(cx.err(c, format!("unknown child `{other}`")));
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
        return Err(cx.err(node, format!("invalid var ID `{id}`")));
    }
    Ok(VarDef {
        id,
        ty: prop_str(node, "type").unwrap_or_else(|| "string".into()),
        desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
        source: prop_str(node, "source"),
        values: prop_str(node, "values")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
        file: cx.file.into(),
        line: cx.line(node),
    })
}

fn parse_check(cx: &Ctx, node: &KdlNode) -> Result<CheckDef, ParseError> {
    let id = first_id(cx, node)?;
    if !is_valid_check_id(&id) {
        return Err(cx.err(node, format!("invalid check ID `{id}`")));
    }
    Ok(CheckDef {
        id,
        desc: prop_str(node, "desc").or_else(|| child_str(node, "desc")),
        reads: prop_str(node, "reads")
            .map(|s| s.split_whitespace().map(String::from).collect())
            .unwrap_or_default(),
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
        assert!(matches!(
            call.edges[0].kind,
            EdgeKind::Returns {
                status: Some(202),
                ..
            }
        ));
        assert_eq!(it.vars[1].values, vec!["admin", "member"]);
        assert_eq!(it.checks[0].reads, vec!["role"]);
        assert_eq!(it.roots[0].id, "ui.login");
        assert_eq!(it.views[0].to, "deploy.done");
        assert_eq!(retry.line, 2);
    }

    #[test]
    fn parses_project_config() {
        let src = r#"
project "duploy" {
  layers "ui" "api" "domain" "infra"
  watch "dorch/src/api/**" "dorch/src/deploy/**"
  watch "dorch/ui/src/views/**"
}
"#;
        let it = parse_file("flow.kdl", src).unwrap();
        let cfg = it.config.unwrap();
        assert_eq!(cfg.name, "duploy");
        assert_eq!(cfg.layers.len(), 4);
        assert_eq!(
            cfg.watch,
            vec![
                "dorch/src/api/**",
                "dorch/src/deploy/**",
                "dorch/ui/src/views/**"
            ]
        );
    }

    #[test]
    fn parses_case_labels_outcomes_requires_and_periodic_roots() {
        let src = r#"
state "event.published" layer="domain" desc="x" {
  check "pool_has_ip" fail=409 outcome="IP havuzu dolu"
  -> "deploy.a" case="deploy"
  -> "deploy.b" case="build"
  on "deploy.done" -> "deploy.a" case="late"
}
call "api.x.save" method="POST" path="/x" {
  requires "deploy.trigger"
  requires "org.admin" -> "ui.forbidden"
  returns 200 case="pr" -> "x.pr"
  returns 200 case="apply" -> "x.apply"
  returns 409 code="volume_in_use" outcome="toast: volume in use"
}
perm "deploy.trigger" scope="project" deny=404 desc="Projede deploy izni" -> "ui.not_found"
root "network.liveness" every="30s"
"#;
        let it = parse_file("t.kdl", src).unwrap();
        let ev = &it.nodes[0];
        assert_eq!(ev.checks[0].outcome.as_deref(), Some("IP havuzu dolu"));
        assert_eq!(ev.checks[0].to, None);
        assert_eq!(ev.edges[0].case.as_deref(), Some("deploy"));
        assert_eq!(ev.edges[0].label(), "case deploy");
        assert_eq!(ev.edges[2].case.as_deref(), Some("late"));
        assert_eq!(ev.edges[2].label(), "on deploy.done · case late");
        let call = &it.nodes[1];
        // requires → perm:<id> check kullanımı; fail/hedef perm tanımından gelir
        assert_eq!(call.checks[0].name, "perm:deploy.trigger");
        assert_eq!(call.checks[0].fail, None);
        assert_eq!(call.checks[1].to.as_deref(), Some("ui.forbidden"));
        assert_eq!(call.edges[0].label(), "200 · case pr");
        assert_eq!(call.edges.len(), 2);
        assert_eq!(call.outcomes.len(), 1);
        assert_eq!(call.outcomes[0].status, Some(409));
        assert_eq!(call.outcomes[0].code.as_deref(), Some("volume_in_use"));
        assert_eq!(call.outcomes[0].text, "toast: volume in use");
        let perm = &it.perms[0];
        assert_eq!(perm.id, "deploy.trigger");
        assert_eq!(perm.scope.as_deref(), Some("project"));
        assert_eq!(perm.deny, Some(404));
        assert_eq!(perm.fail_to.as_deref(), Some("ui.not_found"));
        assert_eq!(it.roots[0].every.as_deref(), Some("30s"));
    }

    #[test]
    fn returns_without_target_or_outcome_is_an_error() {
        let err = parse_file("t.kdl", "call \"a.b\" { returns 500 }\n").unwrap_err();
        assert!(err.to_string().contains("returns"), "{err}");
    }
}
