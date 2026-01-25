use anyhow::Result;
use chrono::{DateTime, Utc};
use clap::{Parser, Subcommand};
use colored::Colorize;
use futures::StreamExt;
use std::io::{self, Write};

mod api;
use api::ApiClient;

#[derive(Parser)]
#[command(name = "lia", about = "Lia development CLI", version)]
struct Cli {
    /// VM API URL
    #[arg(long, env = "LIA_API_URL", default_value = "http://localhost:8811")]
    api_url: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List tasks
    Tasks {
        /// Filter by status (pending, starting, running, suspended, terminated)
        #[arg(long)]
        status: Option<String>,
    },
    /// View or stream VM logs
    Logs {
        /// Task ID (UUID or prefix)
        task_id: String,
        /// Follow/stream logs (like tail -f)
        #[arg(short, long)]
        follow: bool,
        /// Number of lines to show initially
        #[arg(short = 'n', long, default_value = "100")]
        tail: usize,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = ApiClient::new(&cli.api_url);

    match cli.command {
        Commands::Tasks { status } => list_tasks(&client, status.as_deref()).await?,
        Commands::Logs {
            task_id,
            follow,
            tail,
        } => {
            if follow {
                stream_logs(&client, &task_id, tail).await?;
            } else {
                get_logs(&client, &task_id, tail).await?;
            }
        }
    }

    Ok(())
}

async fn list_tasks(client: &ApiClient, status: Option<&str>) -> Result<()> {
    let response = client.list_tasks(status).await?;

    // Print header
    println!(
        "{:<38} {:<12} {:<20} {:<16}",
        "ID".bold(),
        "STATUS".bold(),
        "CREATED".bold(),
        "IP".bold()
    );

    for task in response.tasks {
        let status_colored = match task.status.as_str() {
            "running" => task.status.green(),
            "starting" | "pending" => task.status.yellow(),
            "suspended" => task.status.blue(),
            "terminated" => task.status.red(),
            _ => task.status.normal(),
        };

        let created = task
            .created_at
            .format("%Y-%m-%d %H:%M")
            .to_string();

        let ip = task.ip_address.unwrap_or_else(|| "-".to_string());

        println!("{:<38} {:<12} {:<20} {:<16}", task.id, status_colored, created, ip);
    }

    println!("\n{} total tasks", response.total);

    Ok(())
}

async fn get_logs(client: &ApiClient, task_id: &str, tail: usize) -> Result<()> {
    let response = client.get_logs(task_id, tail).await?;

    for line in response.lines {
        println!("{}", line);
    }

    Ok(())
}

async fn stream_logs(client: &ApiClient, task_id: &str, tail: usize) -> Result<()> {
    let stream = client.stream_logs(task_id, tail).await?;
    tokio::pin!(stream);

    while let Some(result) = stream.next().await {
        match result {
            Ok(event) => {
                match event.event_type.as_str() {
                    "log" => {
                        if let Some(line) = event.line {
                            print!("{}", line);
                            io::stdout().flush()?;
                        }
                    }
                    "init" => {
                        eprintln!(
                            "{}",
                            format!("Connected to task {}", event.task_id.unwrap_or_default())
                                .dimmed()
                        );
                    }
                    "heartbeat" => {
                        // Silent heartbeat
                    }
                    "error" => {
                        if let Some(error) = event.error {
                            eprintln!("{}: {}", "Error".red(), error);
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                eprintln!("{}: {}", "Stream error".red(), e);
                break;
            }
        }
    }

    Ok(())
}
