//! App Usage 聚合：把 GUI 引擎记录的真实轮次用量汇总成官方前端
//! `usage-stats.getAppUsageSnapshot` 需要的 `AppUsageSnapshot` 载荷。
//!
//! 数据只来自本进程真实发生的事：每个 Provider 轮次的 token 用量
//! （`HttpPromptAdapter` 从 SSE `usage` 帧读到的）、工具执行的次数与结果。
//! 没有任何字段是估算或补造的；Provider 没有回报用量时对应计数就是 0，
//! 历史会话（本功能上线前）没有时间戳，不进入任何按天/区间的统计。

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

/// 每个会话保留的最近轮次记录上限。轮次记录本身很小（无正文），
/// 上限只是为了给极端长寿命会话一个确定性的界。
pub const MAX_TRACKED_TURNS_PER_SESSION: usize = 4096;
/// 每个会话保留的最近工具事件上限（用于按天热力图的工具计数）。
pub const MAX_TOOL_EVENTS_PER_SESSION: usize = 1024;

/// 一次 Provider 轮次的真实用量记录。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TurnUsage {
    /// 轮次开始时间（Unix 毫秒）。
    pub at_ms: u64,
    /// 本次请求的模型（GUI 选择或 Provider 默认）；演示轮次为 None。
    #[serde(default)]
    pub model: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// 推理 token（Provider 报告了才非零；OpenAI 兼容口径下已含在 output 内）。
    #[serde(default)]
    pub reasoning_tokens: u64,
    /// 缓存读 token（OpenAI 口径下已含在 input 内）。
    #[serde(default)]
    pub cached_tokens: u64,
    /// 缓存写 token（Anthropic 兼容口径，独立于 input）。
    #[serde(default)]
    pub cache_creation_tokens: u64,
    /// 轮次总时长（毫秒）。
    #[serde(default)]
    pub duration_ms: Option<u64>,
    /// 首 token 延迟（毫秒）。
    #[serde(default)]
    pub ttft_ms: Option<u64>,
    /// false = Provider 报错的失败尝试（计入 modelErrorRate，不计入产出）。
    #[serde(default = "default_true")]
    pub ok: bool,
}

fn default_true() -> bool {
    true
}

impl TurnUsage {
    /// Provider 回报的 token 总量（输入 + 输出；缓存与推理按其口径已含在内）。
    #[must_use]
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// 一个工具的累计执行计数（全会话，无时间戳；按天分布看 [`ToolEvent`]）。
#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolUsageStat {
    pub calls: u64,
    pub errors: u64,
}

/// 一次工具执行的事件（有时间戳；供热力图按天计数，受容量上限截断）。
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct ToolEvent {
    pub at_ms: u64,
    pub tool: String,
    pub ok: bool,
}

/// 一个会话的用量事实切片（聚合器的输入）。
pub struct UsageFacts<'a> {
    pub turns: &'a [TurnUsage],
    pub tools: &'a BTreeMap<String, ToolUsageStat>,
    pub tool_events: &'a [ToolEvent],
}

