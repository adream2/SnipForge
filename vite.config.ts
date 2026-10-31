import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import { resolve } from 'node:path'

const r = (p: string): string => resolve(process.cwd(), p)

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  resolve: {
    alias: {
      '@': r('src')
    }
  },
  server: {
    port: 5183,
    strictPort: true,
    host: '127.0.0.1'
  },
  build: {
    target: 'chrome110',
    outDir: 'dist',
    emptyOutDir: true,
    rollupOptions: {
      input: {
        index: r('index.html'),
        selector: r('selector.html'),
        editor: r('editor.html'),
        pin: r('pin.html'),
        panel: r('panel.html'),
        ocr: r('ocr.html'),
        record: r('record.html'),
        overlay: r('overlay.html')
      }
    }
  }
})
