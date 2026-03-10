use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    gateway::init_tracing();

    let config = gateway::config::Config::from_env()?;
    let bind_addr = config.bind_addr;
    let state = gateway::build_state(config).await?;
    let app = gateway::app(state);

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!("gateway listening on {bind_addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
