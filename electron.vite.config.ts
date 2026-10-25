import { resolve } from 'node:path'
import { defineConfig, externalizeDepsPlugin } from 'electron-vite'
import react from '@vitejs/plugin-react'

const r = (p: string) => resolve(process.cwd(), p)

export default defineConfig({
  main: {
    plugins: [externalizeDepsPlugin()],
    build: {
      outDir: 'out/main',
      rollupOptions: {
        input: { index: r('src/main/index.ts') }
      }
    }
  },
  preload: {
    plugins: [externalizeDepsPlugin()],
    build: {
      outDir: 'out/preload',
      rollupOptions: {
        input: { index: r('src/preload/index.ts') }
      }
    }
  },
  renderer: {
    root: r('src/renderer'),
    plugins: [react()],
    resolve: {
      alias: {
        '@shared': r('src/shared'),
        '@renderer': r('src/renderer/src')
      }
    },
    build: {
      outDir: r('out/renderer'),
      rollupOptions: {
        input: {
          index: r('src/renderer/index.html'),
          selector: r('src/renderer/selector.html'),
          editor: r('src/renderer/editor.html'),
          pin: r('src/renderer/pin.html')
        }
      }
    }
  }
})
