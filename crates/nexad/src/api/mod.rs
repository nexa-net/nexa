mod handlers;
mod routes;

use std::sync::Arc;

use crate::engine::Orchestrator;

pub async fn serve(orchestrator: Arc<Orchestrator>, addr: &str) -> anyhow::Result<()> {
    let app = routes::build(orchestrator);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("nexad API listening on {addr}");

    axum::serve(listener, app).await?;
    Ok(())
}
