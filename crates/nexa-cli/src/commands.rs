use anyhow::Result;
use nexa_core::config::parse_deployment_file;
use nexa_core::models::{Deployment, Pod, Project};
use reqwest::Response;
use std::path::Path;

use crate::client::NexaClient;
use crate::output;

pub async fn deploy(client: &NexaClient, file: &str) -> Result<()> {
    let path = Path::new(file);
    if !path.exists() {
        anyhow::bail!("file not found: {file}");
    }

    let spec = parse_deployment_file(path)?;
    let project = spec.project.clone();
    let name = spec.deployment.name.clone();
    let replicas = spec.replicas;

    println!(
        "Deploying {name} ({replicas} replica{}) to project '{project}'...",
        if replicas > 1 { "s" } else { "" }
    );

    let yaml = std::fs::read_to_string(path)?;
    let deployment: Deployment = client.post_yaml("/api/v1/deploy", &yaml).await?;

    output::print_success(&format!(
        "Deployment '{name}' is {:?} (id: {})",
        deployment.status,
        &deployment.id.to_string()[..8]
    ));

    Ok(())
}

pub async fn pods(client: &NexaClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/pods?project={p}"),
        None => "/api/v1/pods".to_string(),
    };

    let pods: Vec<Pod> = client.get(&path).await?;

    let rows: Vec<Vec<String>> = pods
        .iter()
        .map(|p| {
            vec![
                p.container_name(),
                p.project.clone(),
                p.deployment_name.clone(),
                p.status.to_string(),
                p.image.clone(),
                p.created_at.format("%Y-%m-%d %H:%M").to_string(),
            ]
        })
        .collect();

    output::print_table(
        &["Name", "Project", "Deployment", "Status", "Image", "Created"],
        &rows,
    );

    Ok(())
}

pub async fn deployments(client: &NexaClient, project: Option<&str>) -> Result<()> {
    let path = match project {
        Some(p) => format!("/api/v1/deployments?project={p}"),
        None => "/api/v1/deployments".to_string(),
    };

    let deployments: Vec<Deployment> = client.get(&path).await?;

    let rows: Vec<Vec<String>> = deployments
        .iter()
        .map(|d| {
            vec![
                d.name().to_string(),
                d.project().to_string(),
                format!("{:?}", d.status),
                d.spec.replicas.to_string(),
                d.spec.image.clone(),
                d.created_at.format("%Y-%m-%d %H:%M").to_string(),
            ]
        })
        .collect();

    output::print_table(
        &["Name", "Project", "Status", "Replicas", "Image", "Created"],
        &rows,
    );

    Ok(())
}

pub async fn logs(
    client: &NexaClient,
    project: Option<&str>,
    name: &str,
    tail: Option<u64>,
) -> Result<()> {
    let project = project.unwrap_or("default");
    let mut path = format!("/api/v1/projects/{project}/deployments/{name}/logs");
    if let Some(t) = tail {
        path.push_str(&format!("?tail={t}"));
    }

    let resp: Response = client.get_stream(&path).await?;
    let mut stream = resp.bytes_stream();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                print!("{data}");
            }
        }
    }

    Ok(())
}

pub async fn scale(
    client: &NexaClient,
    project: Option<&str>,
    name: &str,
    replicas: u32,
) -> Result<()> {
    let project = project.unwrap_or("default");
    let path = format!("/api/v1/projects/{project}/deployments/{name}/scale");
    let body = serde_json::json!({ "replicas": replicas }).to_string();

    let deployment: Deployment = client.post_json(&path, &body).await?;
    output::print_success(&format!(
        "Scaled '{name}' to {} replica{}",
        deployment.spec.replicas,
        if deployment.spec.replicas > 1 { "s" } else { "" }
    ));

    Ok(())
}

pub async fn stop(client: &NexaClient, project: Option<&str>, name: &str) -> Result<()> {
    let project = project.unwrap_or("default");
    let path = format!("/api/v1/projects/{project}/deployments/{name}/stop");

    client.post_empty(&path).await?;
    output::print_success(&format!("Deployment '{name}' stopped"));

    Ok(())
}

pub async fn remove(client: &NexaClient, project: Option<&str>, name: &str) -> Result<()> {
    let project = project.unwrap_or("default");
    let path = format!("/api/v1/projects/{project}/deployments/{name}");

    client.delete(&path).await?;
    output::print_success(&format!("Deployment '{name}' removed"));

    Ok(())
}

pub async fn list_projects(client: &NexaClient) -> Result<()> {
    let projects: Vec<Project> = client.get("/api/v1/projects").await?;

    let rows: Vec<Vec<String>> = projects
        .iter()
        .map(|p| {
            vec![
                p.name.clone(),
                p.created_at.format("%Y-%m-%d %H:%M").to_string(),
            ]
        })
        .collect();

    output::print_table(&["Name", "Created"], &rows);

    Ok(())
}

pub async fn create_project(client: &NexaClient, name: &str) -> Result<()> {
    let body = serde_json::json!({ "name": name }).to_string();
    let _: Project = client.post_json("/api/v1/projects", &body).await?;
    output::print_success(&format!("Project '{name}' created"));

    Ok(())
}
