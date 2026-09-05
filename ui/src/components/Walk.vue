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
const pastFrom = ref<number | undefined>()   // commit sonrası geçmiş kartın başlangıç yüksekliği
const nowFrom = ref<number | undefined>()    // commit sonrası şimdi kartın başlangıç yüksekliği
const nextFrom = ref<Record<string, number> | undefined>()
const backSel = ref('')                       // geri dönüşte listeye inen eski "şimdi"

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

function forward(id: string) {
  if (animating.value) return
  animating.value = true
  hot.value = ''
  picked.value = id
  incoming.value = candidates(id)
  state.openGroups = new Set(); state.filter = ''
  // faz 1: kardeşler kapanır, seçilen ortaya kayıp "şimdi" boyuna gelir; teller takip eder
  nextTick(() => drawLoop(400, () => { shift.value = -STEP }))
}

function backward() {
  if (animating.value || state.hist.length < 2) return
  animating.value = true
  hot.value = ''
  leftIn.value = prevPrev.value
  nextTick(() => {
    drawWires()
    requestAnimationFrame(() => requestAnimationFrame(() => { shift.value = STEP }))
  })
}

function onShiftEnd(e: TransitionEvent) {
  if (e.target !== track.value || e.propertyName !== 'transform' || shift.value === 0) return
  const dir = shift.value
  const tr = track.value!
  const h = (sel: string) => tr.querySelector<HTMLElement>(sel)?.offsetHeight
  const oldNow = current.value
  const nowH = h('.slot.now .card'), pastH = h('.slot.past .card')
  const pickedH = picked.value ? h(`.slot.next .card[data-id="${CSS.escape(picked.value)}"]`) : undefined
  // bant sıfırlanırken transition kapalı: görüntü değişmez, sadece state ilerler
  noTransition.value = true
  if (dir < 0) {
    go(picked.value)
    pastFrom.value = nowH; nowFrom.value = pickedH; nextFrom.value = undefined; backSel.value = ''
  } else {
    back()
    nowFrom.value = pastH; pastFrom.value = undefined
    // eski "şimdi" listede seçili (now stili) başlar, kardeşler kapalı; bir kare sonra açılır
    nextFrom.value = { [oldNow]: nowH ?? 0 }; backSel.value = oldNow
  }
  picked.value = ''; incoming.value = null; leftIn.value = ''
  shift.value = 0
  nextTick(() => {
    drawWires()
    requestAnimationFrame(() => {
      noTransition.value = false
      if (backSel.value) backSel.value = ''
      // faz 3: geçmiş küçülür / şimdi büyür / kardeşler açılır; teller takip eder
      drawLoop(420, () => { pastFrom.value = undefined; nowFrom.value = undefined; nextFrom.value = undefined; animating.value = false })
    })
  })
}

// hist dışarıdan değişirse (arama, root seç, geri tuşu) anında yeniden çiz
watch(() => state.hist.slice(), () => { if (!animating.value) { hot.value = ''; nextTick(drawWires) } })
watch([() => state.hiddenLayers, () => state.role, () => state.filter, () => state.openGroups, () => ({ ...state.vars })], () => { if (!animating.value) nextTick(drawWires) })

// ---- teller (bant içinde, bantla kayar) ----
function drawWires() {
  const svg = wires.value, tr = track.value
  if (!svg || !tr) return
  const tb = tr.getBoundingClientRect()
  const P = (el: Element) => { const r = el.getBoundingClientRect(); return { l: r.left - tb.left, r: r.right - tb.left, y: r.top - tb.top + r.height / 2 } }
  const q = (slot: string, id: string) => tr.querySelector<HTMLElement>(`.slot.${slot} .card[data-id="${CSS.escape(id)}"]`)
  const all = (slot: string) => [...tr.querySelectorAll<HTMLElement>(`.slot.${slot} .card`)]
  const parts: string[] = []
  const esc = (t: string) => t.replace(/&/g, '&amp;').replace(/</g, '&lt;')
  const link = (a: Element | null, b: HTMLElement | null, hotp: boolean) => {
    if (!a || !b || b.classList.contains('dropped')) return
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
  const pastEl = previous.value ? q('past', previous.value) : null
  if (leftIn.value) link(q('left', leftIn.value), pastEl, true)
  if (pastEl) link(pastEl, nowEl, true)
  for (const c of all('next')) link(nowEl, c, c.dataset.id === hot.value || c.dataset.id === picked.value)
  if (picked.value) { const p = q('next', picked.value); for (const c of all('right')) link(p, c, false) }
  else if (hot.value) { const h = q('next', hot.value); for (const c of all('right')) link(h, c, false) }
  svg.innerHTML = parts.join('')
  for (const t of svg.querySelectorAll<SVGTextElement>('text[data-max]')) {
    const max = Number(t.dataset.max)
    const node = [...t.childNodes].find((n) => n.nodeType === 3)
    if (!node) continue
    let text = node.textContent ?? ''
    let guard = 0
    while (t.getComputedTextLength() > max && text.length > 3 && guard++ < 60) { text = text.slice(0, -2).trimEnd() + '…'; node.textContent = text }
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
    <div class="track" ref="track" :style="trackStyle" @transitionend="onShiftEnd" @mouseover="onOver" @mouseleave="setHot('')" @click="onClick">
      <svg ref="wires" class="wires"></svg>
      <div class="slot left" data-slot="left">
        <Card v-if="leftIn" :id="leftIn" col="past" />
      </div>
      <div class="slot past" data-slot="past">
        <Card v-if="previous" :id="previous" col="past" :from-height="pastFrom" />
        <div v-else class="empty">başlangıç</div>
      </div>
      <div class="slot now" data-slot="now">
        <Card :id="current" col="now" :from-height="nowFrom" />
      </div>
      <div class="slot next" data-slot="next">
        <CandList :cands="nexts" :hot="hot" :selected="picked || backSel" :from-heights="nextFrom" interactive />
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
.empty { color: var(--faint); font-size: 12px; text-align: center; padding: 20px 8px; border: 1px dashed var(--line); border-radius: 10px; }
</style>
