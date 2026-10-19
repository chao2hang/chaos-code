//! 官方壳的 `zcode-agent` 附件通道（V4）：Composer 的图片/PDF 上行事务，以及把已
//! 发送附件读回对话行的分块查询。
//!
//! 前端从不把整份内容塞进一条消息：`attachmentBeginV4` 开一个暂存条目，
//! `attachmentChunkV4` 按 384 KiB 追加 `dataBase64` 切片，`attachmentCommitV4`
//! 校验声明的 `sha256:` 摘要之后引用（`ref`）才成立。这几个方法缺席时，Composer 的
//! 每一次粘贴都以 `Chaos 后端未实现通道方法 zcode-agent.attachmentBeginV4` 结束——
//! 附件永远停在上传态，发送按钮也就永远等不到「ready」。
//!
//! 落盘位置是 `<workspace>/.chaos/uploads/<sessionId>/<uploadId>/<name>`：`.gitignore`
//! 已经按「粘贴/附件暂存，机器本地，永不入仓」记下这个目录。返回的 `ref` 就是这个
//! 绝对路径，于是提示词侧的 `<image_files>` 块、`attachmentReadV4` 与 Agent 自己的
//! `read_file` 指的是同一个文件——`read_file` 读到图片时，Agent 会把它作为视觉内容块
//! 内联给模型（`xai-grok-shell/src/session/acp_session_impl/tool_calls.rs`），这条链
//! 才算走完。
//!
//! 上限取的是前端 `PROTOCOL_V4_LIMITS` 的同一组数（`~/.chaos/web/zcode-assets/assets/`
//! 那份 `src-*.js` 里的对象），不另立一套。

use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, Instant},
};

use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::confined_under;

/// 单附件上限（`attachmentMaxBytes`）。
const ATTACHMENT_MAX_BYTES: u64 = 20 * 1024 * 1024;
/// 单切片上限（`attachmentChunkMaxBytes`），同时也是预览读一次的返回上限。
const CHUNK_MAX_BYTES: usize = 512 * 1024;
/// 一次上传的切片数上限（`attachmentUploadMaxChunks`）。
const UPLOAD_MAX_CHUNKS: u64 = 64;
/// 同时暂存的上传数上限（`attachmentUploadMaxConcurrent`）。
const UPLOAD_MAX_CONCURRENT: usize = 16;
/// 暂存合计字节上限（`attachmentUploadMaxStagedBytes`）。
const UPLOAD_MAX_STAGED_BYTES: u64 = 64 * 1024 * 1024;
/// 暂存条目存活时长（`attachmentUploadTtlMs`）。
const UPLOAD_TTL: Duration = Duration::from_secs(300);
/// 预览读的总字节上限（`attachmentPreviewMaxBytes`）。
const PREVIEW_MAX_BYTES: u64 = 30 * 1024 * 1024;
/// 工作区里放暂存附件的目录，与 `.gitignore` 的 `/.chaos/uploads/` 是同一条约定。
const UPLOAD_DIR: &str = ".chaos/uploads";

/// 一份还没提交的上行内容。字节收在内存里：单附件 20 MiB、合计 64 MiB 是上限，
/// 而提交之后唯一留下的是盘上那份文件。
struct StagedUpload {
    total_bytes: u64,
    total_chunks: u64,
    checksum: String,
    next_chunk_index: u64,
    bytes: Vec<u8>,
    /// 提交后的最终路径，在 `begin` 就定下来，`chunk` 不再重新计算。
    target: PathBuf,
    touched_at: Instant,
}

/// 进程级的暂存表，键是 `uploadId`。和 `WORKSPACE_FILE_INDEX` 同一个理由：附件方法
/// 由 `handle_call` 这条同步分派回答（测试也直接调它），线程局部的连接状态到不了这里。
static STAGED: Mutex<BTreeMap<String, StagedUpload>> = Mutex::new(BTreeMap::new());

/// 一条 `zcode-agent` 附件调用 → 官方形状的答复值。`None` = 不是本模块的方法，
/// 调用方照旧往下分派。
pub fn handle(name: &str, arg: &Value, file_root: &Path) -> Option<Result<Value, String>> {
    match name {
        "attachmentBeginV4" => Some(begin(arg, file_root)),
        "attachmentChunkV4" => Some(chunk(arg)),
        "attachmentCommitV4" => Some(commit(arg)),
        "attachmentAbortV4" => Some(abort(arg)),
        "attachmentPreviewSourceV4" => Some(preview_source(arg)),
        "attachmentReadV4" => Some(read(arg, file_root)),
        _ => None,
    }
}

