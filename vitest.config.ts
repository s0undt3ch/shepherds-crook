import { defineConfig } from 'vitest/config'

export default defineConfig({
  test: {
    include: ['plugin/hooks/core/**/*.spec.ts'],
    coverage: {
      provider: 'v8',
      include: ['plugin/hooks/core/**'],
      exclude: ['**/*.spec.ts'],
      reporter: ['text', 'cobertura'],
      reportsDirectory: 'coverage',
    },
  },
})
