<script setup lang="ts">
// Walk: soldan sağa akış. geçmiş(1) · şimdi · adaylar · ufuk.
// Hover yerel; yalnız ufuk ve teller güncellenir. FLIP yalnız izinli sütun geçişlerinde.
import { computed, nextTick, onMounted, onUnmounted, onUpdated, ref, watch } from 'vue'
import Card from './Card.vue'
import { groupOf, type Candidate } from '../graph'
import { candidates, current, previous, go, rewindTo, state, stopReplay, replayHover, graph } from '../walk'

const stage = ref<HTMLElement>()
const cols = ref<HTMLElement>()
const wires = ref<SVGSVGElement>()
const hot = ref('')
const HORIZON_MAX = 5

const nexts = computed(() => candidates(current.value))
const horizon = computed(() => (hot.value ? candidates(hot.value) : []))

/** Kalabalık aday sütunu: >6 ise gruplu */
const grouped = computed(() => {
  const ns = nexts.value
  if (ns.length <= 6 && !state.filter) return null
  const groups = new Map<string, Candidate[]>()
  for (const c of ns) {
    const gk = c.diff === 'removed' ? 'artık yok' : c.cls === 'fail' ? 'hata dalları' : groupOf(c.to)
    if (!groups.has(gk)) groups.set(gk, [])
    groups.get(gk)!.push(c)
  }
  const f = state.filter.toLowerCase()
  const out: { name: string; list: Candidate[]; open: boolean; fail: boolean }[] = []
  for (const [name, list0] of groups) {
    const list = f ? list0.filter((c) => c.to.includes(f) || (graph.value?.get(c.to)?.desc ?? '').toLowerCase().includes(f)) : list0
    if (!list.length) continue
    out.push({ name, list, open: state.openGroups.has(name) || !!f, fail: name === 'hata dalları' || name === 'artık yok' })
  }
  // hata/artık yok en sona
  out.sort((a, b) => Number(a.fail) - Number(b.fail))
  return out
})

function toggleGroup(name: string) {
  const s = new Set(state.openGroups)
  if (s.has(name)) s.delete(name); else s.add(name)
  state.openGroups = s
}

watch(replayHover, (v) => { setHot(v); if (v) nextTick(() => cols.value?.querySelector<HTMLElement>('.col.next .card.hot')?.scrollIntoView({ block: 'nearest' })) })

function setHot(id: string) {
  if (hot.value === id) return
  hot.value = id
  nextTick(drawWires)
}
function onOver(e: MouseEvent) {
  const c = (e.target as HTMLElement).closest<HTMLElement>('.col.next .card')
  if (c) setHot(c.dataset.id ?? '')
}
function onClick(e: MouseEvent) {
  const c = (e.target as HTMLElement).closest<HTMLElement>('.card')
  if (!c) return
  const col = c.closest<HTMLElement>('.col')?.dataset.col
  const id = c.dataset.id ?? ''
  if (col === 'next') {
    if (c.classList.contains('removed')) return
    stopReplay(); hot.value = ''; go(id)
  } else if (col === 'past') { stopReplay(); hot.value = ''; rewindTo(id) }
}

// ---- FLIP ----
type Snap = { id: string; col: string; rect: DOMRect; html: string; layer: string }
let pending: Snap[] | null = null
const ALLOWED: Record<string, string[]> = { now: ['next', 'past'], past: ['now'], next: ['horizon', 'now'], horizon: [] }

watch(() => state.hist.slice(), () => {
  if (!cols.value) return
  pending = [...cols.value.querySelectorAll<HTMLElement>('.card')].map((c) => ({
    id: c.dataset.id ?? '', col: c.closest<HTMLElement>('.col')?.dataset.col ?? '', rect: c.getBoundingClientRect(), html: c.outerHTML, layer: c.dataset.layer ?? '',
  }))
  hot.value = ''
}, { flush: 'pre' })