/// `attachmentBeginV4({uploadId, sessionId, fileName, mime, totalBytes, totalChunks,
/// checksum, workspacePath?})` → `{uploadId, state:"staging", nextChunkIndex}`，
/// 或零字节上传直接落盘的 `{uploadId, state:"committed", nextChunkIndex, ref}`。
fn begin(arg: &Value, file_root: &Path) -> Result<Value, String> {
    let upload_id = required_str(arg, "uploadId")?.to_owned();
    validate_upload_id(&upload_id)?;
    let session_id = required_str(arg, "sessionId")?.to_owned();
    let file_name = required_str(arg, "fileName")?.to_owned();
    // 声明的 mime 只核「有没有」：类型最终以字节/扩展名嗅到的为准（`media_type_of`），
    // 客户端声明的类型不参与落盘判定。
    required_str(arg, "mime")?;
    let checksum = required_str(arg, "checksum")?.to_owned();
    validate_checksum(&checksum)?;
    let total_bytes = required_u64(arg, "totalBytes")?;
    let total_chunks = required_u64(arg, "totalChunks")?;
    if total_bytes > ATTACHMENT_MAX_BYTES {
        return Err(format!(
            "附件 {total_bytes} 字节超过单附件上限 {ATTACHMENT_MAX_BYTES} 字节"
        ));
    }
    if total_chunks > UPLOAD_MAX_CHUNKS {
        return Err(format!(
            "附件 {total_chunks} 片超过单次上传上限 {UPLOAD_MAX_CHUNKS} 片"
        ));
    }
    if (total_bytes == 0) != (total_chunks == 0) {
        return Err("零字节附件必须声明零片".to_string());
    }
    let target = destination(arg, file_root, &session_id, &upload_id, &file_name)?;

    let mut staged = staged_lock();
    evict_expired(&mut staged);
    // 同一 uploadId 再来一次 = 客户端在续传（它拿回 nextChunkIndex 接着发），
    // 所以照现有进度回复，而不是把已经收到的字节扔掉重来。
    if let Some(existing) = staged.get(&upload_id) {
        if existing.checksum != checksum
            || existing.total_bytes != total_bytes
            || existing.target != target
        {
            return Err(format!("附件上传 {upload_id} 已经用另一份内容开过"));
        }
        return Ok(json!({
            "uploadId": upload_id,
            "state": "staging",
            "nextChunkIndex": existing.next_chunk_index,
        }));
    }
    // 零片上传没有 `chunk` 可等，`begin` 就是它唯一一次机会：直接落盘并回 committed。
    if total_chunks == 0 {
        write_committed(&target, &[])?;
        return Ok(json!({
            "uploadId": upload_id,
            "state": "committed",
            "nextChunkIndex": 0,
            "ref": target.to_string_lossy(),
        }));
    }
    if staged.len() >= UPLOAD_MAX_CONCURRENT {
        return Err(format!(
            "暂存中的附件上传已有 {} 个（上限 {UPLOAD_MAX_CONCURRENT}），请等它们提交或过期",
            staged.len()
        ));
    }
    let staged_bytes: u64 = staged
        .values()
        .map(|entry| entry.bytes.len() as u64)
        .sum::<u64>()
        .saturating_add(total_bytes);
    if staged_bytes > UPLOAD_MAX_STAGED_BYTES {
        return Err(format!(
            "暂存附件合计会超过 {UPLOAD_MAX_STAGED_BYTES} 字节上限，请先提交或放弃其它上传"
        ));
    }
    staged.insert(
        upload_id.clone(),
        StagedUpload {
            total_bytes,
            total_chunks,
            checksum,
            next_chunk_index: 0,
            bytes: Vec::with_capacity(total_bytes as usize),
            target,
            touched_at: Instant::now(),
        },
    );
    Ok(json!({
        "uploadId": upload_id,
        "state": "staging",
        "nextChunkIndex": 0,
    }))
}

