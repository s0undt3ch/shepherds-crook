export type VerbResult = { ok: boolean; error?: string; [k: string]: unknown }

/** Everything `core/` may touch; the adapter builds it from `$`. */
export interface Ports {
  run(verb: string, input: object, timeoutMs?: number): Promise<VerbResult>
  stat(path: string): Promise<{ mtimeMs: number } | null>
  submit(text: string): Promise<void>
  status(text: string | undefined): void
  every(ms: number, fn: () => void): void
  now(): number
  env(name: string): string | undefined
}
