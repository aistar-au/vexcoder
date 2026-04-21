//! Network utility modules aligned with published protocol specifications.
//!
//! Each sub-module documents the specific RFC it implements and provides
//! a focused, tested API over the underlying third-party crate.

pub mod compression;
pub mod dns;
pub mod http_client;
pub mod jwt;
pub mod oauth;
pub mod proxy;
