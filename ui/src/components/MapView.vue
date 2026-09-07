<script setup lang="ts">
// Map: grup seviyesi genel bakış. Yerleşim: SCC condensation + longest-path katmanı
// (düz BFS hub'lar yüzünden 2 sütuna çöküyordu), sütun başına MAXROW kart, taşan katman sarılır.
// Teller varsayılan çok soluk; bir karta hover → yalnız o grubun in/out telleri parlar, gerisi söner.
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { graph, startAt } from '../walk'
import { topOf, groupOf } from '../graph'

const level = ref<string | null>(null) // null: üst gruplar; "deploy": deploy'un alt grupları
const focus = ref<string | null>(null) // hover'daki grup (yerel, store'a koyma)
const stage = ref<HTMLElement>()
const svg = ref<SVGSVGElement>()
const MAXROW = 9

interface GNode { key: string; label: string; count: number; layers: Record<string, number>; nodes: string[]; diff: { a: number; r: number; c: number } }
interface GEdge { from: string; to: string; n: number }

const model = computed(() => {
  const g = graph.value; if (!g) return { nodes: [] as GNode[], edges: [] as GEdge[], cols: [] as GNode[][] }
  const keyOf = (id: string) => (level.value ? (topOf(id) === level.value ? groupOf(id) : topOf(id)) : topOf(id))
  const nodes = new Map<string, GNode>()
  for (const n of g.nodes.values()) {
    const k = keyOf(n.id)
    if (level.value && !k.startsWith(level.value)) continue
    if (!nodes.has(k)) nodes.set(k, { key: k, label: k, count: 0, layers: {}, nodes: [], diff: { a: 0, r: 0, c: 0 } })
    const gn = nodes.get(k)!
    gn.count++; gn.layers[n.layer] = (gn.layers[n.layer] ?? 0) + 1; gn.nodes.push(n.id)
    if (g.added.has(n.id)) gn.diff.a++
    if (g.changed.has(n.id)) gn.diff.c++
  }
  for (const r of g.removedNodes.values()) { const k = keyOf(r.id); nodes.get(k) && nodes.get(k)!.diff.r++ }
  const edges = new Map<string, GEdge>()
  for (const n of g.nodes.values()) for (const e of n.out) {
    const a = keyOf(n.id), b = keyOf(e.to)
    if (a === b || !nodes.has(a) || !nodes.has(b)) continue
    const k = a + '>' + b
    if (!edges.has(k)) edges.set(k, { from: a, to: b, n: 0 })
    edges.get(k)!.n++
  }
  const out = new Map<string, GEdge[]>()
  for (const k of nodes.keys()) out.set(k, [])
  for (const e of edges.values()) out.get(e.from)!.push(e)

  // SCC (Tarjan, iteratif) → döngüler tek düğüme iner
  const comp = new Map<string, number>()
  {
    const ix = new Map<string, number>(), low = new Map<string, number>(), on = new Set<string>()
    const st: string[] = []; let idx = 0, c = 0
    for (const root of nodes.keys()) {
      if (ix.has(root)) continue
      const work: [string, number][] = [[root, 0]]
      while (work.length) {
        const fr = work[work.length - 1]
        const [v, i] = fr
        if (i === 0) { ix.set(v, idx); low.set(v, idx); idx++; st.push(v); on.add(v) }
        const es = out.get(v)!
        if (i < es.length) {
          fr[1]++
          const w = es[i].to
          if (!ix.has(w)) work.push([w, 0])
          else if (on.has(w)) low.set(v, Math.min(low.get(v)!, ix.get(w)!))
        } else {
          work.pop()
          if (work.length) { const p = work[work.length - 1][0]; low.set(p, Math.min(low.get(p)!, low.get(v)!)) }
          if (low.get(v) === ix.get(v)) { let w: string; do { w = st.pop()!; on.delete(w); comp.set(w, c) } while (w !== v); c++ }
        }
      }
    }
  }
  // condensation üzerinde longest-path katmanı
  const cOut = new Map<number, Set<number>>(), indeg = new Map<number, number>()
  for (const c of comp.values()) { if (!cOut.has(c)) cOut.set(c, new Set()); if (!indeg.has(c)) indeg.set(c, 0) }
  for (const e of edges.values()) {
    const a = comp.get(e.from)!, b = comp.get(e.to)!
    if (a !== b && !cOut.get(a)!.has(b)) { cOut.get(a)!.add(b); indeg.set(b, indeg.get(b)! + 1) }
  }
  const lv = new Map<number, number>()
  const q: number[] = []
  for (const [c, d] of indeg) if (!d) { lv.set(c, 0); q.push(c) }
  while (q.length) {
    const c = q.shift()!
    for (const n of cOut.get(c)!) {
      lv.set(n, Math.max(lv.get(n) ?? 0, lv.get(c)! + 1))
      indeg.set(n, indeg.get(n)! - 1)
      if (!indeg.get(n)) q.push(n)
    }
  }
  const buckets = new Map<number, GNode[]>()
  for (const n of nodes.values()) {
    const d = lv.get(comp.get(n.key)!) ?? 0
    ;(buckets.get(d) ?? buckets.set(d, []).get(d)!).push(n)
  }
  const cols: GNode[][] = []
  for (const d of [...buckets.keys()].sort((a, b) => a - b)) {
    const b = buckets.get(d)!.sort((x, y) => y.count - x.count)
    for (let i = 0; i < b.length; i += MAXROW) cols.push(b.slice(i, i + MAXROW))
  }
  return { nodes: [...nodes.values()], edges: [...edges.values()], cols }
})