// ── 汇总输出的 serde 形状（appUsageSnapshotSchema）──

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct SummaryOut {
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    reasoning_tokens: u64,
    cache_creation_tokens: u64,
    cache_read_tokens: u64,
    cache_hit_rate: f64,
    total_sessions: u64,
    total_turns: u64,
    tool_call_count: u64,
    tool_error_rate: f64,
    model_error_rate: f64,
    avg_time_to_first_token_ms: Option<u64>,
    avg_turn_duration_ms: Option<u64>,
    active_days: u64,
    current_streak_days: u64,
    longest_session_ms: u64,
    longest_streak_days: u64,
    peak_day_tokens: u64,
    favorite_model: Option<FavoriteModelOut>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct FavoriteModelOut {
    model_id: Option<String>,
    total_tokens: u64,
    share: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HeatmapCellOut {
    date: String,
    level: u8,
    total_tokens: u64,
    turn_count: u64,
    tool_call_count: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HeatmapWeekOut {
    week_index: usize,
    days: Vec<Option<HeatmapCellOut>>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct HeatmapOut {
    start_date: Option<String>,
    end_date: Option<String>,
    max_tokens: u64,
    weeks: Vec<HeatmapWeekOut>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyModelItemOut {
    model_id: Option<String>,
    total_tokens: u64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DailyModelUsageOut {
    date: String,
    models: Vec<DailyModelItemOut>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ModelUsageOut {
    model_id: Option<String>,
    total_tokens: u64,
    input_tokens: u64,
    output_tokens: u64,
    request_count: u64,
    share: f64,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolUsageOut {
    tool_name: String,
    call_count: u64,
    error_count: u64,
    error_rate: f64,
    avg_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUsageSnapshotOut {
    range: String,
    generated_at: u64,
    time_zone: String,
    source: &'static str,
    summary: SummaryOut,
    heatmap: HeatmapOut,
    daily_model_usage: Vec<DailyModelUsageOut>,
    models: Vec<ModelUsageOut>,
    tools: Vec<ToolUsageOut>,
}

/// 当前墙钟（Unix 毫秒）。时钟不可用时回退 0，聚合端把这类记录当作
/// "无时间戳" 处理而不是崩溃。
#[must_use]
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ── 纯日期换算（Howard Hinnant 的 civil 算法，无外部依赖）──

/// 天序号（1970-01-01 = 0）→ (年, 月, 日)。
#[must_use]
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// (年, 月, 日) → 天序号。
#[must_use]
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = i64::from(m <= 2) * (y - 1) + i64::from(m > 2) * y;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 天序号 → `YYYY-MM-DD`。
#[must_use]
pub fn format_day(days: i64) -> String {
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// 天序号 → 周几（0 = 周日）。1970-01-01 是周四。
#[must_use]
pub fn weekday_sunday_first(days: i64) -> u32 {
    (days + 4).rem_euclid(7) as u32
}

/// 毫秒时间戳 → 用户时区的天序号（`offset_seconds` = 该时区相对 UTC 的秒数）。
#[must_use]
pub fn local_day(at_ms: u64, offset_seconds: i64) -> i64 {
    let secs = (at_ms / 1000) as i64 + offset_seconds;
    secs.div_euclid(86_400)
}

// ── 时区解析 ──────────────────────────────────────────────
//
// Rust 标准库没有 IANA 时区库。这里按诚实近似处理：
// 1. 数值偏移（`+08:00`、`UTC+8`、`Z`）精确解析；
// 2. 常见固定偏移时区（绝大多数为亚洲城市，无夏令时）查表精确匹配；
// 3. 其余（含夏令时时区）回退到**服务进程本地时区**——单机部署里
//    单机使用时，服务进程的本地时区就是用户时区。

/// 常见的、当前规则为固定偏移（无夏令时）的 IANA 时区 → 相对 UTC 的秒数。
/// 夏令时时区（America/New_York 之类）故意不进表，让它们走本地时区回退，
/// 而不是在半年里给出错一小时的"精确"值。
fn fixed_offset_zone(name: &str) -> Option<i64> {
    const H8: i64 = 8 * 3_600;
    const H9: i64 = 9 * 3_600;
    const H7: i64 = 7 * 3_600;
    const H6: i64 = 6 * 3_600;
    const H5: i64 = 5 * 3_600;
    const H4: i64 = 4 * 3_600;
    const H3: i64 = 3 * 3_600;
    const H2: i64 = 2 * 3_600;
    const H1: i64 = 3_600;
    const M3: i64 = -3 * 3_600;
    const M5: i64 = -5 * 3_600;
    const M6: i64 = -6 * 3_600;
    const M7: i64 = -7 * 3_600;
    const M8: i64 = -8 * 3_600;
    const TABLE: &[(&[&str], i64)] = &[
        (
            &[
                "Asia/Shanghai",
                "Asia/Urumqi",
                "Asia/Hong_Kong",
                "Asia/Harbin",
                "Asia/Macau",
                "Asia/Macao",
                "Asia/Taipei",
                "Asia/Manila",
                "Asia/Kuala_Lumpur",
                "Asia/Kuching",
                "Asia/Singapore",
                "Asia/Brunei",
                "Asia/Makassar",
                "Asia/Irkutsk",
                "Australia/Perth",
                "Asia/Choibalsan",
            ],
            H8,
        ),
        (
            &[
                "Asia/Tokyo",
                "Asia/Seoul",
                "Asia/Pyongyang",
                "Asia/Dili",
                "Asia/Jayapura",
                "Pacific/Palau",
                "Asia/Chita",
            ],
            H9,
        ),
        (
            &[
                "Asia/Bangkok",
                "Asia/Ho_Chi_Minh",
                "Asia/Saigon",
                "Asia/Jakarta",
                "Asia/Pontianak",
                "Asia/Phnom_Penh",
                "Asia/Vientiane",
                "Asia/Novosibirsk",
                "Asia/Krasnoyarsk",
                "Asia/Barnaul",
                "Asia/Hovd",
            ],
            H7,
        ),
        (
            &["Asia/Yangon", "Asia/Rangoon", "Asia/Cocos", "Indian/Cocos"],
            H6 + 1_800,
        ),
        (&["Asia/Dhaka", "Asia/Thimphu", "Asia/Thimbu"], H6),
        (
            &["Asia/Kolkata", "Asia/Calcutta", "Asia/Colombo"],
            H5 + 1_800,
        ),
        (&["Asia/Kathmandu", "Asia/Katmandu"], H5 + 2_700),
        (
            &[
                "Asia/Karachi",
                "Asia/Tashkent",
                "Asia/Samarkand",
                "Asia/Yekaterinburg",
                "Asia/Ashgabat",
                "Asia/Dushanbe",
                "Indian/Maldives",
            ],
            H5,
        ),
        (&["Asia/Kabul"], H4 + 1_800),
        (
            &[
                "Asia/Dubai",
                "Asia/Muscat",
                "Asia/Tbilisi",
                "Asia/Yerevan",
                "Asia/Baku",
                "Asia/Oral",
                "Asia/Aqtobe",
                "Asia/Aqtau",
                "Asia/Atyrau",
                "Indian/Mauritius",
                "Indian/Mahe",
                "Indian/Reunion",
                "America/La_Paz",
            ],
            H4,
        ),
        (&["Asia/Tehran"], H3 + 1_800),
        (
            &[
                "Asia/Riyadh",
                "Asia/Qatar",
                "Asia/Bahrain",
                "Asia/Kuwait",
                "Asia/Baghdad",
                "Asia/Aden",
                "Asia/Damascus",
                "Asia/Amman",
                "Asia/Beirut",
                "Asia/Jerusalem",
                "Europe/Moscow",
                "Europe/Istanbul",
                "Europe/Minsk",
                "Europe/Kirov",
                "Europe/Volgograd",
                "Europe/Samara",
                "Africa/Nairobi",
                "Africa/Djibouti",
                "Africa/Asmara",
                "Africa/Mogadishu",
                "Africa/Dar_es_Salaam",
                "Africa/Kampala",
                "Indian/Antananarivo",
                "Indian/Comoro",
                "Indian/Mayotte",
                "America/Sao_Paulo",
            ],
            H3,
        ),
        (
            &[
                "Africa/Johannesburg",
                "Africa/Lusaka",
                "Africa/Harare",
                "Africa/Maputo",
                "Africa/Gaborone",
                "Africa/Kigali",
                "Africa/Lubumbashi",
                "Africa/Tripoli",
                "Europe/Athens",
                "Europe/Bucharest",
                "Europe/Helsinki",
                "Europe/Riga",
                "Europe/Sofia",
                "Europe/Tallinn",
                "Europe/Vilnius",
                "Europe/Nicosia",
                "Asia/Nicosia",
                "Europe/Kiev",
                "Europe/Kyiv",
                "Europe/Uzhgorod",
                "Europe/Zaporozhye",
            ],
            H2,
        ),
        (
            &[
                "Africa/Lagos",
                "Africa/Algiers",
                "Africa/Tunis",
                "Africa/Luanda",
                "Africa/Kinshasa",
                "Africa/Douala",
                "Africa/Porto-Novo",
                "Africa/Niamey",
                "Africa/Ndjamena",
                "Africa/Libreville",
                "Africa/Malabo",
                "Africa/Brazzaville",
                "Africa/Bangui",
                "Europe/Amsterdam",
                "Europe/Berlin",
                "Europe/Paris",
                "Europe/Madrid",
                "Europe/Rome",
                "Europe/Vienna",
                "Europe/Warsaw",
                "Europe/Prague",
                "Europe/Brussels",
                "Europe/Stockholm",
                "Europe/Oslo",
                "Europe/Copenhagen",
                "Europe/Budapest",
                "Europe/Zurich",
                "Europe/Belgrade",
                "Europe/Bratislava",
                "Europe/Ljubljana",
                "Europe/Zagreb",
                "Europe/Sarajevo",
                "Europe/Skopje",
                "Europe/Tirane",
                "Europe/Luxembourg",
                "Europe/Malta",
                "Europe/Monaco",
                "Europe/Andorra",
                "Europe/Gibraltar",
                "Europe/Vaduz",
            ],
            H1,
        ),
        (
            &[
                "America/Argentina/Buenos_Aires",
                "America/Buenos_Aires",
                "America/Montevideo",
                "America/Cayenne",
                "America/Fortaleza",
                "America/Recife",
                "America/Miquelon",
                "America/Paramaribo",
            ],
            M3,
        ),
        (
            &[
                "America/Lima",
                "America/Bogota",
                "America/Panama",
                "America/Jamaica",
                "America/Guayaquil",
                "America/Havana",
                "America/Nassau",
                "America/Toronto",
                "America/Montreal",
                "America/New_York",
                "America/Detroit",
                "America/Iqaluit",
                "America/Port-au-Prince",
                "America/Grand_Turk",
            ],
            M5,
        ),
        (
            &[
                "America/Mexico_City",
                "America/Chicago",
                "America/Winnipeg",
                "America/Regina",
                "America/Guatemala",
                "America/El_Salvador",
                "America/Tegucigalpa",
                "America/Managua",
                "America/Costa_Rica",
                "America/Belize",
            ],
            M6,
        ),
        (
            &[
                "America/Phoenix",
                "America/Denver",
                "America/Edmonton",
                "America/Creston",
                "America/Dawson_Creek",
                "America/Fort_Nelson",
            ],
            M7,
        ),
        (
            &[
                "America/Los_Angeles",
                "America/Vancouver",
                "America/Tijuana",
                "America/Whitehorse",
                "Pacific/Pitcairn",
            ],
            M8,
        ),
        (
            &[
                "Australia/Brisbane",
                "Australia/Lindeman",
                "Pacific/Chuuk",
                "Pacific/Truk",
                "Pacific/Guam",
                "Pacific/Saipan",
                "Pacific/Port_Moresby",
            ],
            H8 + 2 * 3_600,
        ),
        (
            &[
                "Pacific/Wake",
                "Pacific/Tarawa",
                "Pacific/Majuro",
                "Pacific/Kwajalein",
                "Pacific/Nauru",
                "Pacific/Funafuti",
                "Pacific/Wallis",
                "Pacific/Kanton",
                "Pacific/Enderbury",
            ],
            H8 + 4 * 3_600,
        ),
    ];
    TABLE
        .iter()
        .find(|(zones, _)| zones.contains(&name))
        .map(|(_, offset)| *offset)
}

/// 解析请求里的时区为一个偏移秒数。查表不中且非数值形式时，
/// 回退到服务进程本地时区在"现在"的偏移。
#[must_use]
pub fn resolve_tz_offset_seconds(time_zone: Option<&str>) -> i64 {
    let Some(raw) = time_zone.map(str::trim).filter(|tz| !tz.is_empty()) else {
        return local_offset_seconds();
    };
    if matches!(
        raw,
        "UTC" | "GMT" | "Z" | "Etc/UTC" | "Etc/GMT" | "Universal" | "Zulu"
    ) {
        return 0;
    }
    // 数值形式：+08:00 / -0500 / UTC+8 / GMT+5:30
    let numeric = raw
        .strip_prefix("UTC")
        .or_else(|| raw.strip_prefix("GMT"))
        .unwrap_or(raw);
    let negative = numeric.starts_with('-');
    let digits = numeric
        .strip_prefix('+')
        .or_else(|| numeric.strip_prefix('-'))
        .unwrap_or(numeric);
    let (hours, minutes) = if digits.contains(':') {
        let mut parts = digits.split(':');
        let h = parts.next().and_then(|h| h.parse::<i64>().ok());
        let m = parts
            .next()
            .and_then(|m| m.parse::<i64>().ok())
            .unwrap_or(0);
        (h, m)
    } else if digits.len() == 4 {
        // 紧凑 4 位形式（-0500 = -05:00）：前两位小时，后两位分钟。
        (
            digits[..2].parse::<i64>().ok(),
            digits[2..].parse::<i64>().unwrap_or(0),
        )
    } else {
        (digits.parse::<i64>().ok(), 0)
    };
    if let Some(hours) = hours {
        let offset = hours * 3_600 + minutes * 60;
        return if negative { -offset } else { offset };
    }
    if let Some(offset) = fixed_offset_zone(raw) {
        return offset;
    }
    local_offset_seconds()
}

/// 服务进程本地时区在"现在"的偏移秒数。时钟异常时回退 0（UTC）。
#[must_use]
fn local_offset_seconds() -> i64 {
    i64::from(chrono::Local::now().offset().local_minus_utc())
}

// ── 聚合 ──────────────────────────────────────────────────

fn parse_range(range: &str) -> (&'static str, Option<i64>) {
    match range {
        "7d" => ("7d", Some(7)),
        "30d" => ("30d", Some(30)),
        _ => ("all", None),
    }
}

struct DayBucket {
    tokens: u64,
    turns: u64,
}

/// 工具统计的合并来源：全会话计数器 + 有时间戳的事件。
struct ToolRollup {
    calls: u64,
    errors: u64,
    by_name: BTreeMap<String, (u64, u64)>,
    events: Vec<ToolEvent>,
}

impl ToolRollup {
    fn new() -> Self {
        Self {
            calls: 0,
            errors: 0,
            by_name: BTreeMap::new(),
            events: Vec::new(),
        }
    }

    fn absorb_counters(&mut self, tools: &BTreeMap<String, ToolUsageStat>) {
        for (name, stat) in tools {
            self.calls += stat.calls;
            self.errors += stat.errors;
            let entry = self.by_name.entry(name.clone()).or_default();
            entry.0 += stat.calls;
            entry.1 += stat.errors;
        }
    }

    fn absorb_events(&mut self, events: &[ToolEvent]) {
        self.events.extend(events.iter().cloned());
    }

    /// 区间内的按天工具计数：优先用有时间戳的事件（容量截断内）。
    fn calls_on_day(&self, day: i64, offset: i64) -> u64 {
        self.events
            .iter()
            .filter(|event| local_day(event.at_ms, offset) == day)
            .count() as u64
    }
}

/// 汇总所有会话的真实用量为官方前端 `AppUsageSnapshot` 形状的 JSON 值。
#[must_use]
pub fn build_app_usage_snapshot(
    range: &str,
    time_zone: Option<&str>,
    now_ms_value: u64,
    sessions: &[UsageFacts<'_>],
) -> serde_json::Value {
    let (range_name, window_days) = parse_range(range);
    let offset = resolve_tz_offset_seconds(time_zone);
    let today = local_day(now_ms_value, offset);
    let window_start_day = window_days.map(|days| today - (days - 1));
    let window_start_ms =
        window_start_day.map(|day| u64::try_from((day * 86_400 - offset) * 1000).unwrap_or(0));

    let in_window = |at_ms: u64| window_start_ms.is_none_or(|start| at_ms >= start);

    let mut totals = SummaryOut::default();
    let mut days: BTreeMap<i64, DayBucket> = BTreeMap::new();
    let mut daily_models: HashMap<i64, BTreeMap<Option<String>, u64>> = HashMap::new();
    let mut models: BTreeMap<Option<String>, (u64, u64, u64, u64)> = BTreeMap::new();
    let mut tools = ToolRollup::new();
    let mut ttfts: Vec<u64> = Vec::new();
    let mut durations: Vec<u64> = Vec::new();
    let mut session_spans: Vec<u64> = Vec::new();
    let mut attempts: u64 = 0;
    let mut failures: u64 = 0;

    for facts in sessions {
        let turns: Vec<&TurnUsage> = facts
            .turns
            .iter()
            .filter(|turn| in_window(turn.at_ms))
            .collect();
        if turns.is_empty() {
            tools.absorb_counters(facts.tools);
            tools.absorb_events(facts.tool_events);
            continue;
        }
        totals.total_sessions += 1;
        let mut first: Option<u64> = None;
        let mut last: Option<u64> = None;
        for turn in turns {
            attempts += 1;
            if !turn.ok {
                failures += 1;
                continue;
            }
            totals.total_turns += 1;
            totals.total_tokens += turn.total_tokens();
            totals.input_tokens += turn.input_tokens;
            totals.output_tokens += turn.output_tokens;
            totals.reasoning_tokens += turn.reasoning_tokens;
            totals.cache_read_tokens += turn.cached_tokens;
            totals.cache_creation_tokens += turn.cache_creation_tokens;
            if let Some(ttft) = turn.ttft_ms {
                ttfts.push(ttft);
            }
            if let Some(duration) = turn.duration_ms {
                durations.push(duration);
            }
            first = Some(first.map_or(turn.at_ms, |seen: u64| seen.min(turn.at_ms)));
            last = Some(last.map_or(turn.at_ms, |seen: u64| seen.max(turn.at_ms)));

            let day = local_day(turn.at_ms, offset);
            let bucket = days.entry(day).or_insert(DayBucket {
                tokens: 0,
                turns: 0,
            });
            bucket.tokens += turn.total_tokens();
            bucket.turns += 1;
            *daily_models
                .entry(day)
                .or_default()
                .entry(turn.model.clone())
                .or_insert(0) += turn.total_tokens();
            let model_entry = models.entry(turn.model.clone()).or_insert((0, 0, 0, 0));
            model_entry.0 += turn.total_tokens();
            model_entry.1 += turn.input_tokens;
            model_entry.2 += turn.output_tokens;
            model_entry.3 += 1;
        }
        if let (Some(first), Some(last)) = (first, last) {
            session_spans.push(last.saturating_sub(first));
        }
        tools.absorb_counters(facts.tools);
        tools.absorb_events(facts.tool_events);
    }

    // 工具统计：区间内优先用事件；"all" 或事件截断时补上全会话计数器。
    let events_calls_in_window: u64 = tools
        .events
        .iter()
        .filter(|event| in_window(event.at_ms))
        .count() as u64;
    let (tool_call_count, tool_error_count, by_name) = if window_days.is_some() {
        let mut errors = 0u64;
        let mut by_name: BTreeMap<String, (u64, u64)> = BTreeMap::new();
        for event in &tools.events {
            if !in_window(event.at_ms) {
                continue;
            }
            if !event.ok {
                errors += 1;
            }
            let entry = by_name.entry(event.tool.clone()).or_default();
            entry.0 += 1;
            entry.1 += u64::from(!event.ok);
        }
        (events_calls_in_window, errors, by_name)
    } else {
        // "all" 用无界计数器（准确），事件只补按天视图。
        let mut by_name: BTreeMap<String, (u64, u64)> = tools.by_name.clone();
        if by_name.is_empty() {
            for event in &tools.events {
                let entry = by_name.entry(event.tool.clone()).or_default();
                entry.0 += 1;
                entry.1 += u64::from(!event.ok);
            }
        }
        let calls = by_name.values().map(|(calls, _)| *calls).sum();
        let errors = by_name.values().map(|(_, errors)| *errors).sum();
        (calls, errors, by_name)
    };
    totals.tool_call_count = tool_call_count;
    totals.tool_error_rate = rate(tool_error_count, tool_call_count);
    totals.model_error_rate = rate(failures, attempts);
    totals.cache_hit_rate = rate(totals.cache_read_tokens, totals.input_tokens).min(1.0);
    if !ttfts.is_empty() {
        totals.avg_time_to_first_token_ms = Some(ttfts.iter().sum::<u64>() / ttfts.len() as u64);
    }
    if !durations.is_empty() {
        totals.avg_turn_duration_ms = Some(durations.iter().sum::<u64>() / durations.len() as u64);
    }

    totals.active_days = u64::try_from(days.len()).unwrap_or(0);
    totals.longest_session_ms = session_spans.iter().copied().max().unwrap_or(0);
    totals.peak_day_tokens = days.values().map(|bucket| bucket.tokens).max().unwrap_or(0);
    let mut sorted_active: Vec<i64> = days.keys().copied().collect();
    sorted_active.sort_unstable();
    totals.current_streak_days = current_streak(&sorted_active, today);
    totals.longest_streak_days = longest_streak(&sorted_active);

    if let Some((model_id, tokens)) = models
        .iter()
        .max_by_key(|(_, (tokens, _, _, _))| *tokens)
        .map(|(model, (tokens, _, _, _))| (model.clone(), *tokens))
    {
        let share = rate(tokens, totals.input_tokens + totals.output_tokens);
        totals.favorite_model = Some(FavoriteModelOut {
            model_id,
            total_tokens: tokens,
            share,
        });
    }

    // 热力图窗口：从区间起点（或最早有数据的一天，"all" 最多回看一年）到今天。
    let earliest_active = sorted_active.first().copied();
    let heat_start = match (window_start_day, earliest_active) {
        (Some(start), _) => start,
        (None, Some(earliest)) => earliest.max(today - 364),
        (None, None) => today,
    };
    let heatmap = build_heatmap(&days, &tools, heat_start, today, offset);

    let total_all_tokens: u64 = models.values().map(|(tokens, _, _, _)| *tokens).sum();
    let mut model_rows: Vec<ModelUsageOut> = models
        .into_iter()
        .map(
            |(model_id, (tokens, input, output, requests))| ModelUsageOut {
                share: rate(tokens, total_all_tokens),
                model_id,
                total_tokens: tokens,
                input_tokens: input,
                output_tokens: output,
                request_count: requests,
            },
        )
        .collect();
    model_rows.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));

    let mut daily_model_rows: Vec<DailyModelUsageOut> = daily_models
        .into_iter()
        .map(|(day, per_model)| {
            let mut items: Vec<DailyModelItemOut> = per_model
                .into_iter()
                .map(|(model_id, tokens)| DailyModelItemOut {
                    model_id,
                    total_tokens: tokens,
                })
                .collect();
            // 保留每日 token 最大的 8 个模型，其余并入"未知"条目，约束载荷规模。
            items.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens));
            if items.len() > 8 {
                let rest: u64 = items.drain(8..).map(|item| item.total_tokens).sum();
                if rest > 0 {
                    if let Some(unknown) = items.iter_mut().find(|item| item.model_id.is_none()) {
                        unknown.total_tokens += rest;
                    } else {
                        items.push(DailyModelItemOut {
                            model_id: None,
                            total_tokens: rest,
                        });
                    }
                }
            }
            DailyModelUsageOut {
                date: format_day(day),
                models: items,
            }
        })
        .collect();
    daily_model_rows.sort_by(|a, b| a.date.cmp(&b.date));

    let mut tool_rows: Vec<ToolUsageOut> = by_name
        .into_iter()
        .map(|(tool_name, (calls, errors))| ToolUsageOut {
            error_rate: rate(errors, calls),
            tool_name,
            call_count: calls,
            error_count: errors,
            avg_duration_ms: None,
        })
        .collect();
    tool_rows.sort_by(|a, b| b.call_count.cmp(&a.call_count));

    let snapshot = AppUsageSnapshotOut {
        range: range_name.to_string(),
        generated_at: now_ms_value,
        time_zone: time_zone
            .map(str::trim)
            .filter(|tz| !tz.is_empty())
            .unwrap_or("system")
            .to_string(),
        source: "agent-db",
        summary: totals,
        heatmap,
        daily_model_usage: daily_model_rows,
        models: model_rows,
        tools: tool_rows,
    };
    serde_json::to_value(&snapshot)
        .unwrap_or_else(|_| empty_app_usage_snapshot(range, time_zone, now_ms_value))
}

fn rate(numerator: u64, denominator: u64) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        f64::from(u32::try_from(numerator).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(denominator).unwrap_or(u32::MAX))
    }
}

fn current_streak(active_days: &[i64], today: i64) -> u64 {
    if active_days.is_empty() {
        return 0;
    }
    // 今天或昨天开始往回数连续活跃天。
    let anchor = if active_days.contains(&today) {
        today
    } else if active_days.contains(&(today - 1)) {
        today - 1
    } else {
        return 0;
    };
    let mut streak = 0u64;
    let mut cursor = anchor;
    while active_days.contains(&cursor) {
        streak += 1;
        cursor -= 1;
    }
    streak
}

fn longest_streak(active_days: &[i64]) -> u64 {
    let mut longest = 0u64;
    let mut run = 0u64;
    let mut previous: Option<i64> = None;
    for day in active_days {
        run = if previous.is_some_and(|prev| *day == prev + 1) {
            run + 1
        } else {
            1
        };
        longest = longest.max(run);
        previous = Some(*day);
    }
    longest
}

fn build_heatmap(
    days: &BTreeMap<i64, DayBucket>,
    tools: &ToolRollup,
    start_day: i64,
    today: i64,
    offset: i64,
) -> HeatmapOut {
    if start_day > today {
        return HeatmapOut::default();
    }
    let max_tokens = days.values().map(|bucket| bucket.tokens).max().unwrap_or(0);
    // 第一列从周日开始：在窗口前补 null。
    let leading_pad = usize::try_from(weekday_sunday_first(start_day)).unwrap_or(0);
    let total_days = usize::try_from(today - start_day + 1).unwrap_or(0);
    let mut cells: Vec<Option<HeatmapCellOut>> = Vec::with_capacity(leading_pad + total_days);
    for _ in 0..leading_pad {
        cells.push(None);
    }
    for day in start_day..=today {
        let (tokens, turns) = days
            .get(&day)
            .map(|bucket| (bucket.tokens, bucket.turns))
            .unwrap_or((0, 0));
        let tool_calls = tools.calls_on_day(day, offset);
        cells.push(Some(HeatmapCellOut {
            date: format_day(day),
            level: heatmap_level(tokens, max_tokens),
            total_tokens: tokens,
            turn_count: turns,
            tool_call_count: tool_calls,
        }));
    }
    // 最后一周不足 7 天时补 null，保持每周都是完整的 7 格。
    while !cells.len().is_multiple_of(7) {
        cells.push(None);
    }
    let weeks = cells
        .chunks(7)
        .enumerate()
        .map(|(week_index, chunk)| HeatmapWeekOut {
            week_index,
            days: chunk.to_vec(),
        })
        .collect();
    HeatmapOut {
        start_date: Some(format_day(start_day)),
        end_date: Some(format_day(today)),
        max_tokens,
        weeks,
    }
}

/// 与前端 `levelForValue` 同一算法：按相对峰值的四分位取 0–4。
fn heatmap_level(tokens: u64, max_tokens: u64) -> u8 {
    if tokens == 0 || max_tokens == 0 {
        return 0;
    }
    let scaled = (tokens as f64 / max_tokens as f64 * 4.0).ceil();
    scaled.clamp(1.0, 4.0) as u8
}

/// 没有任何数据时的诚实空快照：形状完整，计数全为零。
#[must_use]
pub fn empty_app_usage_snapshot(
    range: &str,
    time_zone: Option<&str>,
    now_ms_value: u64,
) -> serde_json::Value {
    let (range_name, _) = parse_range(range);
    serde_json::json!({
        "range": range_name,
        "generatedAt": now_ms_value,
        "timeZone": time_zone.unwrap_or("system"),
        "source": "agent-db",
        "summary": {
            "totalTokens": 0,
            "inputTokens": 0,
            "outputTokens": 0,
            "reasoningTokens": 0,
            "cacheCreationTokens": 0,
            "cacheReadTokens": 0,
            "cacheHitRate": 0.0,
            "totalSessions": 0,
            "totalTurns": 0,
            "toolCallCount": 0,
            "toolErrorRate": 0.0,
            "modelErrorRate": 0.0,
            "avgTimeToFirstTokenMs": null,
            "avgTurnDurationMs": null,
            "activeDays": 0,
            "currentStreakDays": 0,
            "longestSessionMs": 0,
            "longestStreakDays": 0,
            "peakDayTokens": 0,
            "favoriteModel": null,
        },
        "heatmap": { "startDate": null, "endDate": null, "maxTokens": 0, "weeks": [] },
        "dailyModelUsage": [],
        "models": [],
        "tools": [],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY_MS: u64 = 86_400_000;

    /// 一个"今天 12:00 +08:00"的时间锚点，供各测试换算相对时间。
    fn anchor_ms(year: i64, month: u32, day: u32) -> u64 {
        let days = days_from_civil(year, month, day);
        u64::try_from((days * 86_400 - 8 * 3_600) * 1_000).unwrap() + 12 * 3_600_000
    }

    fn turn(at_ms: u64, model: Option<&str>, input: u64, output: u64) -> TurnUsage {
        TurnUsage {
            at_ms,
            model: model.map(str::to_owned),
            input_tokens: input,
            output_tokens: output,
            ok: true,
            ..TurnUsage::default()
        }
    }

    fn facts(turns: &[TurnUsage]) -> Vec<UsageFacts<'_>> {
        static NO_TOOLS: std::sync::LazyLock<BTreeMap<String, ToolUsageStat>> =
            std::sync::LazyLock::new(BTreeMap::new);
        static NO_EVENTS: std::sync::LazyLock<Vec<ToolEvent>> = std::sync::LazyLock::new(Vec::new);
        vec![UsageFacts {
            turns,
            tools: &NO_TOOLS,
            tool_events: &NO_EVENTS,
        }]
    }

    #[test]
    fn civil_round_trips() {
        for &days in &[-100_000i64, -1, 0, 1, 19_000, 20_650] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days, "round trip for {days}");
        }
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(format_day(0), "1970-01-01");
    }

    #[test]
    fn weekday_matches_known_days() {
        // 1970-01-01 周四(4)，1970-01-04 周日(0)，2024-01-01 周一(1)。
        assert_eq!(weekday_sunday_first(0), 4);
        assert_eq!(weekday_sunday_first(3), 0);
        assert_eq!(weekday_sunday_first(19_723), 1);
    }

    #[test]
    fn numeric_offsets_parse() {
        assert_eq!(resolve_tz_offset_seconds(Some("UTC")), 0);
        assert_eq!(resolve_tz_offset_seconds(Some("Z")), 0);
        assert_eq!(resolve_tz_offset_seconds(Some("+08:00")), 8 * 3_600);
        assert_eq!(resolve_tz_offset_seconds(Some("-0500")), -5 * 3_600);
        assert_eq!(resolve_tz_offset_seconds(Some("UTC+8")), 8 * 3_600);
        assert_eq!(
            resolve_tz_offset_seconds(Some("GMT+5:30")),
            5 * 3_600 + 1_800
        );
    }

    #[test]
    fn fixed_zones_resolve_without_dst() {
        assert_eq!(resolve_tz_offset_seconds(Some("Asia/Shanghai")), 8 * 3_600);
        assert_eq!(resolve_tz_offset_seconds(Some("Asia/Tokyo")), 9 * 3_600);
        assert_eq!(
            resolve_tz_offset_seconds(Some("Asia/Kolkata")),
            5 * 3_600 + 1_800
        );
        assert_eq!(
            resolve_tz_offset_seconds(Some("Asia/Kathmandu")),
            5 * 3_600 + 2_700
        );
        assert_eq!(
            resolve_tz_offset_seconds(Some("Australia/Brisbane")),
            10 * 3_600
        );
    }

    #[test]
    fn unknown_zone_falls_back_to_server_local_offset() {
        // 任何偏移都合法（取决于测试机的本地时区），只要求不 panic 且落在 ±14h。
        let offset = resolve_tz_offset_seconds(Some("Mars/Olympus"));
        assert!(offset.abs() <= 14 * 3_600, "offset {offset} out of range");
    }

    #[test]
    fn empty_snapshot_has_complete_shape() {
        let value = empty_app_usage_snapshot("7d", Some("Asia/Shanghai"), 1_000);
        assert_eq!(value["range"], "7d");
        assert_eq!(value["source"], "agent-db");
        assert_eq!(value["summary"]["totalTokens"], 0);
        assert_eq!(value["summary"]["favoriteModel"], serde_json::Value::Null);
        assert_eq!(value["heatmap"]["weeks"], serde_json::json!([]));
    }

    #[test]
    fn aggregates_tokens_models_and_days() {
        // "今天" = 2026-10-08（Asia/Shanghai）；锚点取当天 12:00。
        let base_ms = anchor_ms(2026, 10, 8);
        let yesterday = base_ms - DAY_MS;
        let turns = [
            turn(base_ms, Some("m-a"), 100, 30),
            turn(yesterday, Some("m-b"), 40, 10),
            turn(yesterday - 9 * DAY_MS, Some("m-a"), 10, 5),
        ];
        let value = build_app_usage_snapshot(
            "all",
            Some("Asia/Shanghai"),
            base_ms + 60_000,
            &facts(&turns),
        );
        let summary = &value["summary"];
        assert_eq!(summary["totalSessions"], 1);
        assert_eq!(summary["totalTurns"], 3);
        assert_eq!(summary["inputTokens"], 150);
        assert_eq!(summary["outputTokens"], 45);
        assert_eq!(summary["totalTokens"], 195);
        assert_eq!(summary["activeDays"], 3);
        assert_eq!(summary["peakDayTokens"], 130);
        assert_eq!(summary["favoriteModel"]["modelId"], "m-a");
        // 最长会话跨度 = 最早轮次到最晚轮次。
        assert_eq!(summary["longestSessionMs"], 10 * DAY_MS);
        let models = value["models"].as_array().expect("models");
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["modelId"], "m-a");
        assert_eq!(models[0]["totalTokens"], 145);
        let daily = value["dailyModelUsage"].as_array().expect("daily");
        assert_eq!(daily.len(), 3);
    }

    #[test]
    fn range_window_limits_turns() {
        let base_ms = anchor_ms(2026, 10, 8);
        let old = base_ms - 20 * DAY_MS;
        let turns = [
            turn(base_ms, Some("m"), 10, 1),
            turn(old, Some("m"), 500, 500),
        ];
        let value = build_app_usage_snapshot(
            "7d",
            Some("Asia/Shanghai"),
            base_ms + 60_000,
            &facts(&turns),
        );
        assert_eq!(value["summary"]["totalTurns"], 1);
        assert_eq!(value["summary"]["totalTokens"], 11);
        assert_eq!(value["heatmap"]["weeks"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn failed_turns_count_as_errors_not_output() {
        let base_ms = anchor_ms(2026, 10, 8);
        let mut failed = turn(base_ms, Some("m"), 0, 0);
        failed.ok = false;
        let turns = [turn(base_ms, Some("m"), 10, 1), failed];
        let value = build_app_usage_snapshot(
            "all",
            Some("Asia/Shanghai"),
            base_ms + 60_000,
            &facts(&turns),
        );
        assert_eq!(value["summary"]["totalTurns"], 1);
        assert_eq!(value["summary"]["modelErrorRate"], 0.5);
    }

    #[test]
    fn tool_rollup_separates_ranged_and_all_time() {
        let base_ms = anchor_ms(2026, 10, 8);
        let mut tools = BTreeMap::new();
        tools.insert(
            "terminal.execute".to_string(),
            ToolUsageStat {
                calls: 12,
                errors: 3,
            },
        );
        let events = vec![ToolEvent {
            at_ms: base_ms,
            tool: "terminal.execute".to_string(),
            ok: false,
        }];
        let turns = [turn(base_ms, Some("m"), 10, 1)];
        let sessions = vec![UsageFacts {
            turns: &turns,
            tools: &tools,
            tool_events: &events,
        }];
        let all =
            build_app_usage_snapshot("all", Some("Asia/Shanghai"), base_ms + 60_000, &sessions);
        assert_eq!(all["summary"]["toolCallCount"], 12);
        assert_eq!(all["tools"][0]["toolName"], "terminal.execute");
        assert_eq!(all["tools"][0]["errorCount"], 3);
        let week =
            build_app_usage_snapshot("7d", Some("Asia/Shanghai"), base_ms + 60_000, &sessions);
        assert_eq!(week["summary"]["toolCallCount"], 1);
        assert_eq!(week["summary"]["toolErrorRate"], 1.0);
    }

    #[test]
    fn streaks_count_consecutive_days() {
        let base_ms = anchor_ms(2026, 10, 8);
        let turns = [
            turn(base_ms, Some("m"), 1, 1),
            turn(base_ms - DAY_MS, Some("m"), 1, 1),
            turn(base_ms - 2 * DAY_MS, Some("m"), 1, 1),
            turn(base_ms - 9 * DAY_MS, Some("m"), 1, 1),
        ];
        let value = build_app_usage_snapshot(
            "all",
            Some("Asia/Shanghai"),
            base_ms + 60_000,
            &facts(&turns),
        );
        assert_eq!(value["summary"]["currentStreakDays"], 3);
        assert_eq!(value["summary"]["longestStreakDays"], 3);
    }

    #[test]
    fn heatmap_cells_reach_today_and_align_to_sunday() {
        let base_ms = anchor_ms(2026, 10, 8);
        let turns = [turn(base_ms, Some("m"), 10, 1)];
        let value = build_app_usage_snapshot(
            "7d",
            Some("Asia/Shanghai"),
            base_ms + 60_000,
            &facts(&turns),
        );
        let weeks = value["heatmap"]["weeks"].as_array().expect("weeks");
        let last_week = weeks.last().expect("last week");
        assert_eq!(last_week["days"].as_array().map(Vec::len), Some(7));
        let end_date = value["heatmap"]["endDate"].as_str().expect("end date");
        assert_eq!(end_date, format_day(local_day(base_ms, 8 * 3_600)));
    }
}
