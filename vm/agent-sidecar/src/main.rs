use std::io::{BufRead, BufReader, Write};
use std::os::fd::{AsRawFd, FromRawFd, IntoRawFd};
use std::os::unix::io::RawFd;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use anyhow::{Context, Result};
use nix::sys::socket::{socket, AddressFamily, SockFlag, SockType};
use serde::{Deserialize, Serialize};
use tracing::info;

// vsock constants
const VSOCK_PORT: u32 = 5000;

// Message types matching the host API
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum VsockMessage {
    Init {
        api_key: String,
        prompt: String,
        files: Option<Vec<TaskFile>>,
    },
    Output {
        data: String,
    },
    Input {
        data: String,
    },
    Exit {
        code: i32,
    },
    /// Error message sent to host when something fails
    Error {
        message: String,
    },
    Heartbeat,
}

/// Send an error message to the host via vsock
fn send_error(writer: &mut std::fs::File, message: &str) {
    tracing::error!("Sending error to host: {}", message);
    let msg = VsockMessage::Error {
        message: message.to_string(),
    };
    if let Ok(json) = serde_json::to_string(&msg) {
        let _ = writer.write_all((json + "\n").as_bytes());
        let _ = writer.flush();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskFile {
    pub name: String,
    pub content: String,
}

/// Claude Code stream-json input format
#[derive(Debug, Clone, Serialize)]
struct ClaudeInputMessage {
    #[serde(rename = "type")]
    msg_type: &'static str,
    message: ClaudeMessageContent,
}

#[derive(Debug, Clone, Serialize)]
struct ClaudeMessageContent {
    role: &'static str,
    content: String,
}

impl ClaudeInputMessage {
    fn user(content: String) -> Self {
        Self {
            msg_type: "user",
            message: ClaudeMessageContent {
                role: "user",
                content,
            },
        }
    }
}

fn main() -> Result<()> {
    // Initialize logging - log to file for debugging
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/var/log/agent-sidecar-debug.log")
        .ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "agent_sidecar=debug".into()),
        )
        .with_writer(move || {
            if let Some(ref f) = log_file {
                Box::new(f.try_clone().unwrap()) as Box<dyn std::io::Write>
            } else {
                Box::new(std::io::stderr()) as Box<dyn std::io::Write>
            }
        })
        .init();

    info!("Agent sidecar starting...");

    // Listen for host connection via vsock
    info!("Attempting to create vsock socket...");
    let listen_fd = match listen_vsock(VSOCK_PORT) {
        Ok(fd) => {
            info!("vsock listen succeeded, fd={}", fd);
            fd
        }
        Err(e) => {
            tracing::error!("Failed to listen on vsock: {:?}", e);
            return Err(e);
        }
    };
    info!("Listening on vsock port {}", VSOCK_PORT);

    // Accept connection from host
    let vsock_fd = accept_vsock(listen_fd)?;
    info!("Accepted connection from host via vsock");

    // Read init message
    let vsock_reader = unsafe { std::fs::File::from_raw_fd(vsock_fd) };
    let mut vsock_writer = vsock_reader.try_clone()?;

    let mut line = String::new();
    let mut reader = BufReader::new(&vsock_reader);
    if let Err(e) = reader.read_line(&mut line) {
        send_error(&mut vsock_writer, &format!("Failed to read init message: {}", e));
        anyhow::bail!("Failed to read init message: {}", e);
    }

    let init_msg: VsockMessage = match serde_json::from_str(&line) {
        Ok(msg) => msg,
        Err(e) => {
            send_error(&mut vsock_writer, &format!("Failed to parse init message: {} (raw: {})", e, line.trim()));
            anyhow::bail!("Failed to parse init message: {}", e);
        }
    };

    let (api_key, prompt, files) = match init_msg {
        VsockMessage::Init {
            api_key,
            prompt,
            files,
        } => (api_key, prompt, files),
        _ => {
            send_error(&mut vsock_writer, &format!("Expected Init message, got {:?}", init_msg));
            anyhow::bail!("Expected Init message, got {:?}", init_msg);
        }
    };

    info!("Received init message, starting Claude Code");

    // Write files if provided
    if let Some(files) = files {
        for file in files {
            let path = std::path::Path::new("/workspace").join(&file.name);
            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    send_error(&mut vsock_writer, &format!("Failed to create directory {}: {}", parent.display(), e));
                    anyhow::bail!("Failed to create directory: {}", e);
                }
            }
            if let Err(e) = std::fs::write(&path, &file.content) {
                send_error(&mut vsock_writer, &format!("Failed to write file {}: {}", path.display(), e));
                anyhow::bail!("Failed to write file: {}", e);
            }
            info!("Wrote file: {}", path.display());
        }
    }

    // Check if Claude binary exists
    let claude_path = "/home/claude/.local/bin/claude";
    if !std::path::Path::new(claude_path).exists() {
        send_error(&mut vsock_writer, &format!("Claude binary not found at {}", claude_path));
        anyhow::bail!("Claude binary not found");
    }

    // Spawn Claude Code process with piped I/O
    // Use stream-json for both input and output for structured bidirectional communication
    // Run as 'claude' user to allow --dangerously-skip-permissions (which doesn't work as root)
    let mut child = match Command::new("sudo")
        .arg("-u")
        .arg("claude")
        .arg("-E")  // Preserve environment (for ANTHROPIC_API_KEY)
        .arg("--")
        .arg(claude_path)
        .arg("--print")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--output-format")
        .arg("stream-json")
        .arg("--verbose")
        .arg("--include-partial-messages")
        .arg("--dangerously-skip-permissions")
        .env("ANTHROPIC_API_KEY", &api_key)
        .env("HOME", "/home/claude")
        .current_dir("/workspace")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            send_error(&mut vsock_writer, &format!("Failed to spawn Claude Code: {}", e));
            anyhow::bail!("Failed to spawn Claude Code: {}", e);
        }
    };

    let mut child_stdin = child.stdin.take().expect("Failed to get stdin");
    let child_stdout = child.stdout.take().expect("Failed to get stdout");
    let child_stderr = child.stderr.take().expect("Failed to get stderr");

    // Send initial prompt to Claude via stdin as JSON
    let initial_msg = ClaudeInputMessage::user(prompt);
    let initial_json = serde_json::to_string(&initial_msg)? + "\n";
    if let Err(e) = child_stdin.write_all(initial_json.as_bytes()) {
        send_error(&mut vsock_writer, &format!("Failed to send initial prompt to Claude: {}", e));
        anyhow::bail!("Failed to send initial prompt: {}", e);
    }
    if let Err(e) = child_stdin.flush() {
        send_error(&mut vsock_writer, &format!("Failed to flush stdin: {}", e));
        anyhow::bail!("Failed to flush stdin: {}", e);
    }
    info!("Sent initial prompt to Claude");

    let running = Arc::new(AtomicBool::new(true));

    // Thread: stdout -> vsock (line-based for stream-json format)
    let running_clone = running.clone();
    let mut vsock_writer_stdout = vsock_writer.try_clone()?;
    let stdout_thread = std::thread::spawn(move || {
        let reader = BufReader::new(child_stdout);
        for line in reader.lines() {
            if !running_clone.load(Ordering::Relaxed) {
                break;
            }
            match line {
                Ok(data) => {
                    // Each line is a complete JSON object from Claude Code
                    let msg = VsockMessage::Output { data };
                    let json = serde_json::to_string(&msg).unwrap() + "\n";
                    if vsock_writer_stdout.write_all(json.as_bytes()).is_err() {
                        break;
                    }
                    let _ = vsock_writer_stdout.flush();
                }
                Err(_) => break,
            }
        }
    });

    // Thread: stderr -> vsock
    let running_clone = running.clone();
    let mut vsock_writer_stderr = vsock_writer.try_clone()?;
    let stderr_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(child_stderr);
        let mut buffer = [0u8; 4096];
        while running_clone.load(Ordering::Relaxed) {
            match std::io::Read::read(&mut reader, &mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                    let msg = VsockMessage::Output { data };
                    let json = serde_json::to_string(&msg).unwrap() + "\n";
                    if vsock_writer_stderr.write_all(json.as_bytes()).is_err() {
                        break;
                    }
                    let _ = vsock_writer_stderr.flush();
                }
                Err(_) => break,
            }
        }
    });

    // Thread: vsock input -> stdin (convert to Claude's stream-json format)
    let running_clone = running.clone();
    let input_thread = std::thread::spawn(move || {
        let mut reader = BufReader::new(vsock_reader);
        let mut line = String::new();
        while running_clone.load(Ordering::Relaxed) {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Ok(_) => {
                    if let Ok(msg) = serde_json::from_str::<VsockMessage>(&line) {
                        match msg {
                            VsockMessage::Input { data } => {
                                // Wrap user input in Claude's expected JSON format
                                let claude_msg = ClaudeInputMessage::user(data);
                                let json = match serde_json::to_string(&claude_msg) {
                                    Ok(j) => j + "\n",
                                    Err(_) => continue,
                                };
                                if child_stdin.write_all(json.as_bytes()).is_err() {
                                    break;
                                }
                                let _ = child_stdin.flush();
                            }
                            VsockMessage::Heartbeat => {
                                // Respond to heartbeat
                            }
                            _ => {}
                        }
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Wait for Claude Code to exit
    let status = child.wait()?;
    let exit_code = status.code().unwrap_or(-1);
    info!("Claude Code exited with code: {}", exit_code);

    // Stop relay threads
    running.store(false, Ordering::Relaxed);

    // If Claude exited with an error, send error message
    if exit_code != 0 {
        send_error(&mut vsock_writer, &format!("Claude Code exited with code {}", exit_code));
    }

    // Send exit message
    let exit_msg = VsockMessage::Exit { code: exit_code };
    let json = serde_json::to_string(&exit_msg)? + "\n";
    let _ = vsock_writer.write_all(json.as_bytes());

    // Wait for threads to finish
    let _ = stdout_thread.join();
    let _ = stderr_thread.join();
    let _ = input_thread.join();

    info!("Agent sidecar shutting down");
    Ok(())
}

fn listen_vsock(port: u32) -> Result<RawFd> {
    info!("Creating vsock socket with AF_VSOCK={}", libc::AF_VSOCK);

    // Create vsock socket
    let fd = match socket(
        AddressFamily::Vsock,
        SockType::Stream,
        SockFlag::empty(),
        None,
    ) {
        Ok(fd) => {
            info!("Socket created successfully, fd={}", fd.as_raw_fd());
            fd
        }
        Err(e) => {
            tracing::error!("socket() failed: {:?}", e);
            anyhow::bail!("Failed to create vsock socket: {:?}", e);
        }
    };

    // Bind to listen on any CID, specified port
    info!(
        "Binding to vsock port {} with CID=VMADDR_CID_ANY ({})",
        port,
        libc::VMADDR_CID_ANY
    );
    let addr = libc::sockaddr_vm {
        svm_family: libc::AF_VSOCK as u16,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: libc::VMADDR_CID_ANY,
        svm_zero: [0; 4],
    };

    let ret = unsafe {
        libc::bind(
            fd.as_raw_fd(),
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_vm>() as u32,
        )
    };

    if ret < 0 {
        let err = std::io::Error::last_os_error();
        tracing::error!("bind() failed: {} (errno={})", err, err.raw_os_error().unwrap_or(-1));
        anyhow::bail!("Failed to bind vsock: {}", err);
    }
    info!("Bind successful");

    // Listen for connections
    let raw_fd = fd.as_raw_fd();
    info!("Calling listen() on fd={}", raw_fd);
    let ret = unsafe { libc::listen(raw_fd, 1) };
    if ret < 0 {
        let err = std::io::Error::last_os_error();
        tracing::error!("listen() failed: {} (errno={})", err, err.raw_os_error().unwrap_or(-1));
        anyhow::bail!("Failed to listen on vsock: {}", err);
    }
    info!("Listen successful");

    // Use into_raw_fd() to prevent OwnedFd from closing the socket when dropped
    Ok(fd.into_raw_fd())
}

fn accept_vsock(listen_fd: RawFd) -> Result<RawFd> {
    let mut addr: libc::sockaddr_vm = unsafe { std::mem::zeroed() };
    let mut addr_len = std::mem::size_of::<libc::sockaddr_vm>() as u32;

    let conn_fd = unsafe {
        libc::accept(
            listen_fd,
            &mut addr as *mut _ as *mut libc::sockaddr,
            &mut addr_len,
        )
    };

    if conn_fd < 0 {
        anyhow::bail!(
            "Failed to accept vsock connection: {}",
            std::io::Error::last_os_error()
        );
    }

    Ok(conn_fd)
}
