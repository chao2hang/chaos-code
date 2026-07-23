//! Provider 管理模态框 —— `/provider` 的交互式 TUI hub。
//!
//! - 裸 `/provider` / `list` — 可点选渠道列表 + 「+ 添加渠道」
//! - Enter 渠道 → 二级操作菜单（设 Key / 看模型 / 切换模型 / 刷新）
//! - Esc 逐级返回；深链子命令仍可直达表单
//! - `/provider add` — 多步表单
//!
//! 配置读写复用 `slash/commands/provider.rs` 中的函数，
//! 通过 `Action::OpenProviderModal` 触发打开。

mod input;
mod render;
mod state;

pub use input::{handle_provider_key, handle_provider_paste};
pub use render::render_provider_modal;
pub use state::{
    MODAL_TITLE, FormStep, ProviderAction, ProviderKeyOutcome, ProviderModalMode,
    ProviderModalState, ProviderSummary,
};
