//! The GUNBATTE node: one process, two roles (AGENTS.md). The matchmaker
//! (`gunbatte-lobby`) owns people — connections, the queue, lobbies, the ladder.
//! The game-server role (`gunbatte-gameserver`) owns matches. This crate is only
//! the binding between them, plus the binary and the end-to-end tests that
//! pin the whole pipeline as the contract.

pub mod host;

pub use host::GameHost;
