// Graf verisi ve saf sorgular. duflow-core export şeması (schema 2).

export type Kind = 'state' | 'action' | 'call' | 'event'

export interface Edge { to: string; label: string; class: '' | 'fail' | 'guard'; when?: string }
export interface InEdge { from: string; label: string; class: '' | 'fail' | 'guard' }
export interface CheckUse { name: string; fail?: number; code?: string; to?: string; outcome?: string; line: number }
export interface SetVar { var: string; value: string; line: number }
/** Node'suz son: `check ... outcome="..."` ya da `returns 409 outcome="..."`. Aday değil, yaprak. */
export interface Outcome { label: string; text: string; class: '' | 'fail' }
export interface Node {
  id: string; kind: Kind; layer: string; desc: string; doc?: string
  attrs: Record<string, string>; out: Edge[]; in: InEdge[]; checks: CheckUse[]; sets: SetVar[]; outcomes: Outcome[]; file: string
}
export interface VarDef { id: string; ty: string; desc?: string; source?: string; values?: string[] }
export interface CheckDef { id: string; desc?: string; reads?: string[]; fail_to?: string }
export interface PermDef { id: string; desc?: string; scope?: string; deny?: number; fail_to?: string }
export interface ViewDef { id: string; from: string; to: string; desc?: string }
export interface RemovedNode { id: string; kind: Kind; layer: string; desc: string; from: [string, string][] }
export interface DiffData {
  added: string[]; removed: RemovedNode[]; changed: string[]
  edges_removed: [string, string, string][]; edges_added: [string, string, string][]
}
export interface Data {
  schema: number; project: { name: string; layers: string[] }
  nodes: Node[]; vars: VarDef[]; checks: CheckDef[]; roots: string[]
  /** periyodik root'lar: id → "30s" */
  every: Record<string, string>
  perms: PermDef[]; views: ViewDef[]; diff?: DiffData
}

/** Aday: bir node'dan gidilebilecek yer. Aynı hedefe giden kenarlar tek adayda birleşir. */
export interface Candidate {
  to: string
  labels: { label: string; cls: string; when?: string }[]
  /** guard değerlendirmesi (UI'da state.vars ile doldurulur) */
  guard?: 'true' | 'false' | 'unknown'
  cls: '' | 'fail' | 'guard'
  /** API katmanı kapalıyken atlanan call */
  via?: Node
  /** diff: 'added' | 'removed' */
  diff?: 'added' | 'removed'
}

export class Graph {
  nodes = new Map<string, Node>()
  incoming = new Map<string, { from: string; edge: Edge }[]>()
  vars = new Map<string, VarDef>()
  checks = new Map<string, CheckDef>()
  roots: string[]
  every: Record<string, string>
  perms = new Map<string, PermDef>()
  layers: string[]
  diff?: DiffData
  removedNodes = new Map<string, RemovedNode>()
  changed = new Set<string>()
  added = new Set<string>()

  data: Data

  constructor(data: Data) {
    this.data = data
    for (const n of data.nodes) this.nodes.set(n.id, n)
    for (const n of data.nodes) for (const e of n.out) {
      if (!this.incoming.has(e.to)) this.incoming.set(e.to, [])
      this.incoming.get(e.to)!.push({ from: n.id, edge: e })
    }
    for (const v of data.vars) this.vars.set(v.id, v)
    for (const c of data.checks) this.checks.set(c.id, c)
    this.roots = data.roots
    this.every = data.every ?? {}
    for (const p of data.perms ?? []) this.perms.set(p.id, p)
    this.layers = data.project.layers
    this.diff = data.diff
    if (data.diff) {
      for (const r of data.diff.removed) this.removedNodes.set(r.id, r)
      this.changed = new Set(data.diff.changed)
      this.added = new Set(data.diff.added)
    }
  }

  get(id: string): Node | undefined { return this.nodes.get(id) }

  /** Node ya da diff'te silinmiş node (kart çizmek için yeter). */
  card(id: string): { id: string; kind: Kind; layer: string; desc: string; removed?: boolean } | undefined {
    const n = this.nodes.get(id)
    if (n) return n
    const r = this.removedNodes.get(id)
    if (r) return { ...r, removed: true }
    return undefined
  }

  /** Adaylar. `hideLayers`: atlanacak katmanlar (call node'ları "via" olur). `role`: guard filtresi. */
  nexts(id: string, opts: { hideLayers?: Set<string>; role?: string } = {}): Candidate[] {
    const res: Candidate[] = []
    const byKey = new Map<string, Candidate>()
    const push = (to: string, label: string, cls: string, when: string | undefined, via?: Node, diff?: 'added' | 'removed') => {
      if (opts.role && when && !roleAllows(when, opts.role)) return
      const k = to + '|' + (via?.id ?? '')
      let c = byKey.get(k)
      if (!c) { c = { to, labels: [], cls: '', via, diff }; byKey.set(k, c); res.push(c) }
      if (label) c.labels.push({ label, cls, when })
      if (cls === 'fail' || (cls === 'guard' && c.cls !== 'fail')) c.cls = cls as Candidate['cls']
      if (diff === 'added') c.diff = 'added'
    }
    // gizli katmandaki hedef atlanır, onun çıkışları "via" ile gelir (zincirleme, döngü korumalı)
    const walk = (from: string, via: Node | undefined, seen: Set<string>) => {
      const n = this.nodes.get(from); if (!n) return
      for (const e of n.out) {
        const t = this.nodes.get(e.to)
        const addedEdge = !via && this.diff?.edges_added.some(([f, to]) => f === from && to === e.to) ? 'added' : undefined
        if (t && opts.hideLayers?.has(t.layer) && t.id !== id) {
          if (seen.has(t.id)) continue
          seen.add(t.id)
          if (t.out.length) walk(t.id, via ?? t, seen); else push(e.to, e.label, e.class, e.when, via, addedEdge)
        } else push(e.to, e.label, e.class, e.when, via, addedEdge)
      }
    }
    walk(id, undefined, new Set())
    // diff: artık olmayan kenarlar ve silinmiş hedefler
    if (this.diff) {
      for (const [from, to, label] of this.diff.edges_removed) if (from === id) push(to, label, 'fail', undefined, undefined, 'removed')
      for (const r of this.diff.removed) for (const [from, label] of r.from) if (from === id) push(r.id, label, 'fail', undefined, undefined, 'removed')
    }
    return res
  }

