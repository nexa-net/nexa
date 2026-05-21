mod client;
mod commands;
mod output;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "nexa",
    about = "NexaNet CLI — deploy and manage containers",
    version,
    propagate_version = true
)]
struct Cli {
    #[arg(long, default_value = "http://localhost:6443", global = true)]
    server: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Deploy a service from a YAML spec
    Deploy {
        /// Path to the deployment YAML file
        file: String,
    },

    /// List all pods
    Pods {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    /// List all deployments
    Deployments {
        /// Filter by project
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Stream logs from a deployment
    Logs {
        /// Deployment name
        name: String,

        /// Project name
        #[arg(short, long)]
        project: Option<String>,

        /// Number of lines to tail
        #[arg(long)]
        tail: Option<u64>,
    },

    /// Scale a deployment
    Scale {
        /// Deployment name
        name: String,

        /// Number of replicas
        replicas: u32,

        /// Project name
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Stop a deployment
    Stop {
        /// Deployment name
        name: String,

        /// Project name
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Remove a deployment
    Rm {
        /// Deployment name
        name: String,

        /// Project name
        #[arg(short, long)]
        project: Option<String>,
    },

    /// Manage projects
    Project {
        #[command(subcommand)]
        command: ProjectCommands,
    },
}

#[derive(Subcommand)]
enum ProjectCommands {
    /// List all projects
    List,

    /// Create a new project
    Create {
        /// Project name
        name: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let client = client::NexaClient::new(&cli.server);

    match cli.command {
        Commands::Deploy { file } => commands::deploy(&client, &file).await,
        Commands::Pods { project } => commands::pods(&client, project.as_deref()).await,
        Commands::Deployments { project } => {
            commands::deployments(&client, project.as_deref()).await
        }
        Commands::Logs {
            name,
            project,
            tail,
        } => commands::logs(&client, project.as_deref(), &name, tail).await,
        Commands::Scale {
            name,
            replicas,
            project,
        } => commands::scale(&client, project.as_deref(), &name, replicas).await,
        Commands::Stop { name, project } => {
            commands::stop(&client, project.as_deref(), &name).await
        }
        Commands::Rm { name, project } => {
            commands::remove(&client, project.as_deref(), &name).await
        }
        Commands::Project { command } => match command {
            ProjectCommands::List => commands::list_projects(&client).await,
            ProjectCommands::Create { name } => commands::create_project(&client, &name).await,
        },
    }
}
