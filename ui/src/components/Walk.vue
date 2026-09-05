<script setup lang="ts">
// Walk, konveyör modeli: 5 slotlu bant [gizli-sol][geçmiş][şimdi][adaylar][ufuk/gelen].
// İleri: seçilen adayın devamı sağ slota statik çizilir, bant tek translateX ile sola kayar,
// bitince state ilerler ve bant sıfırlanır (görüntü birebir aynı → glitch yok).
// Geri: simetrik. Animasyon sırasında ölçüm/çizim yapılmaz; teller bantla birlikte kayar.
import { computed, nextTick, onMounted, onUnmounted, ref, watch } from 'vue'
import Card from './Card.vue'
import CandList from './CandList.vue'
import type { Candidate } from '../graph'
import { candidates, current, previous, go, back, rewindTo, state, stopReplay, replayHover, nav } from '../walk'

const W = 300, G = 80, STEP = W + G
const HORIZON_MAX = 5

const stage = ref<HTMLElement>()
const track = ref<HTMLElement>()
const wires = ref<SVGSVGElement>()
const hot = ref('')
const picked = ref('')                        // ileri animasyonunda seçilen aday
const incoming = ref<Candidate[] | null>(null) // seçilenin devamı (sağ slot)
const leftIn = ref('')                        // geri animasyonunda sol slota gelen
const shift = ref(0)                          // -STEP ileri, +STEP geri
const animating = ref(false)
const noTransition = ref(false)
const phase = ref<'' | 'fwd' | 'back'>('')
const backCands = ref<Candidate[]>([])       // geri: "şimdi" slotunda açılan gelecek aday listesi
const backSel = ref('')                      // geri: listede önce "şimdi" stilinde duran eski şimdi

/** Layout animasyonu sürerken telleri her karede yeniden çiz (transform yok → ölçüm doğru). */
function drawLoop(ms: number, then?: () => void) {
  const t0 = performance.now()
  const f = () => { drawWires(); if (performance.now() - t0 < ms) requestAnimationFrame(f); else then?.() }
  requestAnimationFrame(f)
}

const nexts = computed(() => candidates(current.value))
const horizon = computed(() => (hot.value && !incoming.value ? candidates(hot.value) : []))
const prevPrev = computed(() => state.hist[state.hist.length - 3] ?? '')

nav.go = forward

function setHot(id: string) {
  if (hot.value === id || animating.value) return
  hot.value = id
  nextTick(drawWires)
}
watch(replayHover, (v) => setHot(v))

function onOver(e: MouseEvent) {
  const c = (e.target as HTMLElement).closest<HTMLElement>('.slot.next .card')
  if (c) setHot(c.dataset.id ?? '')
}
function onClick(e: MouseEvent) {
  if (animating.value) return
  const c = (e.target as HTMLElement).closest<HTMLElement>('.card')
  if (!c) return
  const slot = c.closest<HTMLElement>('.slot')?.dataset.slot
  const id = c.dataset.id ?? ''
  if (slot === 'next') { if (!c.classList.contains('removed')) { stopReplay(); forward(id) } }
  else if (slot === 'past') { stopReplay(); if (id === previous.value) backward(); else rewindTo(id) }
}

// Tek faz: kardeşler kapanır + seçilen büyüyüp ortalanır + eski şimdi küçülür + bant kayar; teller her kare.
function forward(id: string) {
  if (animating.value) return
  animating.value = true
  hot.value = ''
  phase.value = 'fwd'
  picked.value = id
  incoming.value = candidates(id)
  state.openGroups = new Set(); state.filter = ''
  nextTick(() => { shift.value = -STEP; drawLoop(460) })
}

function backward() {
  if (animating.value || state.hist.length < 2) return
  animating.value = true
  hot.value = ''
  phase.value = 'back'
  leftIn.value = prevPrev.value
  backCands.value = candidates(previous.value)
  backSel.value = current.value               // önce yalnız eski şimdi görünür (şimdi stilinde)
  nextTick(() => {
    drawWires()
    void track.value?.offsetHeight   // kapalı başlangıç durumu layout'a işlensin (rAF'a bağımlı olma)
    backSel.value = ''; shift.value = STEP; drawLoop(460)
  })
}

function onShiftEnd(e: TransitionEvent) {
  if (e.target !== track.value || e.propertyName !== 'transform' || shift.value === 0) return
  const dir = shift.value
  // bant sıfırlanırken transition kapalı: kaymış görüntü ile commit sonrası görüntü birebir aynı
  noTransition.value = true
  if (dir < 0) go(picked.value); else back()
  picked.value = ''; incoming.value = null; leftIn.value = ''; backCands.value = []; backSel.value = ''
  phase.value = ''
  shift.value = 0
  nextTick(() => {
    drawWires()
    void track.value?.offsetHeight   // sıfırlanmış transform layout'a işlensin, sonra transition'lar geri
    noTransition.value = false; animating.value = false
  })
}

