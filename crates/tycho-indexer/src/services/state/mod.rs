//! In-memory serving of state requests.
//!
//! This module holds the block window ([`window`]), the entity cache ([`cache`]) it folds into,
//! and the service that answers state requests from both ([`service`]). Locking is described in
//! the [`cache`] module doc; the read order in the [`service`] module doc.
//!
//! # Planned end state
//!
//! The rest of this doc describes the target design (ENG-6293, ENG-6304, ENG-6305). The cache is
//! built at startup by [`cache::EntityCache::load`] when the mode is not `off`, and the windows
//! fold into it. The request routing layer is a skeleton (ENG-6293); every request still reads
//! the database.
//!
//! State responses are built as `cached base ⊕ window deltas up to the requested version`, where
//! `⊕` applies deltas on top of a base and the highest block wins for each value. The database is
//! read only for entities the cache has never seen.
//!
//! A read collects the window patch for its keys first, then reads the entries, and applies the
//! patch changes that are not older than each value's write timestamp — the rule folds use;
//! re-applying a change with an equal timestamp is harmless because delta values are absolute. If
//! an entry has already moved past the requested version (folding is batched), that request is
//! served the way uncommitted versions are served today — latest from the DB plus the buffered
//! window changes — because the cache keeps no history.
//!
//! Versions below the window go to the versioned DB path. Deletions folded into the cache are
//! always below the window floor by then, so a below-window read never misses them.

pub(crate) mod cache;
pub(crate) mod service;
pub(crate) mod window;

/// Which path answers `/contract_state` and `/protocol_state`. Set per deployment with
/// `ENTITY_CACHE_MODE`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum EntityCacheMode {
    /// No cache is loaded; every request reads the database.
    #[default]
    Off,
    /// The cache is loaded at startup and the windows fold into it. Clients get the database
    /// answer; a sample of requests also runs the cache path and compares the two results
    /// (ENG-6295).
    Shadow,
    /// The cache is loaded at startup and the windows fold into it. Clients get the cache answer;
    /// versions the cache cannot rebuild are answered from the database.
    Serve,
}