/// `attachmentChunkV4({uploadId, sessionId, chunkIndex, dataBase64})` →
/// `{uploadId, nextChunkIndex}`；`nextChunkIndex` 恒等于 `chunkIndex + 1`，前端的
/// 进度校验就钉在这条等式上。
fn chunk(arg: &Value) -> Result<Value, String> {
    let upload_id = required_str(arg, "uploadId")?;
    let chunk_index = required_u64(arg, "chunkIndex")?;
    let data = arg
        .get("dataBase64")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .map_err(|_| format!("附件上传 {upload_id} 的 dataBase64 不是合法 base64"))?;
    if decoded.len() > CHUNK_MAX_BYTES {
        return Err(format!(
            "附件上传 {upload_id} 的单片解析出 {} 字节，超过 {CHUNK_MAX_BYTES} 字节上限",
            decoded.len()
        ));
    }

    let mut staged = staged_lock();
    evict_expired(&mut staged);
    let Some(entry) = staged.get_mut(upload_id) else {
        return Err(format!(
            "附件上传 {upload_id} 不在暂存里：可能已提交、已放弃或已过期"
        ));
    };
    entry.touched_at = Instant::now();
    if chunk_index < entry.next_chunk_index {
        // 重发已经收下的那一片：照当前进度回，不重复追加。
        return Ok(json!({
            "uploadId": upload_id,
            "nextChunkIndex": entry.next_chunk_index,
        }));
    }
    if chunk_index > entry.next_chunk_index {
        return Err(format!(
            "附件上传 {upload_id} 收到乱序切片：期望 {}，收到 {chunk_index}",
            entry.next_chunk_index
        ));
    }
    if entry.bytes.len() as u64 + decoded.len() as u64 > entry.total_bytes {
        return Err(format!(
            "附件上传 {upload_id} 收到的字节会超过声明的 {} 字节",
            entry.total_bytes
        ));
    }
    entry.bytes.extend_from_slice(&decoded);
    entry.next_chunk_index += 1;
    Ok(json!({
        "uploadId": upload_id,
        "nextChunkIndex": entry.next_chunk_index,
    }))
}

/// `attachmentCommitV4({uploadId, sessionId})` → `{ref}`。字节齐了、`sha256:` 对上了
/// 才落盘；任何一步不对就把暂存条目丢掉并把原因说清楚，不留下半份附件。
fn commit(arg: &Value) -> Result<Value, String> {
    let upload_id = required_str(arg, "uploadId")?;
    let mut staged = staged_lock();
    let Some(entry) = staged.remove(upload_id) else {
        return Err(format!(
            "附件上传 {upload_id} 不在暂存里：可能已提交、已放弃或已过期"
        ));
    };
    if entry.bytes.len() as u64 != entry.total_bytes {
        return Err(format!(
            "附件 {upload_id} 只收到 {} 字节，声明的 {} 字节还没齐",
            entry.bytes.len(),
            entry.total_bytes
        ));
    }
    if entry.next_chunk_index != entry.total_chunks {
        return Err(format!(
            "附件 {upload_id} 收到 {} 片，声明的 {} 片还没齐",
            entry.next_chunk_index, entry.total_chunks
        ));
    }
    let actual = format!("sha256:{}", hex_digest(&entry.bytes));
    if actual != entry.checksum {
        return Err(format!(
            "附件 {upload_id} 的内容摘要与声明不符：收到 {actual}，声明 {}",
            entry.checksum
        ));
    }
    write_committed(&entry.target, &entry.bytes)?;
    Ok(json!({ "ref": entry.target.to_string_lossy() }))
}

/// `attachmentAbortV4({uploadId, sessionId})` → `{}`。幂等：没这条也是成功。
fn abort(arg: &Value) -> Result<Value, String> {
    let upload_id = required_str(arg, "uploadId")?;
    staged_lock().remove(upload_id);
    Ok(json!({}))
}

/// `attachmentPreviewSourceV4({sessionId, ref, clientMode})` → `{kind}`。
/// Web 端没有本地媒体协议 URL（`createLocalMediaPreviewUrl` 只在 desktop 注入），
/// 所以一律如实回 `chunked`，让调用方走分块读，而不是造一个浏览器打不开的路径。
fn preview_source(arg: &Value) -> Result<Value, String> {
    required_str(arg, "sessionId")?;
    required_str(arg, "ref")?;
    Ok(json!({ "kind": "chunked" }))
}

/// `attachmentReadV4({sessionId, ref, offset, limit})` →
/// `{dataBase64, mediaType, totalBytes, nextOffset}`。前端按 `limit` = 512 KiB 循环读到
/// `nextOffset === null`，并且要求 `nextOffset === offset + 本片字节数`。
fn read(arg: &Value, file_root: &Path) -> Result<Value, String> {
    let reference = required_str(arg, "ref")?;
    let offset = required_u64(arg, "offset")?;
    let limit = required_u64(arg, "limit")?;
    if limit == 0 || limit > CHUNK_MAX_BYTES as u64 {
        return Err(format!(
            "attachmentReadV4 的 limit 必须在 1..={CHUNK_MAX_BYTES} 之间，收到 {limit}"
        ));
    }
    // 与其余 file.* 同一条约束：只读浏览器那个根里的文件，认证状态除外。
    let path =
        super::confined_file_with_root(reference, file_root).map_err(|error| error.to_string())?;
    let metadata =
        fs::metadata(&path).map_err(|error| format!("附件 {reference} 读不到：{error}"))?;
    if !metadata.is_file() {
        return Err(format!("附件 {reference} 不是普通文件"));
    }
    let total_bytes = metadata.len();
    if total_bytes > PREVIEW_MAX_BYTES {
        return Err(format!(
            "附件 {reference} 有 {total_bytes} 字节，超过预览上限 {PREVIEW_MAX_BYTES} 字节"
        ));
    }
    if offset > total_bytes {
        return Err(format!(
            "附件 {reference} 的 offset {offset} 超过了总字节 {total_bytes}"
        ));
    }
    let media_type = media_type_of(&path, arg.get("mediaType").and_then(Value::as_str))?;
    let take = (total_bytes - offset).min(limit) as usize;
    let mut bytes = vec![0u8; take];
    if take > 0 {
        let mut file =
            fs::File::open(&path).map_err(|error| format!("附件 {reference} 打不开：{error}"))?;
        file.seek(SeekFrom::Start(offset))
            .map_err(|error| format!("附件 {reference} 定位失败：{error}"))?;
        file.read_exact(&mut bytes)
            .map_err(|error| format!("附件 {reference} 读取失败：{error}"))?;
    }
    let next_offset = offset + take as u64;
    Ok(json!({
        "dataBase64": base64::engine::general_purpose::STANDARD.encode(&bytes),
        "mediaType": media_type,
        "totalBytes": total_bytes,
        "nextOffset": (next_offset < total_bytes).then_some(next_offset),
    }))
}

