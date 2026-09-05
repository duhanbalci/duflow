import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import { viteSingleFile } from 'vite-plugin-singlefile'

// Tek dosya çıktı: crates/duflow-cli/ui-dist/index.html, CLI include_str ile gömer.
export default defineConfig({
  plugins: [vue(), viteSingleFile()],
  build: { copyPublicDir: false, outDir: '../crates/duflow-cli/ui-dist', emptyOutDir: true, cssCodeSplit: false, assetsInlineLimit: 100000000 },
})
