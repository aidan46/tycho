//! Serves `/contract_state` and `/protocol_state` from the entity cache.
//!
//! A response is `cached entry ⊕ window changes up to the requested version`. The service never
//! reads the database. A request it cannot serve fails with [`StateServiceError::VersionTooOld`],
//! and the RPC handler answers it with today's code, which stays untouched: it is both the
//! fallback and the instant rollback (`ENTITY_CACHE_MODE=off`).
//!
//! # Read order
//!
//! A read holds the window lock while it resolves the version and captures the patch, releases
//! it, then takes the cache read lock and copies the entries out. Folds hold the window lock
//! while they take the cache write lock, so a fold can land between the two steps. That is
//! harmless: the fold moves blocks from the patch into the entries, and a patch change applies
//! only when it is newer than the value's write timestamp, so nothing is applied twice or lost.
//! A fold can also carry an entry past the requested version; the request then fails with
//! [`StateServiceError::VersionTooOld`].
//!
//! # Versions the cache cannot rebuild
//!
//! The cache keeps only the newest value of each entry, so it cannot serve a version older than
//! a value it holds. The version is then below the window, or it was inside the window and a
//! value is newer anyway: a fold landed after the version was resolved (which also moves the
//! version below the window), or another extractor that shares the entity is ahead of this one.
//! Today's handler answers every such version: from the versioned query when the database holds
//! it, otherwise as `latest from the DB ⊕ uncommitted window changes`.

// Constructed once the startup load builds the cache (ENG-6292).
#![allow(dead_code)]

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use thiserror::Error;
use tycho_common::dto;

use super::{cache::EntityCache, window::DeltaWindow};
use crate::services::rpc::RpcError;

/// Which path answers state requests, holding what the cache modes need: the loaded
/// [`EntityCache`] when building the services, the [`StateService`] once built.
///
/// A cache mode without a cache, or `Off` with one, cannot be expressed.
#[derive(Clone, Debug)]
pub enum EntityCacheSetup<T> {
    /// See [`EntityCacheMode::Off`](super::EntityCacheMode::Off).
    Off,
    /// See [`EntityCacheMode::Shadow`](super::EntityCacheMode::Shadow).
    Shadow(T),
    /// See [`EntityCacheMode::Serve`](super::EntityCacheMode::Serve).
    Serve(T),
}

impl<T> EntityCacheSetup<T> {
    /// The value a cache mode holds; `None` for `Off`.
    pub fn cache(&self) -> Option<&T> {
        match self {
            EntityCacheSetup::Off => None,
            EntityCacheSetup::Shadow(cache) | EntityCacheSetup::Serve(cache) => Some(cache),
        }
    }

    /// Replaces the held value, keeping the mode.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> EntityCacheSetup<U> {
        match self {
            EntityCacheSetup::Off => EntityCacheSetup::Off,
            EntityCacheSetup::Shadow(cache) => EntityCacheSetup::Shadow(f(cache)),
            EntityCacheSetup::Serve(cache) => EntityCacheSetup::Serve(f(cache)),
        }
    }
}

/// Why the state service did not answer a request.
#[derive(Debug, Error)]
pub(crate) enum StateServiceError {
    /// The cache cannot rebuild the requested version; the database path answers it instead.
    /// See the module doc for when this happens.
    #[error("Requested version is older than the entity cache")]
    VersionTooOld,
    /// The request is invalid; the client gets this error.
    #[error(transparent)]
    Rpc(#[from] RpcError),
}

/// Answers state requests from the delta windows and the entity cache. Never reads the database.
pub(crate) struct StateService {
    /// One window per protocol system, shared with the pump that writes them.
    windows: HashMap<String, Arc<Mutex<DeltaWindow>>>,
    cache: Arc<EntityCache>,
}

impl StateService {
    pub(crate) fn new(
        windows: HashMap<String, Arc<Mutex<DeltaWindow>>>,
        cache: Arc<EntityCache>,
    ) -> Self {
        Self { windows, cache }
    }

    /// Serves `/contract_state` from the cache.
    ///
    /// Paginates `contract_ids` the way the database path does (slice, then page) and reports
    /// `total` as the number of requested ids. Addresses that exist nowhere are omitted.
    ///
    /// # Errors
    ///
    /// [`StateServiceError::VersionTooOld`] when the cache cannot rebuild the version. Otherwise
    /// [`StateServiceError::Rpc`] with:
    ///
    /// - `RpcError::Parse` (400) when `contract_ids` is `None`: the cache serves explicit ids only.
    /// - `RpcError::Parse` (400) when `protocol_system` is empty or has no window. Today this
    ///   silently reads the database.
    /// - `RpcError::Storage(StorageError::NotFound("Block", ..))` when the version is a block
    ///   number above the tip ([`WindowResolution::AboveTip`]). tycho-client retries a body that
    ///   contains `"Could not find Block"` and may blacklist a component on any other text, so the
    ///   entity name must be `Block`. Today's `calculate_versions` says `Version` here, which the
    ///   client does not match; do not copy it.
    ///
    /// [`WindowResolution::AboveTip`]: super::window::WindowResolution::AboveTip
    pub(crate) fn contract_state(
        &self,
        request: &dto::StateRequestBody,
    ) -> Result<dto::StateRequestResponse, StateServiceError> {
        let _ = request;
        todo!("ENG-6307: resolve + capture under the window lock, then read the cache")
    }

    /// Serves `/protocol_state` from the cache.
    ///
    /// Same shape as [`Self::contract_state`]. Components are looked up under
    /// `request.protocol_system`, the key folds use. Deleted attributes stay deleted. With
    /// `include_balances == false` the balances are removed from the response, like the
    /// database path.
    ///
    /// # Errors
    ///
    /// Same as [`Self::contract_state`], with `protocol_ids` in place of `contract_ids`.
    pub(crate) fn protocol_state(
        &self,
        request: &dto::ProtocolStateRequestBody,
    ) -> Result<dto::ProtocolStateRequestResponse, StateServiceError> {
        let _ = request;
        todo!("ENG-6308")
    }
}