/// 提交后的绝对路径。工作区由客户端在参数里给（`workspacePath`），但同样受
/// `CHAOS_WEB_FILE_ROOT` 约束——`file.*` 读不出去的地方，附件也不该写得进去。
fn destination(
    arg: &Value,
    file_root: &Path,
    session_id: &str,
    upload_id: &str,
    file_name: &str,
) -> Result<PathBuf, String> {
    let root = match arg
        .get("workspacePath")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|path| !path.is_empty())
    {
        Some(workspace) => {
            confined_under(workspace, file_root).map_err(|error| error.to_string())?
        }
        // 老客户端不带工作区：落在浏览器那个根上，不越界。
        None => file_root.to_path_buf(),
    };
    if !root.is_dir() {
        return Err(format!("{} 不是可写入的工作区目录", root.display()));
    }
    Ok(root
        .join(UPLOAD_DIR)
        .join(safe_name(session_id, "session"))
        // uploadId 单独占一层：`-` 在 uploadId 里是合法字符，平铺成
        // `<uploadId>-<name>` 之后就没法在还原时把原名拆回来，而 `fileName` 要的
        // 正是那个原名（这里只有最后一段是文件本身）。
        .join(safe_name(upload_id, "upload"))
        .join(safe_name(file_name, "attachment")))
}

/// 先写临时名再改名：读到半份附件的窗口不该存在。目录按 owner-only 建，
/// 粘贴进来的截图常常是不能给别人看的东西。
fn write_committed(target: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = target
        .parent()
        .ok_or_else(|| format!("附件落盘路径 {} 没有父目录", target.display()))?;
    fs::create_dir_all(dir)
        .map_err(|error| format!("附件目录 {} 建不出来：{error}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o700));
    }
    let name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("attachment");
    let temp = dir.join(format!(".{name}.part"));
    fs::write(&temp, bytes)
        .map_err(|error| format!("附件 {} 写不进去：{error}", temp.display()))?;
    fs::rename(&temp, target)
        .map_err(|error| format!("附件 {} 改名失败：{error}", target.display()))?;
    Ok(())
}

fn staged_lock() -> std::sync::MutexGuard<'static, BTreeMap<String, StagedUpload>> {
    STAGED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn evict_expired(staged: &mut BTreeMap<String, StagedUpload>) {
    let now = Instant::now();
    staged.retain(|_, entry| now.duration_since(entry.touched_at) < UPLOAD_TTL);
}

fn required_str<'a>(arg: &'a Value, key: &str) -> Result<&'a str, String> {
    arg.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("附件调用缺少 {key}"))
}

fn required_u64(arg: &Value, key: &str) -> Result<u64, String> {
    arg.get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("附件调用缺少 {key}"))
}

/// 官方 `v4AttachmentUploadIdSchema` 的字符集与长度（`^[A-Za-z0-9][A-Za-z0-9._:-]*$`，
/// 最长 128）。它同时是落盘名字的前缀，所以这道校验也是路径安全的一部分。
fn validate_upload_id(upload_id: &str) -> Result<(), String> {
    let bytes = upload_id.as_bytes();
    let shaped = (1..=128).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-:".contains(byte));
    if !shaped {
        return Err(format!("uploadId {upload_id:?} 不是合法的上传 id"));
    }
    Ok(())
}

