//! `/client` modal: choose the request-client identity for the current
//! conversation and manage user-defined profiles.

mod input;
mod render;
mod state;

pub use input::{handle_client_key, handle_client_paste};
pub use render::render_client_modal;
pub use state::{
    ClientFormField, ClientKeyOutcome, ClientModalMode, ClientModalState, MODAL_TITLE,
};
