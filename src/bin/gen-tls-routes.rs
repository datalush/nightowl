// SPDX-License-Identifier: AGPL-3.0-only
//! Inspect the desired SNI routing from a FlussCluster JSON on stdin.
//! Usage: kubectl get flusscluster NAME -n NS -o json | gen-tls-routes

use std::io;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cluster: nightowl::api::FlussCluster = serde_json::from_reader(io::stdin())?;
    if cluster.metadata.name.is_none() || cluster.metadata.namespace.is_none() {
        return Err("input must have metadata.name and metadata.namespace".into());
    }
    let manifests = nightowl::resources::tls_routes::manifests(&cluster)?;
    println!("{}", serde_json::to_string_pretty(&manifests)?);
    Ok(())
}
