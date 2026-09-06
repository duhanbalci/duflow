<script setup lang="ts">
import { onMounted, ref } from 'vue'
import { Graph, loadData } from './graph'
import { graph, state, resetVars, startAt, back, stopReplay } from './walk'
import TopBar from './components/TopBar.vue'
import Walk from './components/Walk.vue'
import MapView from './components/MapView.vue'
import SidePanel from './components/SidePanel.vue'

const error = ref('')

onMounted(async () => {
  try {
    const data = await loadData()
    graph.value = new Graph(data)
    resetVars()
    const start = data.roots[0] ?? data.nodes[0]?.id
    if (start) startAt(start)
    // ?diff=… ile açıldıysa overlay zaten veride; ?at=<id> ile başlangıç
    const at = new URLSearchParams(location.search).get('at')
    if (at && graph.value.get(at)) startAt(at)
  } catch (e) {
    error.value = String(e)
  }
  window.addEventListener('keydown', (e) => {
    const tag = (document.activeElement as HTMLElement | null)?.tagName
    if (tag === 'INPUT' || tag === 'TEXTAREA') return
    if (e.key === 'ArrowLeft') { stopReplay(); back() }
    if (e.key === 'm') state.view = state.view === 'map' ? 'walk' : 'map'
  })
})
</script>

<template>
  <div class="app" v-if="graph">
    <TopBar />
    <div class="main">
      <Walk v-if="state.view === 'walk'" />
      <MapView v-else />
      <SidePanel />
    </div>
  </div>
  <div v-else class="loading">
    <span v-if="error" class="err mono">{{ error }}</span>
    <span v-else>loading…</span>
  </div>
</template>

<style>
.app { height: 100%; display: flex; flex-direction: column; }
.main { flex: 1; display: flex; min-height: 0; }
.loading { height: 100%; display: grid; place-items: center; color: var(--muted); }
.loading .err { color: var(--bad); font-size: 12px; padding: 1rem; }
</style>
