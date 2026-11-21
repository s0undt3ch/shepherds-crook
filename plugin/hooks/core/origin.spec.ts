import { describe, expect, it } from 'vitest'

import { createOrigin } from './origin.ts'

describe('userTurn', () => {
  it('is false before any turn', () => {
    expect(createOrigin().userTurn()).toBe(false)
  })

  it('is true after a composer prompt starts a turn', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'hi')
    o.turnStart('hi')
    expect(o.userTurn()).toBe(true)
  })

  it('is true for the bridge', () => {
    const o = createOrigin()
    o.noteOrigin('bridge', false, 'hi')
    o.turnStart('hi')
    expect(o.userTurn()).toBe(true)
  })

  it.each(['scheduled-trigger', 'task-notification', 'plugin', 'peer', 'sdk', 'unclassified'])(
    'is false for %s',
    kind => {
      const o = createOrigin()
      o.noteOrigin('composer', false, 'before')
      o.turnStart('before')
      o.noteOrigin(kind, false, 'x')
      o.turnStart('x')
      expect(o.userTurn()).toBe(false)
    },
  )

  it('is false after the mod submits its own prompt', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'hi')
    o.turnStart('hi')
    o.noteOwnSubmit('wake')
    o.turnStart('wake')
    expect(o.userTurn()).toBe(false)
  })

  it('does not change until a turn starts', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'a')
    o.turnStart('a')
    o.noteOrigin('scheduled-trigger', false, 'b')
    expect(o.userTurn()).toBe(true)
    o.turnStart('b')
    expect(o.userTurn()).toBe(false)
  })

  it('orders a wake-up before a composer prompt typed ahead of its turn', () => {
    const o = createOrigin()
    o.noteOwnSubmit('crook: wake')
    o.noteOrigin('composer', false, 'typed')
    o.turnStart('crook: wake')
    expect(o.userTurn()).toBe(false)
    o.turnStart('typed')
    expect(o.userTurn()).toBe(true)
  })

  it('keeps the order when turn.start carries no matching text', () => {
    const o = createOrigin()
    o.noteOwnSubmit()
    o.noteOrigin('composer', false)
    o.turnStart('rewritten 1')
    expect(o.userTurn()).toBe(false)
    o.turnStart('rewritten 2')
    expect(o.userTurn()).toBe(true)
  })

  it('does not queue a slash command, which starts no turn', () => {
    const o = createOrigin()
    o.noteOrigin('composer', true, '/crook-adopt')
    o.noteOrigin('scheduled-trigger', false, 'tick')
    o.turnStart('tick')
    expect(o.userTurn()).toBe(false)
  })

  it('is false on an empty queue and on a continuation, and keeps the queue', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'a')
    o.turnStart('a')
    o.turnStart('anything')
    expect(o.userTurn()).toBe(false)
    o.noteOrigin('composer', false, 'b')
    o.turnStart('')
    expect(o.userTurn()).toBe(false)
    o.turnStart('b')
    expect(o.userTurn()).toBe(true)
  })

  it('resyncs on text, dropping entries whose turn never started', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'delivered into a running turn')
    o.noteOrigin('scheduled-trigger', false, 'tick')
    o.turnStart('tick')
    expect(o.userTurn()).toBe(false)
    o.noteOrigin('composer', false, 'next')
    o.turnStart('next')
    expect(o.userTurn()).toBe(true)
  })

  it('a stale user entry never makes a later machine turn a user turn', () => {
    const o = createOrigin()
    o.noteOrigin('composer', false, 'stale')
    o.noteOrigin('scheduled-trigger', false, 'tick')
    o.turnStart('tick')
    expect(o.userTurn()).toBe(false)
  })

  it('caps the queue', () => {
    const o = createOrigin()
    for (let i = 0; i < 200; i++) o.noteOrigin('scheduled-trigger', false, `m${i}`)
    o.noteOrigin('composer', false, 'last')
    o.turnStart('last')
    expect(o.userTurn()).toBe(true)
    expect(o.pending()).toBe(0)
  })
})
