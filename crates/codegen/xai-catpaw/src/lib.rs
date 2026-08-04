//! Native CatPaw client core.
//!
//! This crate contains only the protocol/client layer: upstream endpoint and
//! header definitions, request/response cryptography, login normalization,
//! model discovery, Chat and Remote Agent stream accumulation, a small HTTP
//! client, and encrypted local account storage. It intentionally contains no
//! Axum gateway or web UI.

pub mod agent;
pub mod chat;
pub mod client;
pub mod crypto;
pub mod endpoints;
pub mod error;
pub mod headers;
pub mod models;
pub mod qr;
pub mod store;
pub mod tokens;

pub use client::Client;
pub use error::{Error, Result};
pub use store::AccountStore;
