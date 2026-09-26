//! Print the FlussCluster CRD derived from the Rust types.
//!
//! Usage: cargo run -q --bin gen-crd | kubectl apply -f -
//!
//! JSON on purpose: kubectl accepts it natively, and serde_json is
//! already a main dependency (serde_yaml is dev-only and binaries
//! cannot use dev-dependencies).

use kube::CustomResourceExt;
use nightowl::api::FlussCluster;

fn main() {
    let crd = FlussCluster::crd();
    println!(
        "{}",
        serde_json::to_string_pretty(&crd).expect("CRD must serialize")
    );
}
