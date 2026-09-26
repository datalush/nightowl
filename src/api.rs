use std::collections::BTreeMap;

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
#[serde(rename_all = "camelCase")]
pub struct FlussClusterSpec {
    pub version: String,
    pub image: ImageSpec,
    pub zookeeper: ZookeeperSpec,
    pub coordinator: CoordinatorSpec,
    pub tablet_servers: TabletServersSpec,
    pub remote_storage: RemoteStorageSpec,
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
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub configuration_overrides: BTreeMap<String, String>,
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
    pub internal: ListenerSpec,
    pub client: ClientListenerSpec,
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
    /// Only in-cluster clients are supported by this API version.
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
#[serde(rename_all = "camelCase")]
pub struct TableDefaultsSpec {
    #[schemars(range(min = 1))]
    pub buckets: i32,
    #[schemars(range(min = 1))]
    pub log_replication_factor: i32,
    #[schemars(range(min = 1))]
    pub min_in_sync_replicas: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize, JsonSchema)]
pub struct ObservabilitySpec {
    #[serde(default)]
    pub prometheus: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, JsonSchema, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct FlussClusterStatus {
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
    Adoptable,
    KubernetesResourcesReady,
    ZooKeeperReachable,
    RemoteStorageReady,
    FlussReachable,
    ClusterHealthy,
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
    use super::{FlussCluster, ScaleInPolicy};
    use kube::CustomResourceExt;

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
            include_str!("../docs/en/examples/minio.yaml"),
            include_str!("../docs/en/examples/garage.yaml"),
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
}