  /** Root'tan (ya da verilen başlangıçtan) en kısa yol; BFS. */
  shortestPath(from: string, to: string): string[] | null {
    if (from === to) return [from]
    const prev = new Map<string, string | null>([[from, null]])
    const q = [from]
    while (q.length) {
      const cur = q.shift()!
      for (const e of this.nodes.get(cur)?.out ?? []) {
        if (prev.has(e.to)) continue
        prev.set(e.to, cur)
        if (e.to === to) {
          const p: string[] = []
          for (let c: string | null = to; c; c = prev.get(c) ?? null) p.unshift(c)
          return p
        }
        q.push(e.to)
      }
    }
    return null
  }

  pathFromRoots(to: string): string[] | null {
    let best: string[] | null = null
    for (const r of this.roots) {
      const p = this.shortestPath(r, to)
      if (p && (!best || p.length < best.length)) best = p
    }
    return best
  }

  /** Bir değişkeni yazan node'lar. */
  writers(v: string): { node: Node; set: SetVar }[] {
    const out: { node: Node; set: SetVar }[] = []
    for (const n of this.nodes.values()) for (const s of n.sets) if (s.var === v) out.push({ node: n, set: s })
    return out
  }

  /** Grup ağacı: ilk segment → ikinci segment → node'lar. */
  groups(): Map<string, Map<string, Node[]>> {
    const g = new Map<string, Map<string, Node[]>>()
    for (const n of this.nodes.values()) {
      const [a, b] = n.id.split('.')
      const sub = b ?? ''
      if (!g.has(a)) g.set(a, new Map())
      const m = g.get(a)!
      if (!m.has(sub)) m.set(sub, [])
      m.get(sub)!.push(n)
    }
    return g
  }
}

export function groupOf(id: string): string { return id.split('.').slice(0, 2).join('.') }
export function topOf(id: string): string { return id.split('.')[0] }

/** Guard'da `role == "x"` / `role != "x"` / `role in ...` kaba eşleme; anlaşılmazsa izin ver. */
export function roleAllows(when: string, role: string): boolean {
  const eq = when.match(/role\s*==\s*"([^"]+)"/)
  if (eq && eq[1] !== role) return false
  const ne = when.match(/role\s*!=\s*"([^"]+)"/)
  if (ne && ne[1] === role) return false
  return true
}

/** Basit subsequence fuzzy: skor ve eşleşen konumlar. */
export function fuzzy(hay: string, needle: string): { score: number; pos: number[] } | null {
  const h = hay.toLowerCase(); let i = 0, score = 0, last = -2; const pos: number[] = []
  for (const ch of needle.toLowerCase()) {
    const j = h.indexOf(ch, i); if (j < 0) return null
    score += (j === last + 1 ? 3 : 1) + (j === 0 ? 4 : 0); pos.push(j); last = j; i = j + 1
  }
  return { score: score - h.length * 0.02, pos }
}

export interface Hit { kind: string; layer: string; id: string; desc: string; score: number; pos: number[]; go?: string }

export function search(g: Graph, term: string, limit = 14): Hit[] {
  const t = term.trim(); if (!t) return []
  const out: Hit[] = []
  for (const n of g.nodes.values()) {
    const a = fuzzy(n.id, t), b = fuzzy(n.desc, t)
    const best = a && (!b || a.score >= b.score) ? { s: a.score + 2, pos: a.pos } : b ? { s: b.score, pos: [] } : null
    if (best) out.push({ kind: n.kind, layer: n.layer, id: n.id, desc: n.desc, score: best.s, pos: best.pos, go: n.id })
    for (const c of n.checks) { const m = fuzzy(c.name, t); if (m) out.push({ kind: 'check', layer: 'api', id: c.name, desc: '→ ' + n.id, score: m.score + 1, pos: m.pos, go: n.id }) }
  }
  for (const v of g.vars.values()) {
    const m = fuzzy(v.id, t)
    if (m) out.push({ kind: 'var', layer: 'var', id: v.id, desc: v.source ?? 'sets', score: m.score + 1, pos: m.pos, go: g.writers(v.id)[0]?.node.id })
  }
  return out.sort((x, y) => y.score - x.score).slice(0, limit)
}

export async function loadData(): Promise<Data> {
  const inline = (window as unknown as { __DUFLOW__: Data | null }).__DUFLOW__
  if (inline) return inline
  const r = await fetch('/duflow.json')
  if (!r.ok) throw new Error('duflow.json missing; run `duflow export > ui/public/duflow.json`')
  return r.json()
}