// hist dışarıdan değişirse (arama, root seç, geri tuşu) anında yeniden çiz
watch(() => state.hist.slice(), () => { if (!animating.value) { hot.value = ''; nextTick(drawWires) } })
watch([() => state.hiddenLayers, () => state.role, () => state.filter, () => state.openGroups, () => ({ ...state.vars })], () => { if (!animating.value) nextTick(drawWires) })

// ---- teller (bant içinde, bantla kayar) ----
// Key'li: aynı kenar (fromId>toId) çerçeveler arasında korunur, yalnız konumu güncellenir;
// yeni kenar solarak belirir, giden solarak gider. Böylece commit anında tel "çat" diye değişmez.
const wireEls = new Map<string, SVGElement>()
const SVGNS = 'http://www.w3.org/2000/svg'
function drawWires() {
  const svg = wires.value, tr = track.value
  if (!svg || !tr) return
  const tb = tr.getBoundingClientRect()
  const P = (el: Element) => { const r = el.getBoundingClientRect(); return { l: r.left - tb.left, r: r.right - tb.left, y: r.top - tb.top + r.height / 2 } }
  const q = (slot: string, id: string) => tr.querySelector<HTMLElement>(`.slot.${slot} .card[data-id="${CSS.escape(id)}"]`)
  const all = (slot: string) => [...tr.querySelectorAll<HTMLElement>(`.slot.${slot} .card`)]
  const seen = new Set<string>()
  const upsert = (key: string, tag: 'path' | 'text', opacity: number, apply: (el: SVGElement, fresh: boolean) => void) => {
    let el = wireEls.get(key)
    const fresh = !el
    if (!el) {
      el = document.createElementNS(SVGNS, tag)
      el.style.opacity = '0'; el.style.transition = 'opacity .32s'
      svg.appendChild(el); wireEls.set(key, el)
    }
    apply(el, fresh)
    const target = String(opacity)
    if (fresh) void (el as unknown as SVGGraphicsElement).getBoundingClientRect()   // başlangıç opaklığı işlensin
    if (el.style.opacity !== target) el.style.opacity = target
    seen.add(key)
  }
  const link = (a: HTMLElement | null, b: HTMLElement | null, hotp: boolean, fade = 1) => {
    if (!a || !b || b.classList.contains('collapsed')) return
    const A = P(a), B = P(b), mx = (A.r + B.l) / 2
    const cls = b.dataset.cls ?? ''
    const removed = b.classList.contains('removed')
    const guard = b.dataset.guard ?? ''
    const key = `${a.dataset.id}>${b.dataset.id}`
    const stroke = hotp ? 'var(--edge-hot)' : removed ? 'var(--bad)' : guard === 'true' ? 'var(--good)' : 'var(--edge)'
    const op = (hotp ? 1 : guard === 'false' ? .3 : .7) * fade
    upsert('p:' + key, 'path', op, (el) => {
      el.setAttribute('d', `M${A.r},${A.y} C${mx},${A.y} ${mx},${B.y} ${B.l},${B.y}`)
      el.setAttribute('fill', 'none'); el.setAttribute('stroke', stroke); el.setAttribute('stroke-width', hotp || guard === 'true' ? '1.6' : '1.2')
      if (cls === 'guard' || removed) el.setAttribute('stroke-dasharray', '5 4'); else el.removeAttribute('stroke-dasharray')
    })
    let labels: { label: string; cls: string }[] = []
    try { labels = JSON.parse(b.dataset.labels ?? '[]') } catch { /* boş */ }
    const y0 = B.y - (labels.length - 1) * 7
    const maxW = Math.max(0, B.l - A.r - 20)
    labels.forEach((l, i) => upsert(`t:${key}#${i}`, 'text', (hotp ? 1 : .85) * fade, (el) => {
      const t = el as SVGTextElement
      t.setAttribute('x', String(B.l - 8)); t.setAttribute('y', String(y0 + i * 14 + 4)); t.setAttribute('text-anchor', 'end'); t.setAttribute('class', l.cls)
      if (t.dataset.full !== l.label || t.dataset.max !== String(maxW)) {
        t.dataset.full = l.label; t.dataset.max = String(maxW)
        t.textContent = l.label
        let text = l.label, guardN = 0
        while (t.getComputedTextLength() > maxW && text.length > 3 && guardN++ < 60) { text = text.slice(0, -2).trimEnd() + '…'; t.textContent = text }
        const title = document.createElementNS(SVGNS, 'title'); title.textContent = l.label; t.appendChild(title)
      }
    }))
  }
  const nowEl = q('now', current.value)
  const pastEl = previous.value ? q('past', previous.value) : null
  if (leftIn.value) link(q('left', leftIn.value), pastEl, true)
  if (phase.value === 'back') { for (const c of all('now')) link(pastEl, c, c.dataset.id === current.value) }
  else if (pastEl) link(pastEl, nowEl, true, phase.value === 'fwd' ? 0 : 1)
  for (const c of all('next')) link(nowEl, c, c.dataset.id === hot.value || c.dataset.id === picked.value, phase.value === 'back' ? .3 : 1)
  if (picked.value) { const p = q('next', picked.value); for (const c of all('right')) link(p, c, false) }
  else if (hot.value) { const h = q('next', hot.value); for (const c of all('right')) link(h, c, false) }
  for (const [k, el] of wireEls) {
    if (seen.has(k)) continue
    wireEls.delete(k)
    el.style.opacity = '0'
    setTimeout(() => el.remove(), 340)
  }
}

