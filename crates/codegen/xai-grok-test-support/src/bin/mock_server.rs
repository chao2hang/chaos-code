use xai_grok_test_support::MockInferenceServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let server = MockInferenceServer::start().await?;
    println!("Mock server running at: {}", server.url());
    println!("Press Ctrl+C to stop...");
    tokio::signal::ctrl_c().await?;
    println!("Shutting down mock server...");
    Ok(())
}
