//! Shared Kubernetes naming and ownership constants.
//!
//! The literals live here once so a future rename only touches this file.
//! One exception: the `group = "fluss.datalush.com"` inside the `#[kube]`
//! macro in `api.rs` must stay a literal because attribute macros cannot
//! read constants. It must always match [`GROUP`].

/// API group of the FlussCluster CRD. Must match the `#[kube(group = ...)]`
/// literal in `api.rs`, which cannot reference this constant.
#[allow(dead_code)]
pub const GROUP: &str = "fluss.datalush.com";

/// Full apiVersion of the FlussCluster CRD, used in owner references.
#[allow(dead_code)]
pub const API_VERSION: &str = "fluss.datalush.com/v1alpha1";

/// Kind of the FlussCluster CRD, used in owner references.
#[allow(dead_code)]
pub const KIND_FLUSS_CLUSTER: &str = "FlussCluster";

/// Label identifying which FlussCluster owns a resource.
pub const LABEL_CLUSTER: &str = "fluss.datalush.com/cluster";

/// Label identifying the Fluss role (coordinator, tablet-server) of a resource.
pub const LABEL_ROLE: &str = "fluss.datalush.com/role";

/// Value of [`LABEL_ROLE`] for Coordinator resources.
pub const ROLE_COORDINATOR: &str = "coordinator";

/// Value of [`LABEL_ROLE`] for TabletServer resources.
pub const ROLE_TABLET: &str = "tabletserver";

/// Suffix appended to the FlussCluster name for the Coordinator headless Service.
pub const COORDINATOR_HEADLESS_SUFFIX: &str = "-coordinator-headless";

/// Suffix appended to the FlussCluster name for the Coordinator ConfigMap.
pub const COORDINATOR_CONFIG_SUFFIX: &str = "-coordinator-config";

/// Suffix appended to the FlussCluster name for the TabletServer ConfigMap.
pub const TABLET_CONFIG_SUFFIX: &str = "-tabletserver-config";

/// Suffix appended to the FlussCluster name for the Coordinator StatefulSet.
pub const COORDINATOR_STATEFULSET_SUFFIX: &str = "-coordinator";

/// Suffix appended to the FlussCluster name for the TabletServer StatefulSet.
pub const TABLET_STATEFULSET_SUFFIX: &str = "-tabletserver";

/// Suffix for the TabletServer headless Service backing StatefulSet DNS.
///
/// Rendered minimally by the operator so per-pod DNS resolves; the shared
/// client Service is separate 7vp7 scope.
pub const TABLET_HEADLESS_SUFFIX: &str = "-tabletserver-headless";

/// Suffix appended to the FlussCluster name for the TabletServer
/// PodDisruptionBudget.
pub const TABLET_PDB_SUFFIX: &str = "-tabletserver-pdb";

/// Suffix appended to the FlussCluster name for the Coordinator
/// PodDisruptionBudget.
pub const COORDINATOR_PDB_SUFFIX: &str = "-coordinator-pdb";

/// FIP-41 safe default: TabletServer evictions are blocked unless the CR
/// explicitly allows disruption. Applies when the CR sets no
/// `podDisruptionBudget` at all; an explicit `enabled: false` removes the
/// budget instead.
pub const DEFAULT_TABLET_MAX_UNAVAILABLE: i32 = 0;

/// Name of the internal Fluss listener port on generated Services.
pub const PORT_NAME_INTERNAL: &str = "internal";

/// Name of the client Fluss listener port on generated Services and pods.
pub const PORT_NAME_CLIENT: &str = "client";

/// Suffix appended to the FlussCluster name for the shared client Service.
///
/// Single Service for both roles: the CR models one client listener, and
/// Fluss clients bootstrap from any endpoint (coordinator metadata, tablet
/// data). In-cluster only; external exposure is rejected by the API.
pub const CLIENT_SERVICE_SUFFIX: &str = "-client";

/// Key inside the generated ConfigMaps holding the rendered `server.yaml`.
pub const CONFIG_DATA_KEY: &str = "server.yaml";

/// Annotation on the generated ConfigMaps holding the `sha256:<hex>` hash
/// of their own rendered `server.yaml`.
///
/// Future StatefulSets key their pod-template rollout on this annotation;
/// the cluster-level [`crate::api::FlussClusterStatus::observed_config_hash`]
/// hashes both documents instead.
pub const CONFIG_HASH_ANNOTATION: &str = "fluss.datalush.com/config-hash";

/// Default tablet data directory when the CR sets no `storage.data_dir`.
///
/// Shared with the future StatefulSet: its data volume must mount exactly
/// here, so the value lives centrally rather than in `server_config`.
pub const DEFAULT_DATA_DIR: &str = "/tmp/fluss/data";

/// Directory inside the pod where the S3 Secret is mounted for secret auth.
///
/// The rendered `server.yaml` points its `${directory:...}` markers here;
/// the StatefulSet builder mounts exactly this path, so both sides share
/// the constant instead of a duplicated literal.
pub const S3_SECRETS_DIR: &str = "/etc/fluss/secrets/s3";

/// File names inside [`S3_SECRETS_DIR`] holding the static credentials.
pub const S3_ACCESS_KEY_FILE: &str = "access-key";
pub const S3_SECRET_KEY_FILE: &str = "secret-key";
