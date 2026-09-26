use nightowl::controller;

/// Watch every namespace unless `--namespace <name>` pins one.
///
/// Single-namespace mode exists for restricted environments; production
/// runs cluster-wide under the `nightowl` ClusterRole, with the controller
/// logic (never adopt foreign objects) as the narrowing layer.
fn watched_namespace() -> Option<String> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if let Some(name) = arg.strip_prefix("--namespace=") {
            return Some(name.to_string());
        }
        if arg == "--namespace" {
            return args.next();
        }
    }
    None
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let kube_client = kube::Client::try_default().await?;
    controller::run(kube_client, watched_namespace()).await;

    Ok(())
}