/// 官方 `v4AttachmentChecksumSchema`：`sha256:` + 64 位小写十六进制。
fn validate_checksum(checksum: &str) -> Result<(), String> {
    let valid = checksum.len() == 71
        && checksum.starts_with("sha256:")
        && checksum[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !valid {
        return Err(format!(
            "checksum {checksum:?} 不是 sha256: 加 64 位小写十六进制"
        ));
    }
    Ok(())
}

/// 客户端给的名字只当显示名的来源：盘上的名字去掉目录成分、挡掉路径分隔符，
/// 保留可辨认的扩展名。落不下来的字符换成 `_`，绝不悄悄跳出 uploads 目录。
fn safe_name(raw: &str, fallback: &str) -> String {
    let base = raw.rsplit(['/', '\\']).next().unwrap_or_default().trim();
    let mut out = String::new();
    for ch in base.chars() {
        if out.len() >= 96 {
            break;
        }
        if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ' ') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    // "." / ".." / 全是点的名字在这一步变成空串，于是取兜底名。
    let trimmed = out.trim_matches('.').trim();
    if trimmed.is_empty() {
        fallback.to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        hex.push_str(&format!("{byte:02x}"));
    }
    hex
}

/// 预览读要回的 `mediaType`：先看字节的真实签名，再看扩展名，最后才用客户端声明的
/// mime。前端（`v4AttachmentReadResultSchema`）只认 image/* | video/* | application/pdf，
/// 别的一律如实拒绝，而不是回一个它必定拒绝的值。
fn media_type_of(path: &Path, declared: Option<&str>) -> Result<String, String> {
    if let Some(sniffed) = sniff_media_type(path) {
        return Ok(sniffed.to_owned());
    }
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        if let Some(mime) = extension_media_type(&extension.to_ascii_lowercase()) {
            return Ok(mime.to_owned());
        }
    }
    if let Some(declared) = declared.map(str::trim).filter(|mime| {
        let mime = mime.to_ascii_lowercase();
        mime.starts_with("image/")
            || mime.starts_with("video/")
            || mime
                .split(';')
                .next()
                .is_some_and(|head| head.trim() == "application/pdf")
    }) {
        return Ok(declared.to_ascii_lowercase());
    }
    Err(format!(
        "附件 {} 的类型不是图片、视频或 PDF，没有可预览的媒体类型",
        path.display()
    ))
}

fn sniff_media_type(path: &Path) -> Option<&'static str> {
    let mut head = [0u8; 16];
    let mut file = fs::File::open(path).ok()?;
    let read = file.read(&mut head).ok()?;
    let head = &head[..read];
    if head.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        return Some("image/png");
    }
    if head.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if head.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if head.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some("video/webm");
    }
    if head.len() >= 12 {
        if head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
            return Some("image/webp");
        }
        if &head[4..8] == b"ftyp" {
            return Some("video/mp4");
        }
    }
    None
}

