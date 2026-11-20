import { describe, expect, it } from 'vitest'

import { BINARY_NOT_RUN, parseVerb } from './verb.ts'

describe('parseVerb', () => {
  it('passes a good reply through', () => {
    const r = parseVerb(0, '{"ok":true,"slug":"p","schema":1}\n')
    expect(r).toEqual({ ok: true, slug: 'p', schema: 1 })
  })

  it('passes an ok:false reply through', () => {
    const r = parseVerb(1, '{"ok":false,"error":"run crook open p"}')
    expect(r).toEqual({ ok: false, error: 'run crook open p' })
  })

  it('reads the reply whatever the exit code says', () => {
    expect(parseVerb(127, '{"ok":true}')).toEqual({ ok: true })
  })

  it('reports empty stdout as a bad reply when the binary ran', () => {
    expect(parseVerb(0, '')).toEqual({ ok: false, error: 'crook: bad reply' })
    expect(parseVerb(1, '  \n')).toEqual({ ok: false, error: 'crook: bad reply' })
  })

  it('reports non-JSON stdout as a bad reply', () => {
    expect(parseVerb(1, 'Segmentation fault')).toEqual({ ok: false, error: 'crook: bad reply' })
  })

  it('reports JSON that is not an envelope as a bad reply', () => {
    for (const out of ['[1]', '"x"', 'null', '42', '{}', '{"ok":"yes"}']) {
      expect(parseVerb(0, out), out).toEqual({ ok: false, error: 'crook: bad reply' })
    }
  })

  it('gives ok:false replies an error text', () => {
    expect(parseVerb(1, '{"ok":false}')).toEqual({ ok: false, error: 'crook: bad reply' })
  })

  it('reports a binary that did not run', () => {
    const missing = { ok: false, error: 'crook: binary not found' }
    expect(parseVerb(BINARY_NOT_RUN, '')).toEqual(missing)
    expect(parseVerb(126, '')).toEqual(missing)
    expect(parseVerb(127, 'sh: crook: not found')).toEqual(missing)
    expect(parseVerb(-1, '')).toEqual(missing)
  })
})
