use std::sync::Arc;

use axum::routing::{delete, get, post};
use axum::Router;
use tower_http::trace::TraceLayer;

use super::handlers;
use crate::engine::Orchestrator;

pub fn build(orchestrator: Arc<Orchestrator>) -> Router {
    Router::new()
        .route("/health", get(handlers::health))
        // Projects
        .route("/api/v1/projects", get(handlers::list_projects))
        .route("/api/v1/projects", post(handlers::create_project))
        // Deployments
        .route("/api/v1/deployments", get(handlers::list_deployments))
        .route("/api/v1/deploy", post(handlers::deploy))
        .route(
            "/api/v1/projects/{project}/deployments/{name}",
            delete(handlers::remove_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/stop",
            post(handlers::stop_deployment),
        )
        .route(
            "/api/v1/projects/{project}/deployments/{name}/scale",
            post(handlers::scale_deployment),
        )
        // Pods
        .route("/api/v1/pods", get(handlers::list_pods))
        // Logs
        .route(
            "/api/v1/projects/{project}/deployments/{name}/logs",
            get(handlers::logs),
        )
        .layer(TraceLayer::new_for_http())
        .with_state(orchestrator)
}