onUpdated(() => {
  if (!pending || !cols.value || !stage.value) return
  const old = pending; pending = null
  const moved = new Set<string>()
  const sr = stage.value.getBoundingClientRect()
  for (const c of cols.value.querySelectorAll<HTMLElement>('.card')) {
    const col = c.closest<HTMLElement>('.col')?.dataset.col ?? ''
    const from = old.find((o) => o.id === c.dataset.id && (ALLOWED[col] ?? []).includes(o.col))
    if (from) {
      moved.add(from.id + '@' + from.col)
      const to = c.getBoundingClientRect()
      const dx = from.rect.left - to.left, dy = from.rect.top - to.top, s = from.rect.width / to.width
      if (Math.abs(dx) + Math.abs(dy) > 1 || Math.abs(s - 1) > .01) {
        c.style.transition = 'none'; c.style.transformOrigin = 'left top'; c.style.transform = `translate(${dx}px,${dy}px) scale(${s})`
        requestAnimationFrame(() => requestAnimationFrame(() => { c.style.transition = 'transform .38s cubic-bezier(.2,.8,.2,1), opacity .22s'; c.style.transform = '' }))
      }
    } else if (col !== 'horizon') c.classList.add('enter')
  }
  for (const o of old) {
    if (moved.has(o.id + '@' + o.col) || o.col === 'horizon') continue
    const wrap = document.createElement('div'); wrap.innerHTML = o.html
    const el = wrap.firstElementChild as HTMLElement
    el.classList.add('ghost'); el.style.left = `${o.rect.left - sr.left}px`; el.style.top = `${o.rect.top - sr.top}px`; el.style.width = `${o.rect.width}px`
    stage.value.appendChild(el)
    requestAnimationFrame(() => requestAnimationFrame(() => { el.style.opacity = '0'; el.style.transform = 'translateY(18px) scale(.96)' }))
    setTimeout(() => el.remove(), 320)
  }
  if (wires.value) wires.value.style.opacity = '0'
  setTimeout(drawWires, 390)
})

// ---- teller + etiketler (SVG katmanı) ----
function drawWires() {
  const svg = wires.value, st = stage.value, cl = cols.value
  if (!svg || !st || !cl) return
  const sr = st.getBoundingClientRect()
  const P = (el: Element) => { const r = el.getBoundingClientRect(); return { l: r.left - sr.left, r: r.right - sr.left, y: r.top - sr.top + r.height / 2 } }
  const q = (col: string, id: string) => cl.querySelector<HTMLElement>(`.col.${col} .card[data-id="${CSS.escape(id)}"]`)
  const parts: string[] = []
  const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;')
  const link = (a: Element | null, b: HTMLElement | null, hotp: boolean) => {
    if (!a || !b) return
    const A = P(a), B = P(b), mx = (A.r + B.l) / 2
    const cls = b.dataset.cls ?? ''
    const removed = b.classList.contains('removed')
    const guard = b.dataset.guard ?? ''
    const stroke = hotp ? 'var(--edge-hot)' : removed ? 'var(--bad)' : guard === 'true' ? 'var(--good)' : 'var(--edge)'
    parts.push(`<path d="M${A.r},${A.y} C${mx},${A.y} ${mx},${B.y} ${B.l},${B.y}" fill="none" stroke="${stroke}" stroke-width="${hotp || guard === 'true' ? 1.6 : 1.2}" ${cls === 'guard' || removed ? 'stroke-dasharray="5 4"' : ''} opacity="${hotp ? 1 : guard === 'false' ? .3 : .7}"/>`)
    let labels: { label: string; cls: string }[] = []
    try { labels = JSON.parse(b.dataset.labels ?? '[]') } catch { /* boş */ }
    const y0 = B.y - (labels.length - 1) * 7
    const maxW = Math.max(0, B.l - A.r - 20)
    labels.forEach((l, i) => parts.push(`<text x="${B.l - 8}" y="${y0 + i * 14 + 4}" text-anchor="end" class="${l.cls}" opacity="${hotp ? 1 : .85}" data-max="${maxW}"><title>${esc(l.label)}</title>${esc(l.label)}</text>`))
  }
  const nowEl = q('now', current.value)
  if (previous.value) link(q('past', previous.value), nowEl, true)
  for (const c of cl.querySelectorAll<HTMLElement>('.col.next .card')) link(nowEl, c, c.dataset.id === hot.value)
  if (hot.value) { const h = q('next', hot.value); for (const c of cl.querySelectorAll<HTMLElement>('.col.horizon .card')) link(h, c, false) }
  svg.innerHTML = parts.join('')
  // sığmayan etiketi kısalt (ölçüm DOM'a girdikten sonra)
  for (const t of svg.querySelectorAll<SVGTextElement>('text[data-max]')) {
    const max = Number(t.dataset.max)
    const title = t.querySelector('title')
    let text = t.textContent?.replace(title?.textContent ?? '', '') ?? ''
    const node = [...t.childNodes].find((n) => n.nodeType === 3)
    if (!node) continue
    let guard = 0
    while (t.getComputedTextLength() > max && text.length > 3 && guard++ < 60) {
      text = text.slice(0, -2).trimEnd() + '…'
      node.textContent = text
    }
  }
  svg.style.opacity = '1'
}

