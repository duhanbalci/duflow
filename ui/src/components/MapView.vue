<script setup lang="ts">
// Map: grup seviyesi genel bakış. Katmanlı yerleşim: root gruplarından BFS derinliği = sütun.
import { computed, nextTick, onMounted, ref, watch } from 'vue'
import { graph, startAt } from '../walk'
import { topOf, groupOf } from '../graph'

const level = ref<string | null>(null) // null: üst gruplar; "deploy": deploy'un alt grupları
const stage = ref<HTMLElement>()
const svg = ref<SVGSVGElement>()

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
  // sütun = BFS derinliği (root gruplarından)
  const depth = new Map<string, number>()
  const q: string[] = []
  for (const r of g.roots) { const k = keyOf(r); if (nodes.has(k) && !depth.has(k)) { depth.set(k, 0); q.push(k) } }
  while (q.length) {
    const cur = q.shift()!
    for (const e of edges.values()) if (e.from === cur && !depth.has(e.to)) { depth.set(e.to, depth.get(cur)! + 1); q.push(e.to) }
  }
  let maxd = 0
  for (const d of depth.values()) maxd = Math.max(maxd, d)
  const cols: GNode[][] = []
  for (const n of nodes.values()) {
    const d = depth.get(n.key) ?? maxd + 1
    ;(cols[d] ??= []).push(n)
  }
  for (const c of cols) c?.sort((a, b) => b.count - a.count)
  return { nodes: [...nodes.values()], edges: [...edges.values()], cols: cols.filter(Boolean) }
})

function open(n: GNode) {
  if (!level.value && n.count > 1 && n.nodes.some((id) => id.split('.').length > 2)) { level.value = n.key; return }
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
  const parts: string[] = []
  for (const e of model.value.edges) {
    const A = pos.get(e.from), B = pos.get(e.to); if (!A || !B) continue
    const back = B.left < A.left
    const ax = (back ? A.left : A.right) - sr.left, ay = A.top + A.height / 2 - sr.top
    const bx = (back ? B.right : B.left) - sr.left, by = B.top + B.height / 2 - sr.top
    const mx = (ax + bx) / 2
    const w = Math.min(6, 1 + Math.log2(e.n + 1))
    parts.push(`<path d="M${ax},${ay} C${mx},${ay} ${mx},${by} ${bx},${by}" fill="none" stroke="var(--edge)" stroke-width="${w}" opacity=".6" ${back ? 'stroke-dasharray="6 5"' : ''}/>`)
    parts.push(`<text x="${mx}" y="${(ay + by) / 2 - 4}" text-anchor="middle">${e.n}</text>`)
  }
  s.innerHTML = parts.join('')
}
onMounted(() => nextTick(draw))
watch(model, () => nextTick(draw))
window.addEventListener('resize', draw)
</script>

<template>
  <div class="map" ref="stage">
    <div class="crumbs">
      <button @click="level = null" :class="{ on: !level }">{{ graph?.data.project.name }}</button>
      <template v-if="level"><span>›</span><button class="on">{{ level }}</button></template>
      <span class="hint">click a group card to enter · Walk starts at node level</span>
    </div>
    <svg ref="svg" class="wires"></svg>
    <div class="cols">
      <div class="col" v-for="(col, i) in model.cols" :key="i">
        <button v-for="n in col" :key="n.key" class="gcard" :data-key="n.key" @click="open(n)">
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
.cols { display: flex; gap: 90px; align-items: flex-start; position: relative; z-index: 1; }
.col { display: flex; flex-direction: column; gap: 16px; width: 200px; }
.gcard { background: var(--surface); border: 1px solid var(--line); border-radius: 10px; padding: 10px 12px; text-align: left; display: flex; flex-direction: column; gap: 8px; transition: border-color .18s, box-shadow .18s; }
.gcard:hover { border-color: var(--text); box-shadow: var(--shadow); }
.head { display: flex; justify-content: space-between; font-size: 13px; }
.head .count { color: var(--muted); font-size: 12px; }
.bars { display: flex; height: 4px; border-radius: 2px; overflow: hidden; gap: 1px; }
.bar { background: var(--faint); }
.bar[data-layer="ui"] { background: var(--ui); } .bar[data-layer="api"] { background: var(--api); } .bar[data-layer="domain"] { background: var(--domain); }
.diff { display: flex; gap: 4px; }
</style>