let ro: ResizeObserver | undefined
onMounted(() => {
  nextTick(drawWires)
  ro = new ResizeObserver(() => { if (!animating.value) drawWires() })
  ro.observe(stage.value!)
  track.value?.addEventListener('scroll', () => { if (!animating.value) drawWires() }, true)
})
onUnmounted(() => ro?.disconnect())

const trackStyle = computed(() => ({
  width: `${5 * W + 4 * G}px`,
  transform: `translateX(${-STEP + shift.value}px)`,
  transition: noTransition.value ? 'none' : 'transform .42s cubic-bezier(.2,.8,.2,1)',
}))
</script>

<template>
  <div class="stage" ref="stage">
    <div class="track" ref="track" :class="{ notrans: noTransition }" :style="trackStyle" @transitionend="onShiftEnd" @mouseover="onOver" @mouseleave="setHot('')" @click="onClick">
      <svg ref="wires" class="wires"></svg>
      <div class="slot left" data-slot="left" :class="{ showing: phase === 'back' && shift !== 0 }">
        <Card v-if="leftIn" :key="leftIn" :id="leftIn" col="past" />
      </div>
      <div class="slot past" data-slot="past" :class="{ fading: phase === 'fwd' }">
        <Card v-if="previous" :key="previous" :id="previous" :col="phase === 'back' ? 'now' : 'past'" />
        <div v-else class="empty">başlangıç</div>
      </div>
      <div class="slot now" data-slot="now">
        <CandList v-if="phase === 'back'" :cands="backCands" :selected="backSel" />
        <Card v-else :key="current" :id="current" :col="phase === 'fwd' ? 'past' : 'now'" />
      </div>
      <div class="slot next" data-slot="next" :class="{ dim: phase === 'back' }">
        <CandList :cands="nexts" :hot="hot" :selected="picked" interactive />
      </div>
      <div class="slot right" data-slot="right">
        <template v-if="incoming">
          <CandList :cands="incoming" :selected="''" />
        </template>
        <template v-else>
          <Card v-for="c in horizon.slice(0, HORIZON_MAX)" :key="c.to + (c.via?.id ?? '')" :id="c.to" col="horizon" :cand="c" class="fade" />
          <div v-if="horizon.length > HORIZON_MAX" class="empty fade">+{{ horizon.length - HORIZON_MAX }} çıkış daha</div>
        </template>
      </div>
    </div>
  </div>
</template>

<style scoped>
.stage { flex: 1; position: relative; overflow: hidden; display: flex; align-items: center; padding-left: 40px; }
.track { position: relative; display: flex; align-items: center; gap: 80px; height: 100%; will-change: transform; }
.wires { position: absolute; inset: 0; width: 100%; height: 100%; pointer-events: none; }
.wires :deep(text) { font: 11px "Instrument Sans", system-ui, sans-serif; fill: var(--muted); paint-order: stroke; stroke: var(--bg); stroke-width: 5px; stroke-linejoin: round; }
.wires :deep(text.guard) { fill: var(--domain); }
.wires :deep(text.fail) { fill: var(--bad); }
.slot { width: 300px; flex: 0 0 300px; display: flex; flex-direction: column; gap: 14px; align-items: stretch; max-height: calc(100% - 40px); overflow: auto; padding: 4px; position: relative; z-index: 1; }
.slot.next, .slot.past, .slot.left { transition: opacity .4s; }
.slot.next.dim { opacity: .28; }
.slot.past.fading { opacity: 0; }
.slot.left { opacity: 0; } .slot.left.showing { opacity: 1; }
.track.notrans .slot { transition: none; }
.empty { color: var(--faint); font-size: 12px; text-align: center; padding: 20px 8px; border: 1px dashed var(--line); border-radius: 10px; }
</style>
