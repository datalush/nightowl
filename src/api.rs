// SPDX-License-Identifier: AGPL-3.0-only
use std::collections::BTreeMap;
use std::sync::OnceLock;

use k8s_openapi::api::core::v1::{
    Affinity, PodSecurityContext, Toleration, TopologySpreadConstraint,
};
use kube::CustomResource;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(CustomResource, Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[kube(
    group = "fluss.datalush.com",
    version = "v1alpha1",
    kind = "FlussCluster",
    plural = "flussclusters",
    namespaced,
    status = "FlussClusterStatus"
)]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "!has(self.defaults) || self.defaults.logReplicationFactor <= self.tabletServers.replicas",
    "message": "logReplicationFactor must not exceed tabletServers replicas"
}, {
    "rule": "!has(oldSelf.listeners) || !has(oldSelf.listeners.external) || (has(self.listeners) && has(self.listeners.external))",
    "message": "public native access cannot be removed from a running FlussCluster"
}, {
    "rule": "!has(self.security) || !has(self.gateway) || !self.gateway.enabled",
    "message": "the HTTP Gateway cannot share the native per-user ACL listener"
}]))]
#[serde(rename_all = "camelCase")]
pub struct FlussClusterSpec {
    pub version: String,
    pub image: ImageSpec,
    pub zookeeper: ZookeeperSpec,
    pub coordinator: CoordinatorSpec,
    pub tablet_servers: TabletServersSpec,
    pub remote_storage: RemoteStorageSpec,
    /// Native users, authenticated by Fluss and authorized by its ACLs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security: Option<NativeSecuritySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listeners: Option<ListenersSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_disruption_budget: Option<PodDisruptionBudgetSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_upgrade: Option<RollingUpgradeSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scale_in: Option<ScaleInSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub defaults: Option<TableDefaultsSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observability: Option<ObservabilitySpec>,
    /// Optional Gateway: absent or disabled means nothing is deployed.
    /// Opt-in only — a trust-mode HTTP write endpoint is never raised
    /// unless asked for explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewaySpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configuration_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NativeSecuritySpec {
    pub sasl_plain: SaslPlainSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaslPlainSpec {
    /// Secret in the FlussCluster namespace; key `credentials` holds the user:password map.
    pub credentials_secret_name: String,
    /// One superuser used to administer ACLs; credentials come from the same Secret.
    pub admin_user: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ImageSpec {
    /// The version in FlussClusterSpec supplies the image tag.
    pub repository: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pull_policy: Option<ImagePullPolicy>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pull_secrets: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub enum ImagePullPolicy {
    Always,
    IfNotPresent,
    Never,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ZookeeperSpec {
    /// Rendered as the comma-separated zookeeper.address server property.
    #[schemars(length(min = 1))]
    pub addresses: Vec<String>,
    /// If absent, derive a stable root from the FlussCluster namespace and name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_root: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorSpec {
    #[schemars(range(min = 1))]
    pub replicas: i32,
    pub resources: ComponentResourcesSpec,
    /// An image override for this component; compatibility with spec.version
    /// must be checked before any upgrade is performed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm: Option<JvmSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage: Option<StorageSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<SchedulingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_template: Option<PodTemplateSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configuration_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabletServersSpec {
    #[schemars(range(min = 1))]
    pub replicas: i32,
    pub resources: ComponentResourcesSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jvm: Option<JvmSpec>,
    pub storage: StorageSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scheduling: Option<SchedulingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pod_template: Option<PodTemplateSpec>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configuration_overrides: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewaySpec {
    /// Master switch, default off. `false` (or absent section) deploys
    /// nothing and garbage-collects previously managed Gateway objects.
    #[serde(default)]
    pub enabled: bool,
    /// Stateless replicas; 1 suffices to start, scaling is linear.
    #[serde(default = "default_gateway_replicas")]
    #[schemars(range(min = 1))]
    pub replicas: i32,
    /// Gateway image; defaults to the Fluss release mate
    /// (`apache/fluss-gateway:<spec.version>`). Declared by the user so
    /// Gateway and server releases stay decoupled — no version matrix
    /// is hardcoded in the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Optional exterior door. The operator creates the Ingress object
    /// only; TLS secret, DNS and auth live in the environment and must
    /// already exist — otherwise the step refuses with evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ingress: Option<GatewayIngressSpec>,
}

/// Default Gateway replicas: one stateless instance.
fn default_gateway_replicas() -> i32 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GatewayIngressSpec {
    /// Public hostname, e.g. `gateway.example.com`. The pattern itself
    /// is user data, never hardcoded in the operator.
    pub host: String,
    /// Ingress class of the environment (e.g. `traefik`); passed through
    /// verbatim, never validated against cluster state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    /// Existing TLS Secret in the FlussCluster namespace. Checked before
    /// creating the Ingress; a missing secret refuses with evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tls_secret_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct JvmSpec {
    pub heap: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ComponentResourcesSpec {
    pub requests: CpuMemorySpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limits: Option<CpuMemorySpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct CpuMemorySpec {
    pub cpu: String,
    pub memory: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct StorageSpec {
    pub size: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub storage_class_name: Option<String>,
    /// The TabletServer data directory inside the container, when overridden.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_dir: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SchedulingSpec {
    #[serde(default)]
    pub spread_across_nodes: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub node_selector: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affinity: Option<Affinity>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tolerations: Vec<Toleration>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topology_spread_constraints: Vec<TopologySpreadConstraint>,
}

/// Pod customization without duplicating the placement settings in scheduling.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodTemplateSpec {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_context: Option<PodSecurityContext>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListenersSpec {
    #[serde(default = "default_internal_listener")]
    pub internal: ListenerSpec,
    #[serde(default = "default_client_listener")]
    pub client: ClientListenerSpec,
    /// Public native listener behind SNI routing and per-server TLS sidecars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external: Option<ExternalListenerSpec>,
}

static DEFAULT_LISTENERS: OnceLock<ListenersSpec> = OnceLock::new();

impl FlussClusterSpec {
    /// Existing CRs may omit listeners; resolve them without changing their stored spec.
    pub fn resolved_listeners(&self) -> &ListenersSpec {
        self.listeners
            .as_ref()
            .unwrap_or_else(|| DEFAULT_LISTENERS.get_or_init(ListenersSpec::default))
    }
}

impl Default for ListenersSpec {
    fn default() -> Self {
        Self {
            internal: default_internal_listener(),
            client: default_client_listener(),
            external: None,
        }
    }
}

fn default_internal_listener() -> ListenerSpec {
    ListenerSpec {
        name: "INTERNAL".into(),
        port: 9123,
    }
}

fn default_client_listener() -> ClientListenerSpec {
    ClientListenerSpec {
        name: "CLIENT".into(),
        port: 9124,
        service_type: ClientServiceType::ClusterIP,
        annotations: BTreeMap::new(),
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalListenerSpec {
    /// Advanced: override the generated Fluss listener name (EXTERNAL).
    #[serde(default = "default_external_listener_name")]
    pub name: String,
    /// Advanced: override the Fluss pod-side listener port (9125).
    #[serde(default = "default_external_listener_port")]
    #[schemars(range(min = 1, max = 65535))]
    pub port: i32,
    /// Public DNS suffix; server names are generated from this value.
    #[schemars(
        length(min = 1, max = 253),
        regex(pattern = "^[A-Za-z0-9]([A-Za-z0-9.-]*[A-Za-z0-9])?$")
    )]
    pub domain: String,
    pub gateway: ExternalGatewaySpec,
    pub tls: ExternalTlsSpec,
    #[serde(default)]
    pub dns: ExternalDnsSpec,
    /// Advanced: override the public port (443).
    #[serde(default = "default_external_public_port")]
    #[schemars(range(min = 1, max = 65535))]
    pub public_port: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalGatewaySpec {
    pub class_name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalTlsSpec {
    pub secret_name: String,
    /// Override the operator-tested Envoy sidecar image.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ExternalDnsSpec {
    #[serde(default)]
    pub internal_mapping: bool,
}

fn default_external_listener_name() -> String {
    "EXTERNAL".into()
}

fn default_external_listener_port() -> i32 {
    9125
}

fn default_external_public_port() -> i32 {
    443
}

/// Configured external mapping with a reconciled Service, not a reachability claim.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalEndpointStatus {
    pub pod: String,
    pub service: String,
    pub address: String,
    pub role: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ListenerSpec {
    pub name: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ClientListenerSpec {
    pub name: String,
    #[schemars(range(min = 1, max = 65535))]
    pub port: i32,
    /// The CLIENT Service stays internal; external access uses its own listener.
    pub service_type: ClientServiceType,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub enum ClientServiceType {
    ClusterIP,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct PodDisruptionBudgetSpec {
    pub tablet_servers: TabletDisruptionBudgetSpec,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<CoordinatorDisruptionBudgetSpec>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct TabletDisruptionBudgetSpec {
    pub enabled: bool,
    #[schemars(range(min = 0))]
    pub max_unavailable: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorDisruptionBudgetSpec {
    pub enabled: bool,
    #[schemars(range(min = 1))]
    pub min_available: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct RollingUpgradeSpec {
    pub controlled_shutdown_timeout: String,
    pub recovery_timeout: String,
    pub stabilization_window: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScaleInSpec {
    pub on_non_empty_tablet_server: ScaleInPolicy,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub enum ScaleInPolicy {
    Block,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct RemoteStorageSpec {
    pub s3: S3StorageSpec,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "self.authentication.type != 'workloadIdentity' || (has(self.delegation) && self.delegation.type == 'assumeRole')",
    "message": "workloadIdentity requires assumeRole delegation"
}]))]
#[serde(rename_all = "camelCase")]
pub struct S3StorageSpec {
    pub bucket: String,
    /// A dedicated prefix for this FlussCluster; it must not be shared.
    pub prefix: String,
    pub region: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default)]
    pub path_style_access: bool,
    #[schemars(with = "S3AuthenticationSchema")]
    pub authentication: S3AuthenticationSpec,
    /// Required for workload identity in Fluss 1.0; with static keys the
    /// server uses GetSessionToken unless AssumeRole is configured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(with = "Option<S3DelegationSchema>")]
    pub delegation: Option<S3DelegationSpec>,
}

// Kubernetes structural CRD schemas cannot hoist different definitions of
// `type` from a tagged enum's variants. These shapes keep the wire format and
// discriminator enum while Serde's real types reject invalid combinations.
#[allow(dead_code)]
#[derive(Deserialize, Serialize, JsonSchema)]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "self.type == 'secret' ? has(self.secretRef) && !has(self.serviceAccountName) : has(self.serviceAccountName) && !has(self.secretRef)",
    "message": "secret requires secretRef; workloadIdentity requires serviceAccountName"
}]))]
#[serde(rename_all = "camelCase")]
struct S3AuthenticationSchema {
    #[serde(rename = "type")]
    kind: S3AuthenticationType,
    service_account_name: Option<String>,
    secret_ref: Option<S3SecretRef>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum S3AuthenticationType {
    WorkloadIdentity,
    Secret,
}

#[allow(dead_code)]
#[derive(Deserialize, Serialize, JsonSchema)]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "self.type == 'assumeRole' ? has(self.roleArn) : !has(self.roleArn) && !has(self.stsEndpoint)",
    "message": "assumeRole requires roleArn; getSessionToken cannot set roleArn or stsEndpoint"
}]))]
#[serde(rename_all = "camelCase")]
struct S3DelegationSchema {
    #[serde(rename = "type")]
    kind: S3DelegationType,
    role_arn: Option<String>,
    sts_endpoint: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
enum S3DelegationType {
    GetSessionToken,
    AssumeRole,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum S3AuthenticationSpec {
    /// An existing ServiceAccount configured for IRSA or EKS Pod Identity.
    WorkloadIdentity { service_account_name: String },
    /// A Secret in the same namespace as the FlussCluster.
    Secret { secret_ref: S3SecretRef },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct S3SecretRef {
    pub name: String,
    pub access_key_key: String,
    pub secret_key_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum S3DelegationSpec {
    GetSessionToken,
    AssumeRole {
        role_arn: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sts_endpoint: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
#[schemars(extend("x-kubernetes-validations" = [{
    "rule": "!has(self.minInSyncReplicas) || self.minInSyncReplicas <= self.logReplicationFactor",
    "message": "minInSyncReplicas must not exceed logReplicationFactor"
}]))]
#[serde(rename_all = "camelCase")]
pub struct TableDefaultsSpec {
    /// Default table buckets (sharding) for new tables — unrelated to any
    /// S3 bucket. Named explicitly after the S3/table-bucket confusion.
    #[schemars(range(min = 1))]
    pub table_buckets: i32,
    #[schemars(range(min = 1))]
    pub log_replication_factor: i32,
    #[schemars(range(min = 1))]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_in_sync_replicas: Option<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObservabilitySpec {
    /// Prometheus scrape wiring, default-on. `false` opts out: no reporter
    /// key is rendered and no scrape annotations are injected.
    #[serde(default = "default_true")]
    #[schemars(default = "default_true")]
    pub prometheus: bool,
}

/// Serde/schemars default fn: observability is on unless opted out.
fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlussClusterStatus {
    /// Configured mappings whose Services converged in this reconciliation.
    /// External routes and network reachability are managed and verified separately.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub external_endpoints: Vec<ExternalEndpointStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_generation: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_config_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cluster_health: Option<ClusterHealthStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub coordinator_endpoints: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coordinator: Option<CoordinatorStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tablet_servers: Option<TabletServersStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conditions: Vec<FlussClusterCondition>,
    /// Dynamically applied config as key to value-hash: records what Admin
    /// already holds, so restarts and replays stay idempotent. Hashes only —
    /// the allowlist holds credential-bearing keys.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub applied_dynamic_config: BTreeMap<String, String>,
    /// Gateway presence, or absent when not requested. Readiness comes from
    /// the Deployment (kubelet gates pods on the Gateway's own `/ready`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayStatus>,
    /// Admin-rejected dynamic keys escalated to restart-bound. Cleared for
    /// a key once a restart was attempted for it (`restart_attempted_keys`)
    /// or Admin finally applies it — so a persistently rejected key reports
    /// instead of restart-looping.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restart_required_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub restart_attempted_keys: Vec<String>,
    /// Sequenced-restart progress, or absent when no restart is running.
    /// Persists across operator restarts so a new instance resumes instead
    /// of duplicating pod deletions. Cleared on completion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart_seq: Option<RestartSeq>,
}

/// One sequenced-restart run: tablets tail-first, then the coordinator.
#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RestartSeq {
    /// Combined config hash the run drives all pods to.
    pub target_hash: String,
    /// True when the run exists only for restart-bound keys (pod hashes
    /// already match; completion moves them to attempted).
    #[serde(default)]
    pub for_keys: bool,
    /// Tablet ordinals already deleted and verified new.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done_tablet_ordinals: Vec<i32>,
    /// Coordinator ordinals already deleted and verified new.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub done_coordinator_ordinals: Vec<i32>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CoordinatorStatus {
    pub desired: i32,
    pub ready: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_pod: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct TabletServersStatus {
    pub desired: i32,
    pub ready: i32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pods: Vec<TabletServerPodStatus>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatus {
    pub desired: i32,
    pub ready: i32,
    /// In-cluster URL clients use; the external hostname (if any) lives
    /// on the Ingress object, not duplicated here.
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TabletServerPodStatus {
    pub name: String,
    pub ready: bool,
    /// Requires Fluss's proposed per-server Admin API; unknown until observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_tablets: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replica_health: Option<ReplicaHealth>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub struct ClusterHealthStatus {
    pub status: ClusterHealthState,
    #[serde(flatten)]
    pub replicas: ReplicaHealth,
    /// Present only when the server reports persistent recovery evidence.
    #[serde(
        rename = "dataAtRisk",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub data_at_risk: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReplicaHealth {
    pub num_replicas: i32,
    pub in_sync_replicas: i32,
    pub num_leader_replicas: i32,
    pub active_leader_replicas: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ClusterHealthState {
    Green,
    Yellow,
    Red,
    Unknown,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlussClusterCondition {
    #[serde(rename = "type")]
    pub condition_type: FlussConditionType,
    pub status: ConditionStatus,
    pub reason: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<String>,
    pub last_transition_time: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum FlussConditionType {
    Ready,
    Progressing,
    Upgrading,
    Stalled,
    Degraded,
    /// Data with no live replica: RED health with tablets registered but
    /// zero active leaders. Observe-and-report only: the operator retries
    /// nothing and moves no bytes; recovery needs a server-side restore
    /// primitive Fluss 1.0 does not provide.
    DataAtRisk,
    Adoptable,
    KubernetesResourcesReady,
    /// Gateway API accepted and programmed every native SNI route (not an external dial test).
    NativeRoutesProgrammed,
    ZooKeeperReachable,
    RemoteStorageReady,
    FlussReachable,
    ClusterHealthy,
    S3CredentialsStale,
    OperationBlocked,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema, PartialEq)]
pub enum ConditionStatus {
    True,
    False,
    Unknown,
}

#[cfg(test)]
mod tests {
    use super::{FlussCluster, ObservabilitySpec, ScaleInPolicy};
    use kube::CustomResourceExt;

    #[test]
    fn checked_in_crd_matches_the_generator() {
        let generated = serde_json::to_value(FlussCluster::crd()).expect("CRD must serialize");
        let checked_in: serde_json::Value =
            serde_yaml::from_str(include_str!("../deploy/crd.yaml"))
                .expect("checked-in CRD must parse");
        assert_eq!(
            generated, checked_in,
            "deploy/crd.yaml is stale: regenerate with `cargo run --bin gen-crd`"
        );
    }

    #[test]
    fn crd_exposes_lifecycle_policy_and_status() {
        let crd = serde_json::to_value(FlussCluster::crd()).expect("CRD must serialize");
        let schema = &crd["spec"]["versions"][0]["schema"]["openAPIV3Schema"];

        for field in [
            "listeners",
            "podDisruptionBudget",
            "rollingUpgrade",
            "scaleIn",
        ] {
            assert!(
                schema["properties"]["spec"]["properties"][field].is_object(),
                "missing spec.{field} from generated CRD"
            );
        }
        assert!(schema["properties"]["status"].is_object());
        let validations = &schema["properties"]["spec"]["x-kubernetes-validations"];
        assert!(validations.is_array());
        let rules: Vec<&str> = validations
            .as_array()
            .expect("spec validations must be an array")
            .iter()
            .filter_map(|v| v["rule"].as_str())
            .collect();
        assert!(
            rules
                .iter()
                .any(|r| r.contains("logReplicationFactor") && r.contains("tabletServers")),
            "missing RF<=tablets validation, got: {rules:?}"
        );
        let s3 = &schema["properties"]["spec"]["properties"]["remoteStorage"]["properties"]["s3"];
        assert!(s3["x-kubernetes-validations"].is_array());
        assert!(s3["properties"]["authentication"]["x-kubernetes-validations"].is_array());
        assert!(s3["properties"]["delegation"]["x-kubernetes-validations"].is_array());
        assert_eq!(
            crd["spec"]["versions"][0]["subresources"]["status"],
            serde_json::json!({})
        );
    }

    #[test]
    fn documented_manifests_match_the_rust_api() {
        for yaml in [
            include_str!("../docs/en/examples/aws-eks.yaml"),
            include_str!("../docs/en/examples/rustfs.yaml"),
        ] {
            let cluster: FlussCluster =
                serde_yaml::from_str(yaml).expect("documented FlussCluster must deserialize");
            assert!(matches!(
                cluster
                    .spec
                    .scale_in
                    .expect("scale-in policy is documented")
                    .on_non_empty_tablet_server,
                ScaleInPolicy::Block
            ));
        }
    }

    #[test]
    fn observability_prometheus_defaults_to_true() {
        let empty: ObservabilitySpec =
            serde_yaml::from_str("{}").expect("empty observability must deserialize");
        assert!(empty.prometheus, "absent flag means default-on");
        let explicit: ObservabilitySpec =
            serde_yaml::from_str("prometheus: false").expect("explicit false must deserialize");
        assert!(!explicit.prometheus, "explicit opt-out must hold");
    }
}
