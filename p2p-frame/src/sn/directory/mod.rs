use std::sync::Arc;
use std::time::Duration;

pub mod client;
mod control_plane;
mod election;
mod local;
mod membership;
mod route;
pub mod server;
mod store;

pub use client::{
    NoopOwnerDirectoryClient, OwnerDirectoryClient, StaticOwnerDirectoryClient,
    noop_owner_directory_client,
};
pub use control_plane::{
    OwnerControlPlane, OwnerControlPlaneRef, OwnerSessionEntry, ServingSnSession,
    ServingSnSessionState,
};
pub use election::{
    OwnerElectionNode, OwnerElectionNodeRef, OwnerElectionRole, OwnerHeartbeatRequest,
    OwnerHeartbeatResponse, OwnerPeerControlClient, OwnerPeerControlClientRef, OwnerSessionForward,
    OwnerSessionForwardResponse, OwnerSessionReplication, OwnerSessionReplicationResponse,
    OwnerVoteRequest, OwnerVoteResponse,
};
pub use local::{
    LocalOwnerDirectory, LocalOwnerDirectoryRef, LocalSnDirectory, LocalSnDirectoryRef,
};
pub use membership::{
    OwnerMember, OwnerMemberHealth, OwnerMemberHealthRef, OwnerMembership, OwnerResolver,
    ServingLease,
};
pub use route::{PeerRoute, PeerRouteStore, PeerRouteStoreRef};
pub use server::{OwnerDirectoryListenConfig, OwnerDirectoryServer, OwnerDirectoryService};
pub use store::{OwnerDirectoryStore, OwnerDirectoryStoreRef};

pub type OwnerDirectoryClientRef = Arc<dyn OwnerDirectoryClient>;
pub type OwnerDirectoryServerRef = Arc<OwnerDirectoryServer>;
pub type OwnerDirectoryServiceRef = Arc<OwnerDirectoryService>;

const DEFAULT_OWNER_COUNT: usize = 1;
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(300);
const DEFAULT_OWNER_MEMBER_HEALTH_TTL: Duration = Duration::from_secs(60);
const DEFAULT_SERVING_SESSION_TTL: Duration = Duration::from_secs(300);
