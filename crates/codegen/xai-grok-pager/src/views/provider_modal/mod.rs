//! Provider 管理模态框 —— `/provider` 的交互式 TUI hub。
//!
//! - 裸 `/provider` / `list` — 可点选渠道列表 + 「+ 添加渠道」
//! - Enter 渠道 → 二级操作菜单（编辑 / 设 Key / 看模型 / 手动输入模型 / 切换模型 / 刷新）
//! - Esc 逐级返回；深链子命令仍可直达表单
//! - `/provider add` — 多步表单
//! - `/provider edit` — 编辑已有渠道连接参数
//! - `/provider manual-model` — 手写模型 ID（不依赖上游 /models）
//! - `/provider configure-model` — 为模型设置 max_completion_tokens 等参数
//!
//! 配置读写复用 `slash/commands/provider.rs` 中的函数，
//! 通过 `Action::OpenProviderModal` 触发打开。

mod input;
mod render;
mod state;

pub use input::{handle_provider_key, handle_provider_paste};
// sanitize used by slash/commands/provider commit path
pub(crate) use input::sanitize_provider_field;
pub use render::render_provider_modal;
pub use state::{
    API_BACKENDS, AUTH_SCHEMES, CatPawLoginPhase, FormStep, MODAL_TITLE, ModelParamField,
    ProviderAction, ProviderKeyOutcome, ProviderModalMode, ProviderModalState, ProviderSummary,
};
