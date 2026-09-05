// Yürüyüş durumu: geçmiş, katman toggle'ı, rol filtresi, simüle değişkenler, replay.
// Hover bilerek burada DEĞİL: Walk bileşeninde yerel (sahne yeniden kurulmasın).

import { computed, reactive, ref, shallowRef } from 'vue'
import { Graph, type Candidate, type Node } from './graph'

export const graph = shallowRef<Graph | null>(null)

export const state = reactive({
  hist: [] as string[],
  hiddenLayers: new Set<string>(),
  role: '' as string,
  view: 'walk' as 'walk' | 'map',
  /** simüle edilen değişken değerleri */
  vars: {} as Record<string, string>,
  flash: '' as string,
  replay: null as null | { path: string[]; idx: number; timer: number },
  /** kalabalık aday sütununda açık gruplar ve süzgeç */
  openGroups: new Set<string>(),
  filter: '',
})

export const current = computed(() => state.hist[state.hist.length - 1] ?? '')
export const previous = computed(() => state.hist[state.hist.length - 2] ?? '')

export function candidates(id: string): Candidate[] {
  const g = graph.value; if (!g) return []
  return g.nexts(id, { hideLayers: state.hiddenLayers, role: state.role || undefined })
}

export function resetVars() {
  const g = graph.value; if (!g) return
  state.vars = {}
  for (const v of g.vars.values()) state.vars[v.id] = v.source ? (v.source === 'session' ? '—' : '…') : '—'
  if (state.role) state.vars['role'] = state.role
}

function applySets(n: Node | undefined) {
  if (!n) return
  for (const s of n.sets) {
    const cur = state.vars[s.var]
    let next = s.value
    if (/^[+-]\d+$/.test(s.value)) next = String((parseInt(cur) || 0) + parseInt(s.value))
    state.vars[s.var] = next
    state.flash = s.var
    setTimeout(() => { if (state.flash === s.var) state.flash = '' }, 1100)
  }
}

export function go(id: string) {
  state.hist.push(id)
  state.openGroups = new Set(); state.filter = ''
  applySets(graph.value?.get(id))
}

export function back() {
  if (state.hist.length > 1) state.hist.pop()
}

export function rewindTo(id: string) {
  const i = state.hist.lastIndexOf(id)
  if (i >= 0) state.hist = state.hist.slice(0, i + 1)
}

/** Arama/harita atlaması: root'tan en kısa yol geçmiş olur. */
export function jumpTo(id: string) {
  const g = graph.value; if (!g) return
  stopReplay()
  resetVars()
  const p = g.pathFromRoots(id)
  state.hist = p ?? [id]
  state.openGroups = new Set(); state.filter = ''
  for (const h of state.hist) applySets(g.get(h))
  state.view = 'walk'
}

export function startAt(id: string) {
  stopReplay(); resetVars()
  state.hist = [id]; state.openGroups = new Set(); state.filter = ''
  applySets(graph.value?.get(id))
  state.view = 'walk'
}

/** Replay: yolu adım adım oynatır. Walk bileşeni `replayHover` ile ufku önceden gösterir. */
export const replayHover = ref('')

export function startReplay(path: string[], stepMs = 1500) {
  const g = graph.value; if (!g || path.length < 2) return
  stopReplay(); resetVars()
  state.hist = [path[0]]; applySets(g.get(path[0]))
  state.view = 'walk'
  const timer = window.setInterval(() => {
    const r = state.replay; if (!r) return
    r.idx++
    if (r.idx >= r.path.length) { stopReplay(); return }
    const target = r.path[r.idx]
    // katlı grupta hedef görünür olsun
    state.openGroups.add(target.split('.').slice(0, 2).join('.'))
    replayHover.value = target
    window.setTimeout(() => { if (state.replay === r) { replayHover.value = ''; go(target) } }, Math.min(650, stepMs * 0.45))
  }, stepMs)
  state.replay = { path, idx: 0, timer }
}

export function stopReplay() {
  if (state.replay) { clearInterval(state.replay.timer); state.replay = null; replayHover.value = '' }
}

export function toggleLayer(l: string) {
  const s = new Set(state.hiddenLayers)
  if (s.has(l)) s.delete(l); else s.add(l)
  state.hiddenLayers = s
}
