import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// 개발 중에는 Vite(5173)가 /api 요청을 Rocket 서버(8000)로 넘긴다.
// 빌드 결과(dist)는 Rocket이 직접 서빙한다.
const backend = process.env.CASHBOOK_API ?? 'http://127.0.0.1:8000'

export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      '/api': { target: backend, changeOrigin: false },
    },
  },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 900,
  },
})
