// Guard ifadesi için küçük değerlendirici. Best-effort: tüm değişkenler biliniyorsa true/false,
// aksi halde 'unknown'. Desteklenen: değişken, sayı, "string", true/false, == != < <= > >= && || ! ( ).

export type Tri = 'true' | 'false' | 'unknown'

type Tok = { t: 'num'; v: number } | { t: 'str'; v: string } | { t: 'id'; v: string } | { t: 'op'; v: string }

function lex(src: string): Tok[] | null {
  const out: Tok[] = []
  let i = 0
  while (i < src.length) {
    const c = src[i]
    if (/\s/.test(c)) { i++; continue }
    if (c === '"' || c === "'") { const j = src.indexOf(c, i + 1); if (j < 0) return null; out.push({ t: 'str', v: src.slice(i + 1, j) }); i = j + 1; continue }
    if (/[0-9]/.test(c)) { const m = src.slice(i).match(/^\d+(\.\d+)?/)!; out.push({ t: 'num', v: Number(m[0]) }); i += m[0].length; continue }
    if (/[a-zA-Z_]/.test(c)) { const m = src.slice(i).match(/^[a-zA-Z_][a-zA-Z0-9_.:]*/)!; out.push({ t: 'id', v: m[0] }); i += m[0].length; continue }
    const two = src.slice(i, i + 2)
    if (['==', '!=', '<=', '>=', '&&', '||'].includes(two)) { out.push({ t: 'op', v: two }); i += 2; continue }
    if ('<>!()'.includes(c)) { out.push({ t: 'op', v: c }); i++; continue }
    return null
  }
  return out
}

type Val = number | string | boolean | undefined

/** Değişken değerleri string olarak gelir ('—' ve '…' bilinmiyor demektir). */
export function evalGuard(expr: string, vars: Record<string, string>): Tri {
  const toks = lex(expr); if (!toks) return 'unknown'
  let p = 0
  let unknown = false
  const val = (s: string | undefined): Val => {
    if (s === undefined || s === '—' || s === '…' || s === '') { unknown = true; return undefined }
    if (s === 'true') return true
    if (s === 'false') return false
    if (/^-?\d+(\.\d+)?$/.test(s)) return Number(s)
    return s
  }
  const peek = () => toks[p]
  const isOp = (v: string) => peek()?.t === 'op' && (peek() as { v: string }).v === v
  const primary = (): Val => {
    const t = toks[p++]
    if (!t) { unknown = true; return undefined }
    if (t.t === 'num' || t.t === 'str') return t.v
    if (t.t === 'id') {
      if (t.v === 'true') return true
      if (t.v === 'false') return false
      if (t.v === 'null') return undefined
      return val(vars[t.v])
    }
    if (t.v === '!') { const v = primary(); return v === undefined ? undefined : !truthy(v) }
    if (t.v === '(') { const v = or(); if (isOp(')')) p++; return v }
    unknown = true; return undefined
  }
  const cmp = (): Val => {
    let a = primary()
    while (peek()?.t === 'op' && ['==', '!=', '<', '<=', '>', '>='].includes((peek() as { v: string }).v)) {
      const op = (toks[p++] as { v: string }).v
      const b = primary()
      if (a === undefined || b === undefined) { a = undefined; continue }
      switch (op) {
        case '==': a = a == b; break
        case '!=': a = a != b; break
        case '<': a = Number(a) < Number(b); break
        case '<=': a = Number(a) <= Number(b); break
        case '>': a = Number(a) > Number(b); break
        case '>=': a = Number(a) >= Number(b); break
      }
    }
    return a
  }
  const and = (): Val => {
    let a = cmp()
    while (isOp('&&')) { p++; const b = cmp(); a = a === undefined || b === undefined ? (a === false || b === false ? false : undefined) : truthy(a) && truthy(b) }
    return a
  }
  const or = (): Val => {
    let a = and()
    while (isOp('||')) { p++; const b = and(); a = a === undefined || b === undefined ? (a === true || b === true ? true : undefined) : truthy(a) || truthy(b) }
    return a
  }
  const r = or()
  if (r === undefined || unknown && r === undefined) return 'unknown'
  return truthy(r) ? 'true' : 'false'
}

function truthy(v: Val): boolean { return v !== undefined && v !== false && v !== 0 && v !== '' }