fn extension_media_type(extension: &str) -> Option<&'static str> {
    match extension {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "pdf" => Some("application/pdf"),
        "mp4" | "m4v" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "webm" => Some("video/webm"),
        "ogv" => Some("video/ogg"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 暂存表是进程级的，下面几例又各自盯着它的上限/清空，所以逐例串行：并行跑时
    /// 一例留在表里的暂存条目会去撞另一例的并发上限（`workspace_file_search_tests`
    /// 的 `INDEX_LOCK` 是同一个理由）。
    static STAGING_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn locked() -> std::sync::MutexGuard<'static, ()> {
        STAGING_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// 把上一例的残留清掉：验上限的那条得从空表开始数。
    fn clear_staging() {
        staged_lock().clear();
    }

    /// 一份真的 1x1 PNG：签名、切片与摘要都照生产那份走。
    const PNG: &[u8] = &[
        0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x08, 0xd7, 0x63, 0xf8,
        0xcf, 0xc0, 0x00, 0x00, 0x00, 0x03, 0x00, 0x01, 0x00, 0x05, 0xfe, 0xd4, 0xef, 0x00, 0x00,
        0x00, 0x00, 0x49, 0x45, 0x4e, 0x44, 0xae, 0x42, 0x60, 0x82,
    ];

    fn encode(bytes: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    fn begin_arg(root: &Path, upload_id: &str, bytes: &[u8], chunks: u64) -> Value {
        json!({
            "connectionId": "conn-test",
            "uploadId": upload_id,
            "sessionId": "session-1",
            "workspacePath": root.to_string_lossy(),
            "fileName": "screenshot.png",
            "mime": "image/png",
            "totalBytes": bytes.len() as u64,
            "totalChunks": chunks,
            "checksum": format!("sha256:{}", hex_digest(bytes)),
        })
    }

    fn answer(result: Option<Result<Value, String>>) -> Value {
        result.expect("这些方法都归本模块回答").expect("应当成功")
    }

    fn refused(result: Option<Result<Value, String>>) -> String {
        result
            .expect("这些方法都归本模块回答")
            .expect_err("应当被拒绝")
    }

    #[test]
    fn begin_chunk_commit_writes_the_bytes_the_client_declared() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let bytes = PNG;
        let upload_id = "upload-3f2a";

        // 两片：第二片故意只放几个字节，走的是真实的分片边界。
        let split = 40usize;
        let began = answer(handle(
            "attachmentBeginV4",
            &begin_arg(root, upload_id, bytes, 2),
            root,
        ));
        assert_eq!(began["state"], "staging");
        assert_eq!(began["nextChunkIndex"], 0);

        let first = answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&bytes[..split]),
            }),
            root,
        ));
        assert_eq!(first["nextChunkIndex"], 1);

        let second = answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 1,
                "dataBase64": encode(&bytes[split..]),
            }),
            root,
        ));
        assert_eq!(second["nextChunkIndex"], 2);

        let committed = answer(handle(
            "attachmentCommitV4",
            &json!({ "uploadId": upload_id, "sessionId": "session-1" }),
            root,
        ));
        let reference = committed["ref"].as_str().expect("commit 必回 ref");
        let expected = root
            .join(".chaos/uploads/session-1")
            .join(upload_id)
            .join("screenshot.png");
        assert_eq!(Path::new(reference), expected);
        assert_eq!(std::fs::read(&expected).unwrap(), bytes);
        // 半份临时文件不许留下。
        assert!(
            !std::fs::read_dir(expected.parent().unwrap())
                .unwrap()
                .any(|entry| entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".part"))
        );
    }

    #[test]
    fn chunk_from_an_unknown_upload_is_named_not_silent() {
        let _guard = locked();
        let root = tempfile::tempdir().unwrap();
        let message = refused(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": "upload-nope",
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(PNG),
            }),
            root.path(),
        ));
        assert!(message.contains("upload-nope"), "{message}");
        assert!(message.contains("暂存"), "{message}");
    }

    #[test]
    fn commit_refuses_a_content_that_does_not_match_the_checksum() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let upload_id = "upload-checksum";
        answer(handle(
            "attachmentBeginV4",
            &begin_arg(root, upload_id, PNG, 1),
            root,
        ));
        // 声明的是 PNG 的摘要，字节数分毫不差但内容被改过一个位：提交必须在摘要
        // 这一步失败（字节数那条是另一例），且不留下文件。
        let mut tampered = PNG.to_vec();
        tampered[0] ^= 0xff;
        answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&tampered),
            }),
            root,
        ));
        let message = refused(handle(
            "attachmentCommitV4",
            &json!({ "uploadId": upload_id, "sessionId": "session-1" }),
            root,
        ));
        assert!(message.contains("摘要"), "{message}");
        assert!(!root.join(".chaos/uploads/session-1").exists());
        // 失败之后条目已经作废：再提交一次是「不在暂存里」，不是重复写盘。
        assert!(
            refused(handle(
                "attachmentCommitV4",
                &json!({ "uploadId": upload_id, "sessionId": "session-1" }),
                root,
            ))
            .contains("暂存")
        );
    }

    #[test]
    fn chunk_refuses_out_of_order_slices_and_oversized_ones() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let upload_id = "upload-order";
        answer(handle(
            "attachmentBeginV4",
            &begin_arg(root, upload_id, PNG, 2),
            root,
        ));
        let out_of_order = refused(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 1,
                "dataBase64": encode(PNG),
            }),
            root,
        ));
        assert!(out_of_order.contains("乱序"), "{out_of_order}");

        let oversized = refused(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&vec![0u8; CHUNK_MAX_BYTES + 1]),
            }),
            root,
        ));
        assert!(oversized.contains("上限"), "{oversized}");

        // 同一片重发是幂等的：回当前进度，不重复追加。
        answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&PNG[..8]),
            }),
            root,
        ));
        let resent = answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&PNG[..8]),
            }),
            root,
        ));
        assert_eq!(resent["nextChunkIndex"], 1);
    }

    #[test]
    fn begin_refuses_an_upload_over_the_declared_limits() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let mut arg = begin_arg(root, "upload-big", PNG, 1);
        arg["totalBytes"] = json!(ATTACHMENT_MAX_BYTES + 1);
        let message = refused(handle("attachmentBeginV4", &arg, root));
        assert!(message.contains("单附件上限"), "{message}");

        let mut arg = begin_arg(root, "upload-many", PNG, 1);
        arg["totalChunks"] = json!(UPLOAD_MAX_CHUNKS + 1);
        assert!(refused(handle("attachmentBeginV4", &arg, root)).contains("单次上传上限"),);

        // 零字节必须零片：两个数字得说同一件事。
        let mut arg = begin_arg(root, "upload-zero", &[], 1);
        arg["totalBytes"] = json!(0);
        assert!(refused(handle("attachmentBeginV4", &arg, root)).contains("零字节"),);

        // 摘要形状不对：早期就拒，不放一个假摘要进暂存。
        let mut arg = begin_arg(root, "upload-digest", PNG, 1);
        arg["checksum"] = json!("sha256:zz");
        assert!(refused(handle("attachmentBeginV4", &arg, root)).contains("checksum"),);
    }

    #[test]
    fn a_zero_byte_upload_commits_at_begin() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let began = answer(handle(
            "attachmentBeginV4",
            &begin_arg(root, "upload-empty", &[], 0),
            root,
        ));
        assert_eq!(began["state"], "committed");
        let reference = began["ref"].as_str().expect("committed 必带 ref");
        assert_eq!(std::fs::read(reference).unwrap(), Vec::<u8>::new());
    }

    #[test]
    fn resume_returns_the_progress_instead_of_restarting_the_upload() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let upload_id = "upload-resume";
        let arg = begin_arg(root, upload_id, PNG, 2);
        answer(handle("attachmentBeginV4", &arg, root));
        answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": upload_id,
                "sessionId": "session-1",
                "chunkIndex": 0,
                "dataBase64": encode(&PNG[..40]),
            }),
            root,
        ));
        let resumed = answer(handle("attachmentBeginV4", &arg, root));
        assert_eq!(resumed["state"], "staging");
        assert_eq!(resumed["nextChunkIndex"], 1);

        // 同一个 uploadId 换一份内容：是调用方错了，得说出来。
        let mut other = begin_arg(root, upload_id, b"another payload", 2);
        other["totalBytes"] = json!(15);
        assert!(refused(handle("attachmentBeginV4", &other, root)).contains("另一份内容"),);
    }

    #[test]
    fn abort_forgets_the_staging_entry() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let upload_id = "upload-abort";
        answer(handle(
            "attachmentBeginV4",
            &begin_arg(root, upload_id, PNG, 1),
            root,
        ));
        assert_eq!(
            answer(handle(
                "attachmentAbortV4",
                &json!({ "uploadId": upload_id, "sessionId": "session-1" }),
                root,
            )),
            json!({})
        );
        assert!(
            refused(handle(
                "attachmentChunkV4",
                &json!({
                    "uploadId": upload_id,
                    "sessionId": "session-1",
                    "chunkIndex": 0,
                    "dataBase64": encode(PNG),
                }),
                root,
            ))
            .contains("不在暂存里")
        );
        // 幂等：再放弃一次也是 `{}`。
        assert_eq!(
            answer(handle(
                "attachmentAbortV4",
                &json!({ "uploadId": upload_id, "sessionId": "session-1" }),
                root,
            )),
            json!({})
        );
    }

    #[test]
    fn staging_is_bounded_by_the_concurrency_cap() {
        let _guard = locked();
        clear_staging();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        // 每个条目声明 1 片、内容 1 字节，好让这条只撞并发上限，不撞字节上限。
        let small = b"x";
        for index in 0..UPLOAD_MAX_CONCURRENT {
            let upload_id = format!("upload-{index:02}");
            answer(handle(
                "attachmentBeginV4",
                &begin_arg(root, &upload_id, small, 1),
                root,
            ));
        }
        let message = refused(handle(
            "attachmentBeginV4",
            &begin_arg(root, "upload-overflow", small, 1),
            root,
        ));
        assert!(message.contains("上限"), "{message}");
        // 清场，别给同二进制里的别的用例留下暂存条目。
        for index in 0..UPLOAD_MAX_CONCURRENT {
            let _ = handle(
                "attachmentAbortV4",
                &json!({
                    "uploadId": format!("upload-{index:02}"),
                    "sessionId": "session-1",
                }),
                root,
            );
        }
    }

    #[test]
    fn read_serves_one_slice_at_a_time_and_reports_the_real_type() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        // 直接落一份文件，模拟已提交的附件（含没有扩展名的情形：类型只能靠字节）。
        let dir = root.join("work/.chaos/uploads/session-1/upload-x");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("attachment");
        std::fs::write(&path, PNG).unwrap();
        let reference = path.to_string_lossy().into_owned();

        let first = answer(handle(
            "attachmentReadV4",
            &json!({
                "sessionId": "session-1",
                "ref": reference,
                "offset": 0,
                "limit": 16,
            }),
            root,
        ));
        assert_eq!(first["mediaType"], "image/png");
        assert_eq!(first["totalBytes"], PNG.len() as u64);
        assert_eq!(first["nextOffset"], 16);
        let slice = base64::engine::general_purpose::STANDARD
            .decode(first["dataBase64"].as_str().unwrap())
            .unwrap();
        assert_eq!(slice, &PNG[..16]);

        // 尾片：nextOffset 必须是 null，字节数正好把文件读完。
        let tail = answer(handle(
            "attachmentReadV4",
            &json!({
                "sessionId": "session-1",
                "ref": reference,
                "offset": 16,
                "limit": CHUNK_MAX_BYTES,
            }),
            root,
        ));
        assert!(tail["nextOffset"].is_null(), "{tail}");
        let rest = base64::engine::general_purpose::STANDARD
            .decode(tail["dataBase64"].as_str().unwrap())
            .unwrap();
        assert_eq!(rest, &PNG[16..]);
    }

    #[test]
    fn read_refuses_an_unreadable_type_and_a_path_outside_the_root() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let workspace = served.path().join("work");
        let dir = workspace.join(".chaos/uploads/session-1/upload-y");
        std::fs::create_dir_all(&dir).unwrap();
        let note = dir.join("note.txt");
        std::fs::write(&note, "不是媒体文件").unwrap();
        let message = refused(handle(
            "attachmentReadV4",
            &json!({
                "sessionId": "session-1",
                "ref": note.to_string_lossy(),
                "offset": 0,
                "limit": 16,
            }),
            &workspace,
        ));
        assert!(message.contains("媒体类型"), "{message}");

        // 根外的文件读不到：与 file.* 同一条 confinement。
        let outside = tempfile::tempdir().unwrap();
        let stray = outside.path().join("shot.png");
        std::fs::write(&stray, PNG).unwrap();
        let message = refused(handle(
            "attachmentReadV4",
            &json!({
                "sessionId": "session-1",
                "ref": stray.to_string_lossy(),
                "offset": 0,
                "limit": 16,
            }),
            &workspace,
        ));
        assert!(message.contains("CHAOS_WEB_FILE_ROOT"), "{message}");
    }

    #[test]
    fn preview_source_is_honestly_chunked_for_the_web() {
        let _guard = locked();
        let root = tempfile::tempdir().unwrap();
        let answer = answer(handle(
            "attachmentPreviewSourceV4",
            &json!({
                "sessionId": "session-1",
                "ref": "/abs/shot.mp4",
                "clientMode": "web-remote-replayable",
            }),
            root.path(),
        ));
        assert_eq!(answer, json!({ "kind": "chunked" }));
        // 参数不全时是有名字的报错，不是一个假成功。
        assert!(
            refused(handle(
                "attachmentPreviewSourceV4",
                &json!({ "sessionId": "session-1" }),
                root.path(),
            ))
            .contains("ref")
        );
    }

    #[test]
    fn a_workspace_outside_the_browser_root_is_refused_with_the_same_words_as_file_rpc() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let message = refused(handle(
            "attachmentBeginV4",
            &begin_arg(outside.path(), "upload-outside", PNG, 1),
            served.path(),
        ));
        assert!(message.contains("CHAOS_WEB_FILE_ROOT"), "{message}");
    }

    #[test]
    fn a_hostile_session_or_file_name_stays_inside_the_uploads_directory() {
        let _guard = locked();
        let served = tempfile::tempdir().unwrap();
        let root = served.path();
        let bytes = PNG;
        let mut arg = begin_arg(root, "upload-evil", bytes, 1);
        arg["sessionId"] = json!("../../etc");
        arg["fileName"] = json!("../../../../etc/passwd.png");
        answer(handle("attachmentBeginV4", &arg, root));
        answer(handle(
            "attachmentChunkV4",
            &json!({
                "uploadId": "upload-evil",
                "sessionId": "../../etc",
                "chunkIndex": 0,
                "dataBase64": encode(bytes),
            }),
            root,
        ));
        let committed = answer(handle(
            "attachmentCommitV4",
            &json!({ "uploadId": "upload-evil", "sessionId": "../../etc" }),
            root,
        ));
        let reference = PathBuf::from(committed["ref"].as_str().unwrap());
        let uploads = root.join(".chaos/uploads");
        assert!(
            reference.starts_with(&uploads),
            "落盘路径必须还在 uploads 目录里：{}",
            reference.display()
        );
        assert!(
            !reference.to_string_lossy().contains(".."),
            "{}",
            reference.display()
        );
    }

    #[test]
    fn only_the_six_attachment_methods_are_claimed() {
        let _guard = locked();
        let root = tempfile::tempdir().unwrap();
        for name in [
            "attachmentBeginV4",
            "attachmentChunkV4",
            "attachmentCommitV4",
            "attachmentAbortV4",
            "attachmentPreviewSourceV4",
            "attachmentReadV4",
        ] {
            assert!(
                handle(name, &json!({}), root.path()).is_some(),
                "{name} 得由本模块回答"
            );
        }
        assert!(handle("attachmentPutV4", &json!({}), root.path()).is_none());
        assert!(handle("sendText", &json!({}), root.path()).is_none());
    }
}
