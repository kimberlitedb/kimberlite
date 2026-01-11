//! vdb-directory: Placement routing for VerityDB
//!
//! The directory determines which VSR group handles a given stream
//! based on its placement policy (regional PHI vs global non-PHI).
//!
//! Key function: group_for_stream(stream_metadata) -> GroupId

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use vdb_types::{GroupId, Placement, Region};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Directory {
    global_group: GroupId,
    regional_groups: HashMap<Region, GroupId>,
}

impl Directory {
    pub fn new(global_group: GroupId) -> Self {
        Self {
            global_group,
            regional_groups: HashMap::new(),
        }
    }

    pub fn with_region(mut self, region: Region, group: GroupId) -> Self {
        self.regional_groups.insert(region, group);

        self
    }

    pub fn group_for_placement(&self, placement: &Placement) -> Result<GroupId, DirectoryError> {
        match placement {
            Placement::Region(region) => self
                .regional_groups
                .get(region)
                .copied()
                .ok_or_else(|| DirectoryError::RegionNotFound(region.clone())),
            Placement::Global => Ok(self.global_group),
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum DirectoryError {
    #[error("region not found: {0}")]
    RegionNotFound(Region),
}

#[cfg(test)]
mod tests;
