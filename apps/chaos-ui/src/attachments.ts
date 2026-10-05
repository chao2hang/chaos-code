import type { ClientMessage } from './generated/protocol'

/**
 * The Web host refuses any WebSocket text frame longer than this, whole JSON
 * included (`MAX_REQUEST_BYTES` in `crates/codegen/xai-grok-web/src/lib.rs`),
 * and answers `message_too_large`. A base64 slice is therefore the binding
 * constraint on how much of a file may go out in one message.
 */
export const MAX_FRAME_BYTES = 64 * 1024

/**
 * The largest slice whose frame still fits, in 1 KiB steps: 47 KiB of raw bytes
 * base64 to 64,172 characters and the JSON around them adds 144, so the frame is
 * 64,316 bytes against the 65,536 limit, while the next kilobyte would need
 * 65,680. `attachments.test.ts` asserts both sides, so the constant cannot be
 * nudged up in silence and is not left smaller than the wire allows either.
 */
export const UPLOAD_CHUNK_BYTES = 47 * 1024

/**
 * Largest file the engine will stage. Reading a bigger file into memory first
 * would only end in a refusal, so the picker stops it before the bytes are read;
 * `attachments.test.ts` compares this with the bound in
 * `AttachmentStager::validate_name_type_size`.
 */
export const MAX_ATTACHMENT_BYTES = 10 * 1024 * 1024

/**
 * Extension → MIME pairs the engine accepts (`AttachmentStager::validate_name_type_size`).
 * A browser reports `''` for several of these (`text/markdown` is not in every
 * platform's MIME database), so the declared type cannot be trusted to arrive
 * non-empty; `contentTypeFor` fills the gap. `attachments.test.ts` compares this
 * table against the Rust allowlist so the two cannot drift apart unnoticed.
 */
export const MIME_BY_EXTENSION: Record<string, string> = {
  '.txt': 'text/plain',
  '.md': 'text/markdown',
  '.json': 'application/json',
  '.png': 'image/png',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.pdf': 'application/pdf',
}

/**
 * Codes the attachment flow answers with, named in `crates/codegen/chaos-engine/src/lib.rs`:
 * a refused policy check, a chunk that is not base64 or arrives for an upload
 * that is gone, a slice that overran the declared size, a finalize whose bytes
 * never all arrived, and the host refusing the write.
 */
export const ATTACHMENT_ERROR_CODES = [
  'attachment_rejected',
  'attachment_chunk_invalid',
  'attachment_not_found',
  'attachment_quota_exceeded',
  'attachment_incomplete',
  'attachment_write_failed',
  'message_too_large',
  'safe_web_mode_blocked',
]

/**
 * Codes shared with the file flows (`path_invalid`, `path_escape`,
 * `workspace_unavailable`). They only mean "the upload failed" once the bytes
 * are staged and `finalize_attachment` has been sent; before that the same code
 * belongs to some other request and must not be blamed on the transfer.
 */
export const FINALIZE_ERROR_CODES = ['path_invalid', 'path_escape', 'workspace_unavailable']

export type UploadStatus = 'validating' | 'beginning' | 'uploading' | 'awaiting_approval' | 'done' | 'failed' | 'cancelled'

/** Whether an upload is still waiting for the host to act on it. */
export function uploadIsInFlight(upload: { status: UploadStatus } | undefined): boolean {
  return upload !== undefined && upload.status !== 'done' && upload.status !== 'failed' && upload.status !== 'cancelled'
}

/** Does this error end the upload currently in `status`? */
export function isUploadFailure(code: string, status: UploadStatus | undefined): boolean {
  if (!status || status === 'done' || status === 'cancelled') return false
  if (ATTACHMENT_ERROR_CODES.includes(code)) return true
  return FINALIZE_ERROR_CODES.includes(code) && (status === 'uploading' || status === 'awaiting_approval')
}

export type AttachmentSource = { filename: string; contentType: string; bytes: Uint8Array }

export function contentTypeFor(filename: string, declared: string): string {
  const dot = filename.toLowerCase().lastIndexOf('.')
  const expected = dot < 0 ? undefined : MIME_BY_EXTENSION[filename.slice(dot).toLowerCase()]
  return expected && !declared ? expected : declared
}

/** Standard base64, no line breaks. `btoa` exists in the browser and in Node 18+. */
export function base64(bytes: Uint8Array): string {
  const block = 8 * 1024
  let binary = ''
  for (let offset = 0; offset < bytes.length; offset += block) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + block))
  }
  return btoa(binary)
}

export function base64Length(byteLen: number): number {
  return 4 * Math.ceil(byteLen / 3)
}

/**
 * Length of the JSON frame carrying a slice of `byteLen` raw bytes.
 *
 * Measured against a real message rather than estimated by hand: the ids are the
 * 36-character UUIDs `crypto.randomUUID()` produces, so this is the frame the
 * browser will actually put on the wire.
 */
