import { readFileSync } from 'node:fs'
import { describe, expect, it } from 'vitest'
import {
  ATTACHMENT_ERROR_CODES,
  FINALIZE_ERROR_CODES,
  MAX_ATTACHMENT_BYTES,
  MAX_FRAME_BYTES,
  MIME_BY_EXTENSION,
  UPLOAD_CHUNK_BYTES,
  UPLOAD_WINDOW,
  attachmentChunkMessages,
  base64,
  beginAttachmentMessage,
  cancelAttachmentMessage,
  chunkFrameBytes,
  chunkPlan,
  contentTypeFor,
  describeUpload,
  finalizeAttachmentMessage,
  isUploadFailure,
  slicesStaged,
  uploadIsInFlight,
  uploadWindowSlices,
  validateAttachmentMessage,
  type UploadStatus,
} from './attachments'

const ENGINE_SOURCE = readFileSync(new URL('../../../crates/codegen/chaos-engine/src/lib.rs', import.meta.url), 'utf8')
const WEB_SOURCE = readFileSync(new URL('../../../crates/codegen/xai-grok-web/src/lib.rs', import.meta.url), 'utf8')

function decode(chunk: string): Uint8Array {
  const binary = atob(chunk)
  const bytes = new Uint8Array(binary.length)
  for (let index = 0; index < binary.length; index += 1) bytes[index] = binary.charCodeAt(index)
  return bytes
}

function patternBytes(length: number): Uint8Array {
  const bytes = new Uint8Array(length)
  for (let index = 0; index < length; index += 1) bytes[index] = (index * 251 + 7) % 256
  return bytes
}

