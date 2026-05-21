use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;

use crate::engine::Orchestrator;
use nexa_core::config::parse_deployment;
use nexa_core::models::DeploymentSpec;

type AppState = State<Arc<Orchestrator>>;

pub async fn health() -> &'static str {
    "ok"
}

// --- Projects ---

pub async fn list_projects(State(orch): AppState) -> impl IntoResponse {
    Json(orch.list_projects())
}

#[derive(Deserialize)]
pub struct CreateProjectRequest {
    name: String,
}

pub async fn create_project(
    State(orch): AppState,
    Json(req): Json<CreateProjectRequest>,
) -> impl IntoResponse {
    match orch.create_project(&req.name) {
        Ok(project) => (StatusCode::CREATED, Json(serde_json::json!(project))).into_response(),
        Err(e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// --- Deployments ---

#[derive(Deserialize)]
pub struct DeploymentFilter {
    project: Option<String>,
}

pub async fn list_deployments(
    State(orch): AppState,
    Query(filter): Query<DeploymentFilter>,
) -> impl IntoResponse {
    Json(orch.list_deployments(filter.project.as_deref()))
}

pub async fn deploy(State(orch): AppState, body: String) -> impl IntoResponse {
    let spec = match serde_json::from_str::<DeploymentSpec>(&body) {
        Ok(spec) => spec,
        Err(_) => match parse_deployment(&body) {
            Ok(spec) => spec,
            Err(e) => {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({ "error": e.to_string() })),
                )
                    .into_response();
            }
        },
    };

    match orch.deploy(spec).await {
        Ok(deployment) => (StatusCode::CREATED, Json(serde_json::json!(deployment))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn stop_deployment(
    State(orch): AppState,
    Path((project, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match orch.stop_deployment(&project, &name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

pub async fn remove_deployment(
    State(orch): AppState,
    Path((project, name)): Path<(String, String)>,
) -> impl IntoResponse {
    match orch.remove_deployment(&project, &name).await {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct ScaleRequest {
    replicas: u32,
}

pub async fn scale_deployment(
    State(orch): AppState,
    Path((project, name)): Path<(String, String)>,
    Json(req): Json<ScaleRequest>,
) -> impl IntoResponse {
    match orch.scale_deployment(&project, &name, req.replicas).await {
        Ok(deployment) => Json(serde_json::json!(deployment)).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}

// --- Pods ---

pub async fn list_pods(
    State(orch): AppState,
    Query(filter): Query<DeploymentFilter>,
) -> impl IntoResponse {
    Json(orch.list_pods(filter.project.as_deref()))
}

// --- Logs ---

#[derive(Deserialize)]
pub struct LogsQuery {
    tail: Option<u64>,
}

pub async fn logs(
    State(orch): AppState,
    Path((project, name)): Path<(String, String)>,
    Query(query): Query<LogsQuery>,
) -> impl IntoResponse {
    match orch.pod_logs(&project, &name, query.tail).await {
        Ok(stream) => {
            let event_stream = stream.map(|result| -> std::result::Result<Event, std::convert::Infallible> {
                match result {
                    Ok(line) => Ok(Event::default().data(line)),
                    Err(e) => Ok(Event::default().data(format!("error: {e}"))),
                }
            });
            Sse::new(event_stream).into_response()
        }
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": e.to_string() })),
        )
            .into_response(),
    }
}
