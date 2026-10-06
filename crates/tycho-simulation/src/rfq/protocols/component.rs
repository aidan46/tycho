use std::{collections::HashMap, time::SystemTime};

use tycho_client::feed::synchronizer::{ComponentWithState, Snapshot, StateSyncMessage};
use tycho_common::models::protocol::ProtocolComponent;

use crate::rfq::{errors::RFQError, models::TimestampHeader};

/// Seconds since the UNIX epoch.
pub fn unix_timestamp() -> Result<u64, RFQError> {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .map_err(|_| RFQError::ParsingError("SystemTime before UNIX EPOCH!".into()))
}

/// The stream message for one poll: every component in `components`, and the removal of every
/// component the stream emitted before that `components` lacks.
pub fn poll_message(
    current: &mut HashMap<String, ComponentWithState>,
    components: HashMap<String, ComponentWithState>,
    timestamp: u64,
) -> StateSyncMessage<TimestampHeader> {
    let mut removed_components = HashMap::new();
    for (id, component) in current.iter() {
        if !components.contains_key(id) {
            removed_components.insert(id.clone(), component.component.clone());
        }
    }
    *current = components.clone();
    sync_message(components, removed_components, timestamp)
}

fn sync_message(
    states: HashMap<String, ComponentWithState>,
    removed_components: HashMap<String, ProtocolComponent>,
    timestamp: u64,
) -> StateSyncMessage<TimestampHeader> {
    StateSyncMessage {
        header: TimestampHeader { timestamp },
        snapshots: Snapshot { states, vm_storage: HashMap::new() },
        deltas: None,
        removed_components,
    }
}
