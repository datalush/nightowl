use nightowl::controller;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let kube_client = kube::Client::try_default().await?;
    controller::run(kube_client, "operator-dev").await;

    Ok(())
}