function open(n: GNode) {
  if (!level.value && n.count > 1 && n.nodes.some((id) => id.split('.').length > 2)) { level.value = n.key; focus.value = null; return }
  // node seviyesine in: grubun root'tan erişilen ilk node'u
  const g = graph.value!
  const first = n.nodes.map((id) => ({ id, p: g.pathFromRoots(id) })).filter((x) => x.p).sort((a, b) => a.p!.length - b.p!.length)[0]
  startAt(first?.id ?? n.nodes[0])
}

function draw() {
  const s = svg.value, st = stage.value; if (!s || !st) return
  const sr = st.getBoundingClientRect()
  const pos = new Map<string, DOMRect>()
  for (const el of st.querySelectorAll<HTMLElement>('.gcard')) pos.set(el.dataset.key!, el.getBoundingClientRect())
  const f = focus.value
  const parts: string[] = []
  const near = new Set<string>()
  for (const e of model.value.edges) {
    const hot = !!f && (e.from === f || e.to === f)
    if (f && !hot) continue
    const A = pos.get(e.from), B = pos.get(e.to); if (!A || !B) continue
    if (hot) { near.add(e.from); near.add(e.to) }
    const back = B.left < A.left
    const ax = (back ? A.left : A.right) - sr.left, ay = A.top + A.height / 2 - sr.top
    const bx = (back ? B.right : B.left) - sr.left, by = B.top + B.height / 2 - sr.top
    const mx = (ax + bx) / 2
    const w = Math.min(5, 1 + Math.log2(e.n + 1))
    const color = !hot ? 'var(--edge)' : e.from === f ? 'var(--api)' : 'var(--ui)'
    parts.push(`<path d="M${ax},${ay} C${mx},${ay} ${mx},${by} ${bx},${by}" fill="none" stroke="${color}" stroke-width="${w}" opacity="${hot ? .9 : .16}" ${back ? 'stroke-dasharray="6 5"' : ''}/>`)
    if (hot) parts.push(`<text x="${mx}" y="${(ay + by) / 2 - 4}" text-anchor="middle">${e.n}</text>`)
  }
  s.innerHTML = parts.join('')
  for (const el of st.querySelectorAll<HTMLElement>('.gcard')) {
    const k = el.dataset.key!
    el.classList.toggle('dim', !!f && k !== f && !near.has(k))
    el.classList.toggle('hot', k === f)
  }
}
onMounted(() => nextTick(draw))
watch(model, () => nextTick(draw))
watch(focus, draw)
window.addEventListener('resize', draw)
</script>

<template>
  <div class="map" ref="stage">
    <div class="crumbs">
      <button @click="level = null; focus = null" :class="{ on: !level }">{{ graph?.data.project.name }}</button>
      <template v-if="level"><span>›</span><button class="on">{{ level }}</button></template>
      <span class="hint">hover a card to trace its edges · click to enter</span>
    </div>
    <svg ref="svg" class="wires"></svg>
    <div class="cols">
      <div class="col" v-for="(col, i) in model.cols" :key="i">
        <button v-for="n in col" :key="n.key" class="gcard" :data-key="n.key" @click="open(n)"
          @mouseenter="focus = n.key" @mouseleave="focus = null">
          <div class="head"><span class="mono">{{ n.label }}</span><span class="count">{{ n.count }}</span></div>
          <div class="bars">
            <span v-for="(c, l) in n.layers" :key="l" class="bar" :data-layer="l" :style="{ flex: c }" :title="`${l}: ${c}`"></span>
          </div>
          <div class="diff" v-if="n.diff.a || n.diff.r || n.diff.c">
            <span v-if="n.diff.a" class="chip added">+{{ n.diff.a }}</span>
            <span v-if="n.diff.r" class="chip removed">−{{ n.diff.r }}</span>
            <span v-if="n.diff.c" class="chip changed">~{{ n.diff.c }}</span>
          </div>
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.map { flex: 1; position: relative; overflow: auto; padding: 16px 40px; }
.crumbs { display: flex; align-items: center; gap: 8px; font-size: 13px; color: var(--muted); margin-bottom: 18px; position: relative; z-index: 2; }
.crumbs button { color: var(--muted); } .crumbs button.on { color: var(--text); font-weight: 600; }
.crumbs .hint { margin-left: auto; font-size: 12px; color: var(--faint); }
.wires { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
.wires :deep(text) { font: 11px "Instrument Sans", system-ui, sans-serif; fill: var(--muted); paint-order: stroke; stroke: var(--bg); stroke-width: 4px; }
.cols { display: flex; gap: 64px; align-items: flex-start; position: relative; z-index: 1; }
.col { display: flex; flex-direction: column; gap: 10px; width: 156px; }
.gcard { background: var(--surface); border: 1px solid var(--line); border-radius: 8px; padding: 7px 10px; text-align: left; display: flex; flex-direction: column; gap: 6px; transition: border-color .18s, opacity .18s, box-shadow .18s; }
.gcard.dim { opacity: .2; }
.gcard.hot, .gcard:hover { border-color: var(--text); box-shadow: var(--shadow); }
.head { display: flex; justify-content: space-between; font-size: 12px; }
.head .count { color: var(--muted); font-size: 11px; }
.bars { display: flex; height: 3px; border-radius: 2px; overflow: hidden; gap: 1px; }
.bar { background: var(--faint); }
.bar[data-layer="ui"] { background: var(--ui); } .bar[data-layer="api"] { background: var(--api); } .bar[data-layer="domain"] { background: var(--domain); }
.diff { display: flex; gap: 4px; }
</style>