export function chunkFrameBytes(byteLen: number): number {
  const filler = (bytes: number) => 'x'.repeat(base64Length(bytes))
  return JSON.stringify({
    type: 'attachment_chunk',
    client_msg_id: '0'.repeat(36),
    upload_id: '0'.repeat(36),
    chunk: filler(byteLen),
  }).length
}

/** Slice offsets and lengths for a file of `byteLen`, each slice within one frame. */
export function chunkPlan(byteLen: number): Array<{ offset: number; length: number }> {
  const plan: Array<{ offset: number; length: number }> = []
  for (let offset = 0; offset < byteLen; offset += UPLOAD_CHUNK_BYTES) {
    plan.push({ offset, length: Math.min(UPLOAD_CHUNK_BYTES, byteLen - offset) })
  }
  return plan
}

export function validateAttachmentMessage(clientMsgId: string, source: AttachmentSource): ClientMessage {
  return {
    type: 'validate_attachment',
    client_msg_id: clientMsgId,
    filename: source.filename,
    byte_len: source.bytes.length,
    content_type: contentTypeFor(source.filename, source.contentType),
  }
}

export function beginAttachmentMessage(clientMsgId: string, sessionId: string, source: AttachmentSource): ClientMessage {
  return {
    type: 'begin_attachment',
    client_msg_id: clientMsgId,
    session_id: sessionId,
    filename: source.filename,
    content_type: contentTypeFor(source.filename, source.contentType),
    byte_len: source.bytes.length,
  }
}

/**
 * How many slices may be on the wire for one upload at a time.
 *
 * The host handles one connection's messages in arrival order and only keeps a
 * bounded number of them waiting (`MAX_IN_FLIGHT_PER_CONNECTION` in
 * `crates/codegen/xai-grok-web/src/lib.rs`), so a client that put a whole 10 MiB
 * attachment -- 218 slices -- on the socket at once would be refused partway
 * through and would have to call the failure its own. A window is sent, the host's
 * `attachment_progress` says how much of it landed, and the next window follows:
 * the transfer stays in order, bounded on both ends, and takes one round trip per
 * window rather than per slice.
 */
export const UPLOAD_WINDOW = 4

/** How many slices of a `byteLen` file the host has staged, given `receivedBytes`. */
export function slicesStaged(byteLen: number, receivedBytes: number): number {
  let staged = 0
  let end = 0
  for (const slice of chunkPlan(byteLen)) {
    end += slice.length
    if (receivedBytes < end) break
    staged += 1
  }
  return staged
}

/**
 * Slices to send now for a file of `byteLen`, with `sentSlices` already on the wire
 * and `receivedBytes` acknowledged by the host.
 *
 * Never more than the window ahead of what the host has confirmed, so a stalled or
 * refused host cannot be buried, and never past the last slice.
 */
export function uploadWindowSlices(byteLen: number, receivedBytes: number, sentSlices: number): number {
  const total = chunkPlan(byteLen).length
  const inFlight = sentSlices - slicesStaged(byteLen, receivedBytes)
  return Math.max(0, Math.min(total - sentSlices, UPLOAD_WINDOW - inFlight))
}

/**
 * One `attachment_chunk` per slice, in order. The host processes a connection's
 * messages in arrival order, so a `finalize_attachment` queued behind these is
 * not applied until every byte ahead of it has been staged.
 */
export function attachmentChunkMessages(uploadId: string, bytes: Uint8Array, nextId: () => string): ClientMessage[] {
  return chunkPlan(bytes.length).map((slice) => ({
    type: 'attachment_chunk',
    client_msg_id: nextId(),
    upload_id: uploadId,
    chunk: base64(bytes.subarray(slice.offset, slice.offset + slice.length)),
  }))
}

export function finalizeAttachmentMessage(clientMsgId: string, uploadId: string, relativePath: string): ClientMessage {
  return { type: 'finalize_attachment', client_msg_id: clientMsgId, upload_id: uploadId, relative_path: relativePath }
}

export function cancelAttachmentMessage(clientMsgId: string, uploadId: string): ClientMessage {
  return { type: 'cancel_attachment', client_msg_id: clientMsgId, upload_id: uploadId }
}

/** One line of progress for the panel; the states the host can leave us in all read differently. */
export function describeUpload(upload: { filename: string; byteLen: number; sentBytes: number; status: UploadStatus; path?: string; bytes?: number }): string {
  switch (upload.status) {
    case 'validating': return `正在向主机校验附件 ${upload.filename}…`
    case 'beginning': return `校验通过，正在建立上传：${upload.filename}`
    case 'uploading': return `正在上传 ${upload.filename}：${upload.sentBytes} / ${upload.byteLen} 字节`
    case 'awaiting_approval': return `附件已传完，等待审批写入：${upload.filename}`
    case 'done': return `附件已写入 ${upload.path ?? upload.filename}（${upload.bytes ?? upload.sentBytes} 字节）`
    case 'failed': return `上传失败：${upload.filename}`
    case 'cancelled': return `上传已取消：${upload.filename}`
  }
}