let ro: ResizeObserver | undefined
onMounted(() => {
  nextTick(drawWires)
  ro = new ResizeObserver(() => drawWires()); ro.observe(stage.value!)
  cols.value?.addEventListener('scroll', drawWires, true)
})
onUnmounted(() => ro?.disconnect())
watch([() => state.hiddenLayers, () => state.role, () => state.filter, () => state.openGroups, () => ({ ...state.vars })], () => nextTick(drawWires))
</script>

<template>
  <div class="stage" ref="stage">
    <svg ref="wires" class="wires"></svg>
    <div class="cols" ref="cols" @mouseover="onOver" @mouseleave="setHot('')" @click="onClick">
      <div class="col past" data-col="past">
        <Card v-if="previous" :id="previous" col="past" />
        <div v-else class="empty">başlangıç</div>
      </div>
      <div class="col now" data-col="now">
        <Card :id="current" col="now" />
      </div>
      <div class="col next" data-col="next">
        <div v-if="!nexts.length" class="empty">çıkış yok</div>
        <template v-else-if="!grouped">
          <Card v-for="c in nexts" :key="c.to + (c.via?.id ?? '')" :id="c.to" col="next" :cand="c" :hot="hot === c.to" />
        </template>
        <template v-else>
          <input class="nfilter mono" :placeholder="`${nexts.length} çıkış · süz…`" v-model="state.filter" />
          <template v-for="grp in grouped" :key="grp.name">
            <button class="ghead" :class="{ fail: grp.fail }" @click.stop="toggleGroup(grp.name)">
              <span class="mono">{{ grp.name }}</span><span>{{ grp.list.length }}</span>
            </button>
            <Card v-for="c in (grp.open ? grp.list : grp.list.slice(0, 2))" :key="c.to + (c.via?.id ?? '')" :id="c.to" col="next" :cand="c" :hot="hot === c.to" />
            <button v-if="!grp.open && grp.list.length > 2" class="more" @click.stop="toggleGroup(grp.name)">+{{ grp.list.length - 2 }} daha</button>
          </template>
        </template>
      </div>
      <div class="col horizon" data-col="horizon">
        <Card v-for="c in horizon.slice(0, HORIZON_MAX)" :key="c.to + (c.via?.id ?? '')" :id="c.to" col="horizon" :cand="c" class="fade" />
        <div v-if="horizon.length > HORIZON_MAX" class="empty fade">+{{ horizon.length - HORIZON_MAX }} çıkış daha</div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stage { flex: 1; position: relative; overflow: hidden; display: flex; align-items: center; padding: 0 40px; }
.wires { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; transition: opacity .18s; }
.wires :deep(text) { font: 11px "Instrument Sans", system-ui, sans-serif; fill: var(--muted); paint-order: stroke; stroke: var(--bg); stroke-width: 5px; stroke-linejoin: round; }
.wires :deep(text.guard) { fill: var(--domain); }
.wires :deep(text.fail) { fill: var(--bad); }
.cols { display: flex; align-items: center; gap: 96px; position: relative; z-index: 1; }
.col { display: flex; flex-direction: column; gap: 14px; align-items: stretch; }
.col.past { width: 200px; } .col.now { width: 340px; } .col.next { width: 260px; } .col.horizon { width: 210px; }
.col.next { max-height: calc(100vh - 120px); overflow: auto; padding: 4px; }
.empty { color: var(--faint); font-size: 12px; text-align: center; padding: 20px 8px; border: 1px dashed var(--line); border-radius: 10px; }
.nfilter { background: var(--surface2); border: 1px solid var(--line); border-radius: 6px; padding: 5px 8px; font-size: 12px; outline: 0; }
.nfilter:focus { border-color: var(--text); }
.ghead { display: flex; justify-content: space-between; font-size: 11px; letter-spacing: .06em; text-transform: uppercase; color: var(--muted); padding: 6px 2px 0; margin-top: 4px; border-top: 1px solid var(--line); }
.ghead.fail { color: var(--bad); }
.ghead:hover { color: var(--text); }
.more { font-size: 12px; color: var(--faint); text-align: left; padding: 2px 12px; }
.more:hover { color: var(--text); }
:deep(.ghost) { position: absolute; pointer-events: none; transition: opacity .26s, transform .26s; z-index: 0; }
</style>