describe('attachment upload framing', () => {
  it('slices a file into whole slices that cover every byte exactly once', () => {
    expect(chunkPlan(0)).toEqual([])
    expect(chunkPlan(1)).toEqual([{ offset: 0, length: 1 }])
    expect(chunkPlan(UPLOAD_CHUNK_BYTES)).toEqual([{ offset: 0, length: UPLOAD_CHUNK_BYTES }])

    const size = 100 * 1024
    const full = Math.floor(size / UPLOAD_CHUNK_BYTES)
    const plan = chunkPlan(size)
    expect(plan.map((slice) => slice.length)).toEqual([
      ...Array.from({ length: full }, () => UPLOAD_CHUNK_BYTES),
      size - full * UPLOAD_CHUNK_BYTES,
    ])
    expect(plan.reduce((total, slice) => total + slice.length, 0)).toBe(size)
    expect(plan[1].offset).toBe(UPLOAD_CHUNK_BYTES)
    expect(plan[plan.length - 1].offset + plan[plan.length - 1].length).toBe(size)

    // The biggest file the policy admits is 218 frames, not thousands: the slice
    // size is what keeps a 10 MiB attachment off a slow handshake.
    expect(chunkPlan(MAX_ATTACHMENT_BYTES)).toHaveLength(218)
    for (const slice of chunkPlan(MAX_ATTACHMENT_BYTES)) {
      expect(slice.length).toBeGreaterThan(0)
      expect(slice.length).toBeLessThanOrEqual(UPLOAD_CHUNK_BYTES)
      expect(chunkFrameBytes(slice.length)).toBeLessThan(MAX_FRAME_BYTES)
    }
  })

  it('keeps one chunk inside the host frame limit and proves the limit is what binds', () => {
    expect(chunkFrameBytes(UPLOAD_CHUNK_BYTES)).toBeLessThan(MAX_FRAME_BYTES)
    // Not merely inside: the next kilobyte is already outside, so the slice is the
    // largest whole-kibibyte transfer the wire allows and the constant is pinned
    // from both sides. Raising it further would make the host answer
    // `message_too_large` and the transfer would stall at the first slice.
    expect(chunkFrameBytes(UPLOAD_CHUNK_BYTES + 1024)).toBeGreaterThan(MAX_FRAME_BYTES)
    // The frame is base64 and UUID text, so its JS string length is its byte
    // length; a non-ASCII frame would make this comparison lie to the test above.
    expect(/^[\x00-\x7F]*$/.test(JSON.stringify({ chunk: base64(patternBytes(UPLOAD_CHUNK_BYTES)) }))).toBe(true)
  })

  it('sends base64 slices that decode back to the original bytes in order', () => {
    const bytes = patternBytes(100 * 1024 + 5)
    const messages = attachmentChunkMessages('upload-1', bytes, () => crypto.randomUUID())
    const rejoined = new Uint8Array(bytes.length)
    let written = 0
    for (const message of messages) {
      expect(message.type).toBe('attachment_chunk')
      if (message.type !== 'attachment_chunk') throw new Error('unreachable')
      const slice = decode(message.chunk)
      expect(slice.length).toBeLessThanOrEqual(UPLOAD_CHUNK_BYTES)
      rejoined.set(slice, written)
      written += slice.length
    }
    expect(written).toBe(bytes.length)
    expect(rejoined).toEqual(bytes)
    expect(new Set(messages.map((message) => message.client_msg_id)).size).toBe(messages.length)
    expect(messages.every((message) => message.type === 'attachment_chunk' && message.upload_id === 'upload-1')).toBe(true)
  })

  it('counts only whole slices as staged, so the window waits for real bytes', () => {
    const size = 10 * UPLOAD_CHUNK_BYTES + 5
    const total = chunkPlan(size).length

    expect(slicesStaged(size, 0)).toBe(0)
    // A partial slice is not staged: the host appends bytes as they arrive and
    // only a whole slice can be relied on, so the window stays shut.
    expect(slicesStaged(size, UPLOAD_CHUNK_BYTES - 1)).toBe(0)
    expect(slicesStaged(size, UPLOAD_CHUNK_BYTES)).toBe(1)
    expect(slicesStaged(size, 3 * UPLOAD_CHUNK_BYTES + 1)).toBe(3)
    expect(slicesStaged(size, size - 5)).toBe(total - 1)
    expect(slicesStaged(size, size)).toBe(total)
    // Past the last byte the host can report, the count does not run off the end.
    expect(slicesStaged(size, size + UPLOAD_CHUNK_BYTES)).toBe(total)
    expect(slicesStaged(0, 0)).toBe(0)
  })

  it('keeps at most a window of slices unacknowledged and never sends past the end', () => {
    const size = 10 * UPLOAD_CHUNK_BYTES + 5
    const total = chunkPlan(size).length

    // Nothing acknowledged yet: a full window goes out, which is what keeps a
    // short file to a single round trip instead of one slice per round trip.
    expect(uploadWindowSlices(size, 0, 0)).toBe(UPLOAD_WINDOW)
    expect(uploadWindowSlices(2 * UPLOAD_CHUNK_BYTES, 0, 0)).toBe(2)
    expect(uploadWindowSlices(UPLOAD_CHUNK_BYTES, 0, 0)).toBe(1)
    expect(uploadWindowSlices(0, 0, 0)).toBe(0)
    // Everything on the wire and none of it acknowledged: the client waits rather
    // than burying a host that is slow or has stopped reading.
    expect(uploadWindowSlices(size, 0, UPLOAD_WINDOW)).toBe(0)
    expect(uploadWindowSlices(size, 0, total)).toBe(0)
    // One slice acknowledged lets exactly one more go, so throughput is the window.
    expect(uploadWindowSlices(size, UPLOAD_CHUNK_BYTES, UPLOAD_WINDOW)).toBe(1)
    // The tail: fewer slices remain than the window is willing to send.
    expect(uploadWindowSlices(size, size - 5, total - 1)).toBe(1)
    expect(uploadWindowSlices(size, size, total)).toBe(0)
  })

  it('drives a whole upload through the window without exceeding it', () => {
    const size = 37 * UPLOAD_CHUNK_BYTES + 11
    const plan = chunkPlan(size)
    const total = plan.length
    const ends = plan.map((slice) => slice.offset + slice.length)

    // The pump `main.tsx` runs, replayed against a host that stages slices in
    // arrival order and acknowledges every byte it holds: send what the window
    // allows, take back the acknowledgement, repeat. The assertions are the
    // invariants the host's per-connection queue depends on.
    let sent = 0
    let received = 0
    let peakInFlight = 0
    for (let round = 0; received < size; round += 1) {
      expect(round).toBeLessThan(total + UPLOAD_WINDOW + 2)
      const toSend = uploadWindowSlices(size, received, sent)
      expect(toSend).toBeGreaterThanOrEqual(0)
      expect(sent + toSend).toBeLessThanOrEqual(total)
      sent += toSend
      peakInFlight = Math.max(peakInFlight, sent - slicesStaged(size, received))
      expect(peakInFlight).toBeLessThanOrEqual(UPLOAD_WINDOW)
      // The host acks up to the end of the next slice it has taken.
      const staged = slicesStaged(size, received)
      received = ends[Math.min(staged, total - 1)]
    }

    expect(sent).toBe(total)
    expect(uploadWindowSlices(size, received, sent)).toBe(0)
    // The window is what makes the biggest admitted file a bounded burst: without
    // it the browser would put all 218 slices on the socket at once and the host
    // would refuse the frames past its in-flight limit, blaming the user's file.
    expect(chunkPlan(MAX_ATTACHMENT_BYTES).length).toBeGreaterThan(UPLOAD_WINDOW * 10)
    expect(peakInFlight).toBe(UPLOAD_WINDOW)
    // The host counts frames it has not answered yet (`MAX_IN_FLIGHT_PER_CONNECTION`
    // in `crates/codegen/xai-grok-web/src/lib.rs`) and answers the frames past that
    // with `too_many_in_flight`, so a window at or past its bound would make every
    // legal upload fail on the host's own accounting. Read from the Rust source so
    // the two numbers cannot drift apart unnoticed.
    const inFlightBound = Number(/const MAX_IN_FLIGHT_PER_CONNECTION: usize = (\d+);/.exec(WEB_SOURCE)?.[1])
    expect(Number.isInteger(inFlightBound)).toBe(true)
    expect(UPLOAD_WINDOW).toBeLessThan(inFlightBound)
  })

  it('round-trips a single byte and an unaligned tail through base64', () => {

    for (const length of [1, 2, 3, 4, UPLOAD_CHUNK_BYTES - 1, UPLOAD_CHUNK_BYTES + 1]) {
      const bytes = patternBytes(length)
      expect(decode(base64(bytes))).toEqual(bytes)
    }
  })

  it('asks the host to validate before it asks to store anything', () => {
    const source = { filename: 'note.txt', contentType: 'text/plain', bytes: patternBytes(64) }
    expect(validateAttachmentMessage('c1', source)).toEqual({
      type: 'validate_attachment', client_msg_id: 'c1', filename: 'note.txt', byte_len: 64, content_type: 'text/plain',
    })
    expect(beginAttachmentMessage('c2', 'session-1', source)).toEqual({
      type: 'begin_attachment', client_msg_id: 'c2', session_id: 'session-1', filename: 'note.txt', content_type: 'text/plain', byte_len: 64,
    })
    expect(finalizeAttachmentMessage('c3', 'upload-1', 'docs/note.txt')).toEqual({
      type: 'finalize_attachment', client_msg_id: 'c3', upload_id: 'upload-1', relative_path: 'docs/note.txt',
    })
    expect(cancelAttachmentMessage('c4', 'upload-1')).toEqual({ type: 'cancel_attachment', client_msg_id: 'c4', upload_id: 'upload-1' })
  })

  it('fills the MIME type the browser left empty from the engine allowlist', () => {
    expect(contentTypeFor('notes.md', '')).toBe('text/markdown')
    expect(contentTypeFor('PHOTO.JPG', '')).toBe('image/jpeg')
    expect(contentTypeFor('notes.md', 'text/plain')).toBe('text/plain')
    expect(contentTypeFor('archive.zip', '')).toBe('')
  })

  it('agrees with the engine about which extensions and sizes may be staged', () => {
    const from = ENGINE_SOURCE.indexOf('fn validate_name_type_size')
    const to = ENGINE_SOURCE.indexOf('pub fn new(root: impl AsRef<Path>, max_bytes: u64)')
    expect(from, 'the engine policy function moved').toBeGreaterThan(-1)
    expect(to, 'the engine policy function moved').toBeGreaterThan(from)
    const block = ENGINE_SOURCE.slice(from, to)
    const rust = [...block.matchAll(/\("(\.[a-z]+)", "([a-z/+.]+)"\)/g)]
      .map(([, extension, mime]) => [extension, mime])
    expect(rust.length).toBeGreaterThan(0)
    expect(Object.entries(MIME_BY_EXTENSION)).toEqual(rust)
    const bound = /byte_len == 0 \|\| byte_len > (\d+) \* (\d+) \* (\d+)/.exec(block)
    expect(bound).not.toBeNull()
    expect(MAX_ATTACHMENT_BYTES).toBe(Number(bound![1]) * Number(bound![2]) * Number(bound![3]))
  })

  it('names the upload flow error codes the host really emits', () => {
    // `message_too_large` and `safe_web_mode_blocked` come from the Web host, not
    // the engine: the frame check and the allowlist run before a message is dispatched.
    const producers = [ENGINE_SOURCE, WEB_SOURCE]
    for (const code of [...ATTACHMENT_ERROR_CODES, ...FINALIZE_ERROR_CODES]) {
      expect(producers.some((source) => source.includes(`"${code}"`)), `${code} is produced nowhere`).toBe(true)
    }
  })

  it('blames an upload only for errors that can end it', () => {
    for (const status of ['validating', 'beginning', 'uploading', 'awaiting_approval'] satisfies UploadStatus[]) {
      expect(isUploadFailure('attachment_rejected', status)).toBe(true)
      expect(isUploadFailure('message_too_large', status)).toBe(true)
    }
    // A path refusal from the file browser is not the transfer's fault until the
    // bytes are staged and finalize has been sent.
    expect(isUploadFailure('path_escape', 'validating')).toBe(false)
    expect(isUploadFailure('path_escape', 'uploading')).toBe(true)
    expect(isUploadFailure('workspace_unavailable', 'awaiting_approval')).toBe(true)
    expect(isUploadFailure('attachment_quota_exceeded', 'done')).toBe(false)
    expect(isUploadFailure('attachment_rejected', 'cancelled')).toBe(false)
    expect(isUploadFailure('attachment_rejected', undefined)).toBe(false)
    expect(isUploadFailure('git_failed', 'uploading')).toBe(false)
  })

  it('keeps the upload controls locked only while the host still has work to do', () => {
    // 上传附件 is disabled and 取消上传 is offered on this predicate, so a status the
    // host has finished with has to release both -- otherwise a refused upload
    // leaves the form stuck on a transfer nobody is running.
    for (const status of ['validating', 'beginning', 'uploading', 'awaiting_approval'] satisfies UploadStatus[]) {
      expect(uploadIsInFlight({ status })).toBe(true)
    }
    for (const status of ['done', 'failed', 'cancelled'] satisfies UploadStatus[]) {
      expect(uploadIsInFlight({ status })).toBe(false)
    }
    expect(uploadIsInFlight(undefined)).toBe(false)
  })

  it('describes every state the host can leave an upload in', () => {
    const statuses: UploadStatus[] = ['validating', 'beginning', 'uploading', 'awaiting_approval', 'done', 'failed', 'cancelled']
    const lines = statuses.map((status) => describeUpload({ filename: 'note.txt', byteLen: 64, sentBytes: 64, status, path: 'docs/note.txt', bytes: 64 }))
    expect(new Set(lines).size).toBe(statuses.length)
    expect(lines.every((line) => line.length > 0)).toBe(true)
    expect(lines[statuses.indexOf('uploading')]).toContain('64 / 64 字节')
  })
})
