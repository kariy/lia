//! Integration test for Claude Code streaming via vsock in Firecracker VMs
//!
//! This test verifies that:
//! 1. The agent-sidecar correctly spawns Claude Code with stream-json flags
//! 2. The vsock communication protocol works for bidirectional streaming
//! 3. Claude's JSON output events are correctly relayed to the host
//!
//! Prerequisites:
//! - Running as root (for TAP device and Firecracker)
//! - Firecracker binary installed at /usr/local/bin/firecracker
//! - Kernel at /var/lib/lia/kernel/vmlinux
//! - Rootfs at /var/lib/lia/rootfs/rootfs.ext4 with:
//!   - agent-sidecar binary (musl-linked)
//!   - Claude Code CLI installed (requires glibc - see note below)
//! - Network bridge (lia-br0) configured
//! - Valid ANTHROPIC_API_KEY environment variable
//!
//! IMPORTANT: The current Alpine-based rootfs uses musl libc, but the Claude Code
//! CLI binary requires glibc. To run these tests, you must either:
//! 1. Rebuild the rootfs using a glibc-based distro (Debian/Ubuntu minimal), or
//! 2. Install glibc compatibility layer in Alpine (gcompat package)
//!
//! Run with: sudo ANTHROPIC_API_KEY=sk-... cargo test --test claude_streaming_test -- --nocapture --test-threads=1

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};
use std::{fs, thread};

/// Check if running as root
fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

const FIRECRACKER_BIN: &str = "/usr/local/bin/firecracker";
const KERNEL_PATH: &str = "/var/lib/lia/kernel/vmlinux";
const ROOTFS_PATH: &str = "/var/lib/lia/rootfs/rootfs.ext4";
const BRIDGE_NAME: &str = "lia-br0";
const BRIDGE_IP: &str = "172.16.0.1";
const TEST_VM_IP: &str = "172.16.0.252";
const TEST_TAP_NAME: &str = "tap-claudetest";
const VSOCK_PORT: u32 = 5000;

/// Message types for vsock communication (matching agent-sidecar)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    Heartbeat,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TaskFile {
    pub name: String,
    pub content: String,
}

/// Claude Code JSON event types (subset for testing)
#[derive(Debug, Clone, serde::Deserialize)]
struct ClaudeEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default)]
    session_id: Option<String>,
    #[serde(default)]
    result: Option<String>,
    #[serde(default)]
    is_error: Option<bool>,
}

/// Check if all prerequisites are available
fn check_prerequisites() -> Result<(), String> {
    if !is_root() {
        return Err("This test must be run as root".to_string());
    }

    if !PathBuf::from(FIRECRACKER_BIN).exists() {
        return Err(format!("Firecracker binary not found at {}", FIRECRACKER_BIN));
    }

    if !PathBuf::from(KERNEL_PATH).exists() {
        return Err(format!("Kernel not found at {}", KERNEL_PATH));
    }

    if !PathBuf::from(ROOTFS_PATH).exists() {
        return Err(format!("Rootfs not found at {}", ROOTFS_PATH));
    }

    // Check bridge exists
    let output = Command::new("ip")
        .args(["link", "show", BRIDGE_NAME])
        .output()
        .map_err(|e| format!("Failed to check bridge: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Bridge {} not found. Run 'sudo bash vm/setup.sh' first",
            BRIDGE_NAME
        ));
    }

    // Check for API key
    if std::env::var("ANTHROPIC_API_KEY").is_err() {
        return Err("ANTHROPIC_API_KEY environment variable not set".to_string());
    }

    Ok(())
}

/// Create a TAP device and attach it to the bridge
fn create_tap_device(tap_name: &str) -> Result<(), String> {
    let _ = Command::new("ip")
        .args(["link", "delete", tap_name])
        .output();

    let output = Command::new("ip")
        .args(["tuntap", "add", "dev", tap_name, "mode", "tap"])
        .output()
        .map_err(|e| format!("Failed to create TAP device: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to create TAP device: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = Command::new("ip")
        .args(["link", "set", tap_name, "up"])
        .output()
        .map_err(|e| format!("Failed to bring up TAP device: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to bring up TAP: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    let output = Command::new("ip")
        .args(["link", "set", tap_name, "master", BRIDGE_NAME])
        .output()
        .map_err(|e| format!("Failed to attach TAP to bridge: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "Failed to attach TAP to bridge: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(())
}

fn delete_tap_device(tap_name: &str) {
    let _ = Command::new("ip")
        .args(["link", "set", tap_name, "down"])
        .output();
    let _ = Command::new("ip")
        .args(["link", "delete", tap_name])
        .output();
}

fn generate_mac(ip: &str) -> String {
    let last_octet: u8 = ip.split('.').last().unwrap().parse().unwrap_or(100);
    format!("02:FC:00:00:00:{:02X}", last_octet)
}

/// Firecracker configuration structures
#[derive(serde::Serialize)]
struct BootSource {
    kernel_image_path: String,
    boot_args: String,
}

#[derive(serde::Serialize)]
struct Drive {
    drive_id: String,
    path_on_host: String,
    is_root_device: bool,
    is_read_only: bool,
}

#[derive(serde::Serialize)]
struct MachineConfig {
    vcpu_count: u32,
    mem_size_mib: u32,
}

#[derive(serde::Serialize)]
struct NetworkInterface {
    iface_id: String,
    guest_mac: String,
    host_dev_name: String,
}

#[derive(serde::Serialize)]
struct VsockDevice {
    vsock_id: String,
    guest_cid: u32,
    uds_path: String,
}

#[derive(serde::Serialize)]
struct InstanceActionInfo {
    action_type: String,
}

/// Send a PUT request to Firecracker API via Unix socket
fn fc_put<T: serde::Serialize>(socket_path: &str, endpoint: &str, body: &T) -> Result<(), String> {
    let body_json =
        serde_json::to_string(body).map_err(|e| format!("JSON serialization error: {}", e))?;

    let output = Command::new("curl")
        .arg("--unix-socket")
        .arg(socket_path)
        .arg("-X")
        .arg("PUT")
        .arg("-H")
        .arg("Content-Type: application/json")
        .arg("-d")
        .arg(&body_json)
        .arg(format!("http://localhost{}", endpoint))
        .output()
        .map_err(|e| format!("Failed to call Firecracker API: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(format!("Firecracker API error: {} {}", stderr, stdout));
    }

    let response = String::from_utf8_lossy(&output.stdout);
    if response.contains("fault_message") {
        return Err(format!("Firecracker error: {}", response));
    }

    Ok(())
}

/// Test VM with vsock support
struct TestVm {
    socket_path: PathBuf,
    vsock_uds_path: PathBuf,
    rootfs_copy: PathBuf,
    log_path: PathBuf,
    process: std::process::Child,
    tap_name: String,
}

impl TestVm {
    fn start(vm_ip: &str) -> Result<Self, String> {
        let test_id = format!("claude-test-{}", std::process::id());
        let socket_path = PathBuf::from(format!("/tmp/{}.sock", test_id));
        let vsock_uds_path = PathBuf::from(format!("/tmp/{}_v.sock", test_id));
        let rootfs_copy = PathBuf::from(format!("/tmp/{}-rootfs.ext4", test_id));
        let log_path = PathBuf::from(format!("/tmp/{}.log", test_id));

        let _ = fs::remove_file(&socket_path);
        let _ = fs::remove_file(&vsock_uds_path);
        let _ = fs::remove_file(&log_path);

        println!("Copying rootfs...");
        fs::copy(ROOTFS_PATH, &rootfs_copy)
            .map_err(|e| format!("Failed to copy rootfs: {}", e))?;

        // Create empty log file (Firecracker requires it to exist)
        fs::write(&log_path, "").map_err(|e| format!("Failed to create log file: {}", e))?;

        println!("Creating TAP device {}...", TEST_TAP_NAME);
        create_tap_device(TEST_TAP_NAME)?;

        println!("Starting Firecracker...");
        let process = Command::new(FIRECRACKER_BIN)
            .arg("--api-sock")
            .arg(&socket_path)
            .arg("--log-path")
            .arg(&log_path)
            .arg("--level")
            .arg("Debug")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("Failed to start Firecracker: {}", e))?;

        println!("Waiting for Firecracker socket...");
        let start = Instant::now();
        while !socket_path.exists() {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("Timeout waiting for Firecracker socket".to_string());
            }
            thread::sleep(Duration::from_millis(100));
        }
        thread::sleep(Duration::from_millis(200));

        let socket_path_str = socket_path.to_string_lossy().to_string();

        println!("Configuring VM...");

        // Boot source with network config
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/init lia.ip={} lia.gateway={}",
            vm_ip, BRIDGE_IP
        );

        fc_put(
            &socket_path_str,
            "/boot-source",
            &BootSource {
                kernel_image_path: KERNEL_PATH.to_string(),
                boot_args,
            },
        )?;

        fc_put(
            &socket_path_str,
            "/machine-config",
            &MachineConfig {
                vcpu_count: 2,
                mem_size_mib: 1024, // More memory for Claude Code
            },
        )?;

        fc_put(
            &socket_path_str,
            "/drives/rootfs",
            &Drive {
                drive_id: "rootfs".to_string(),
                path_on_host: rootfs_copy.to_string_lossy().to_string(),
                is_root_device: true,
                is_read_only: false,
            },
        )?;

        // Network interface
        let mac_address = generate_mac(vm_ip);
        fc_put(
            &socket_path_str,
            "/network-interfaces/eth0",
            &NetworkInterface {
                iface_id: "eth0".to_string(),
                guest_mac: mac_address,
                host_dev_name: TEST_TAP_NAME.to_string(),
            },
        )?;

        // vsock device - CID 3 is conventional for guest
        fc_put(
            &socket_path_str,
            "/vsock",
            &VsockDevice {
                vsock_id: "vsock0".to_string(),
                guest_cid: 3,
                uds_path: vsock_uds_path.to_string_lossy().to_string(),
            },
        )?;

        println!("Starting VM instance...");
        fc_put(
            &socket_path_str,
            "/actions",
            &InstanceActionInfo {
                action_type: "InstanceStart".to_string(),
            },
        )?;

        // Wait a bit and check the log
        thread::sleep(Duration::from_secs(2));
        if let Ok(log_content) = fs::read_to_string(&log_path) {
            println!("\n=== Firecracker Log After Start ===");
            println!("{}", log_content);
            println!("=== End Firecracker Log ===\n");
        }

        Ok(TestVm {
            socket_path,
            vsock_uds_path,
            rootfs_copy,
            log_path,
            process,
            tap_name: TEST_TAP_NAME.to_string(),
        })
    }

    fn stop(&mut self) {
        println!("Stopping VM...");
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.vsock_uds_path);
        let _ = fs::remove_file(&self.rootfs_copy);
        let _ = fs::remove_file(&self.log_path);
        delete_tap_device(&self.tap_name);
    }
}

impl Drop for TestVm {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Connect to the agent-sidecar via vsock
fn connect_vsock(vsock_path: &PathBuf, timeout: Duration) -> Result<UnixStream, String> {
    println!("Connecting to vsock at {:?}...", vsock_path);
    let start = Instant::now();

    while start.elapsed() < timeout {
        if !vsock_path.exists() {
            thread::sleep(Duration::from_millis(500));
            continue;
        }

        match UnixStream::connect(vsock_path) {
            Ok(mut stream) => {
                // Firecracker vsock protocol: send "CONNECT <port>\n"
                let connect_cmd = format!("CONNECT {}\n", VSOCK_PORT);
                if let Err(e) = stream.write_all(connect_cmd.as_bytes()) {
                    println!("Failed to send CONNECT: {}", e);
                    thread::sleep(Duration::from_millis(500));
                    continue;
                }

                // Read response "OK <local_port>\n"
                let mut response = [0u8; 64];
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .ok();
                match stream.read(&mut response) {
                    Ok(n) if n > 0 => {
                        let response_str = String::from_utf8_lossy(&response[..n]);
                        if response_str.starts_with("OK ") {
                            println!("vsock connected: {}", response_str.trim());
                            return Ok(stream);
                        } else {
                            println!("Unexpected vsock response: {}", response_str.trim());
                        }
                    }
                    Ok(_) => println!("Empty vsock response"),
                    Err(e) => println!("Failed to read vsock response: {}", e),
                }
            }
            Err(e) => {
                println!("vsock connect attempt failed: {}", e);
            }
        }
        thread::sleep(Duration::from_millis(500));
    }

    Err(format!(
        "Timeout connecting to vsock after {:?}",
        timeout
    ))
}

/// Collected events from Claude streaming output
#[derive(Debug, Default)]
struct StreamingResults {
    got_system_init: bool,
    got_assistant_message: bool,
    got_result: bool,
    got_stream_events: bool,
    session_id: Option<String>,
    final_result: Option<String>,
    exit_code: Option<i32>,
    all_output: Vec<String>,
    errors: Vec<String>,
}

/// Read and parse streaming output from vsock
fn read_streaming_output(
    stream: &mut UnixStream,
    timeout: Duration,
) -> Result<StreamingResults, String> {
    let mut results = StreamingResults::default();
    let start = Instant::now();

    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;

    let mut reader = BufReader::new(stream.try_clone().map_err(|e| e.to_string())?);

    println!("\n=== Reading streaming output ===\n");

    while start.elapsed() < timeout {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => {
                println!("EOF reached");
                break;
            }
            Ok(_) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                // Parse vsock wrapper message
                match serde_json::from_str::<VsockMessage>(line) {
                    Ok(VsockMessage::Output { data }) => {
                        results.all_output.push(data.clone());

                        // Parse the inner Claude event
                        if let Ok(event) = serde_json::from_str::<ClaudeEvent>(&data) {
                            match event.event_type.as_str() {
                                "system" => {
                                    if event.subtype.as_deref() == Some("init") {
                                        results.got_system_init = true;
                                        results.session_id = event.session_id;
                                        println!("[SYSTEM INIT] session_id: {:?}", results.session_id);
                                    }
                                }
                                "stream_event" => {
                                    results.got_stream_events = true;
                                    // Don't print every delta, just note we got them
                                }
                                "assistant" => {
                                    results.got_assistant_message = true;
                                    println!("[ASSISTANT MESSAGE]");
                                }
                                "result" => {
                                    results.got_result = true;
                                    if event.is_error == Some(true) {
                                        results.errors.push(format!("Result error: {:?}", &event.result));
                                    }
                                    results.final_result = event.result;
                                    println!("[RESULT] success={}", event.is_error != Some(true));
                                    // We got the final result, can stop reading
                                    break;
                                }
                                other => {
                                    println!("[{}]", other);
                                }
                            }
                        } else {
                            // Not a structured Claude event, might be raw output
                            println!("[RAW] {}", if data.len() > 100 { &data[..100] } else { &data });
                        }
                    }
                    Ok(VsockMessage::Exit { code }) => {
                        results.exit_code = Some(code);
                        println!("[EXIT] code={}", code);
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("[PARSE ERROR] {}: {}", e, if line.len() > 50 { &line[..50] } else { line });
                    }
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // Timeout on read, check if we should continue
                if results.got_result {
                    break;
                }
                continue;
            }
            Err(e) => {
                println!("Read error: {}", e);
                break;
            }
        }
    }

    println!("\n=== Streaming output complete ===\n");
    println!("Total output lines: {}", results.all_output.len());
    println!("Got system init: {}", results.got_system_init);
    println!("Got stream events: {}", results.got_stream_events);
    println!("Got assistant message: {}", results.got_assistant_message);
    println!("Got result: {}", results.got_result);

    Ok(results)
}

#[test]
fn test_claude_streaming_via_vsock() {
    println!("\n=== Claude Code Streaming Integration Test ===\n");

    // Check prerequisites
    if let Err(e) = check_prerequisites() {
        println!("Skipping test: {}", e);
        println!("\nTo run this test:");
        println!("  1. Run as root with API key: sudo ANTHROPIC_API_KEY=sk-... cargo test --test claude_streaming_test -- --nocapture --test-threads=1");
        println!("  2. Ensure vm/setup.sh has been run");
        println!("  3. Ensure rootfs has been built with agent-sidecar and claude installed");
        return;
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    // Start VM with vsock
    println!("Starting Firecracker VM with vsock...");
    let mut vm = match TestVm::start(TEST_VM_IP) {
        Ok(vm) => vm,
        Err(e) => {
            panic!("Failed to start VM: {}", e);
        }
    };
    println!("VM started");

    // Wait for sidecar to be ready (it listens on vsock port 5000)
    // Debian boots slower than Alpine, so we need more time
    println!("Waiting for agent-sidecar to start (30s for Debian boot)...");
    thread::sleep(Duration::from_secs(30)); // Give VM time to boot

    // Connect to sidecar via vsock
    let mut stream = match connect_vsock(&vm.vsock_uds_path, Duration::from_secs(60)) {
        Ok(s) => s,
        Err(e) => {
            vm.stop();
            panic!("Failed to connect to vsock: {}", e);
        }
    };

    // Send init message with a simple prompt
    println!("Sending init message with test prompt...");
    let init_msg = VsockMessage::Init {
        api_key,
        prompt: "Say exactly: STREAMING_TEST_SUCCESS".to_string(),
        files: None,
    };
    let init_json = serde_json::to_string(&init_msg).unwrap() + "\n";

    if let Err(e) = stream.write_all(init_json.as_bytes()) {
        vm.stop();
        panic!("Failed to send init message: {}", e);
    }
    println!("Init message sent");

    // Read streaming output
    let results = match read_streaming_output(&mut stream, Duration::from_secs(120)) {
        Ok(r) => r,
        Err(e) => {
            vm.stop();
            panic!("Failed to read streaming output: {}", e);
        }
    };

    // Verify results
    println!("\n=== Verification ===\n");

    // Check that we got the expected event types
    assert!(
        results.got_system_init,
        "Should have received system init event"
    );

    assert!(
        results.got_result,
        "Should have received result event"
    );

    // Check for our test string in the output
    let all_output = results.all_output.join("\n");
    let has_success_marker = all_output.contains("STREAMING_TEST_SUCCESS")
        || results.final_result.as_ref().map(|r| r.contains("STREAMING_TEST_SUCCESS")).unwrap_or(false);

    if !has_success_marker {
        println!("Warning: Test marker not found in output");
        println!("Final result: {:?}", results.final_result);
        // Don't fail the test if Claude didn't follow instructions exactly
        // The important thing is that streaming worked
    }

    // Verify no critical errors
    assert!(
        results.errors.is_empty(),
        "Should not have critical errors: {:?}",
        results.errors
    );

    println!("Claude streaming test PASSED!");
    println!("\nTest completed successfully!");
}

/// Test multi-turn conversation streaming
#[test]
fn test_claude_multiturn_streaming() {
    println!("\n=== Claude Code Multi-turn Streaming Test ===\n");

    if let Err(e) = check_prerequisites() {
        println!("Skipping test: {}", e);
        return;
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    println!("Starting Firecracker VM with vsock...");
    let mut vm = match TestVm::start(TEST_VM_IP) {
        Ok(vm) => vm,
        Err(e) => {
            panic!("Failed to start VM: {}", e);
        }
    };

    thread::sleep(Duration::from_secs(10));

    let mut stream = match connect_vsock(&vm.vsock_uds_path, Duration::from_secs(60)) {
        Ok(s) => s,
        Err(e) => {
            vm.stop();
            panic!("Failed to connect to vsock: {}", e);
        }
    };

    // First turn: establish context
    println!("Sending first message...");
    let init_msg = VsockMessage::Init {
        api_key: api_key.clone(),
        prompt: "Remember this number: 42".to_string(),
        files: None,
    };
    let init_json = serde_json::to_string(&init_msg).unwrap() + "\n";
    stream.write_all(init_json.as_bytes()).unwrap();

    // Read first response
    let results1 = read_streaming_output(&mut stream, Duration::from_secs(60)).unwrap();
    assert!(results1.got_result, "First turn should complete");

    // Second turn: test context retention
    println!("Sending follow-up message...");
    let input_msg = VsockMessage::Input {
        data: "What number did I ask you to remember?".to_string(),
    };
    let input_json = serde_json::to_string(&input_msg).unwrap() + "\n";
    stream.write_all(input_json.as_bytes()).unwrap();

    // Read second response
    let results2 = read_streaming_output(&mut stream, Duration::from_secs(60)).unwrap();
    assert!(results2.got_result, "Second turn should complete");

    // Check if Claude remembered the number
    let all_output = results2.all_output.join("\n");
    let remembered = all_output.contains("42")
        || results2.final_result.as_ref().map(|r| r.contains("42")).unwrap_or(false);

    if remembered {
        println!("Claude correctly remembered the context!");
    } else {
        println!("Warning: Context may not have been retained");
    }

    println!("Multi-turn streaming test PASSED!");
}

/// Comprehensive end-to-end test with long conversation, file operations, and tool usage
#[test]
fn test_claude_comprehensive_conversation() {
    println!("\n=== Claude Code Comprehensive Conversation Test ===\n");
    println!("This test exercises:");
    println!("  - 5+ turn conversation with context retention");
    println!("  - File upload and reading");
    println!("  - Tool usage (bash commands, file creation)");
    println!("  - Error handling\n");

    if let Err(e) = check_prerequisites() {
        println!("Skipping test: {}", e);
        return;
    }

    let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap();

    println!("Starting Firecracker VM with vsock...");
    let mut vm = match TestVm::start(TEST_VM_IP) {
        Ok(vm) => vm,
        Err(e) => {
            panic!("Failed to start VM: {}", e);
        }
    };

    println!("Waiting for Debian to boot (30s)...");
    thread::sleep(Duration::from_secs(30));

    let mut stream = match connect_vsock(&vm.vsock_uds_path, Duration::from_secs(60)) {
        Ok(s) => s,
        Err(e) => {
            vm.stop();
            panic!("Failed to connect to vsock: {}", e);
        }
    };

    // Track conversation state
    let mut turn_count = 0;
    let mut conversation_succeeded = true;
    let mut errors: Vec<String> = Vec::new();

    // Helper to send input and read response
    let send_and_receive = |stream: &mut UnixStream, msg: VsockMessage, turn: u32| -> Result<StreamingResults, String> {
        let json = serde_json::to_string(&msg).unwrap() + "\n";
        stream.write_all(json.as_bytes())
            .map_err(|e| format!("Turn {}: Failed to send message: {}", turn, e))?;
        println!("\n--- Turn {} sent ---", turn);

        read_streaming_output(stream, Duration::from_secs(120))
            .map_err(|e| format!("Turn {}: Failed to read response: {}", turn, e))
    };

    // ============================================
    // Turn 1: Initialize with files
    // ============================================
    println!("\n========== TURN 1: Initialize with files ==========");
    turn_count += 1;

    let test_files = vec![
        TaskFile {
            name: "data.json".to_string(),
            content: r#"{"name": "test-project", "version": "1.0.0", "items": [1, 2, 3]}"#.to_string(),
        },
        TaskFile {
            name: "config.txt".to_string(),
            content: "setting1=value1\nsetting2=value2\ndebug=false".to_string(),
        },
        TaskFile {
            name: "src/main.py".to_string(),
            content: "def hello():\n    print('Hello, World!')\n\nif __name__ == '__main__':\n    hello()".to_string(),
        },
    ];

    let init_msg = VsockMessage::Init {
        api_key: api_key.clone(),
        prompt: "I've uploaded some files. Please list what files you see in /workspace and briefly describe what each one contains. Be concise.".to_string(),
        files: Some(test_files),
    };

    match send_and_receive(&mut stream, init_msg, turn_count) {
        Ok(results) => {
            if !results.got_result {
                errors.push(format!("Turn {}: No result received", turn_count));
                conversation_succeeded = false;
            }
            let output = results.all_output.join("\n");
            // Check if Claude acknowledged the files
            if !output.contains("data.json") && !output.contains("config") {
                println!("Warning: Claude may not have listed the files");
            }
            println!("Turn {} complete: got_result={}", turn_count, results.got_result);
        }
        Err(e) => {
            errors.push(e);
            conversation_succeeded = false;
        }
    }

    // ============================================
    // Turn 2: Read and analyze a file
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 2: Read and analyze file ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Read the data.json file and tell me: what is the project name and how many items are in the array?".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                let output = results.all_output.join("\n");
                // Check if Claude found the data
                let found_name = output.contains("test-project");
                let found_count = output.contains("3") || output.contains("three");
                if !found_name || !found_count {
                    println!("Warning: Claude may not have correctly read data.json");
                    println!("  Found project name: {}, Found item count: {}", found_name, found_count);
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 3: Create a new file using bash/tools
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 3: Create a new file ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Create a new file called 'output.txt' in /workspace with the content 'COMPREHENSIVE_TEST_MARKER_12345'. Use bash or the write tool.".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 4: Verify the file was created
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 4: Verify file creation ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Now read output.txt and confirm it contains the marker. Also run 'ls -la /workspace' to show all files.".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                let output = results.all_output.join("\n");
                if output.contains("COMPREHENSIVE_TEST_MARKER_12345") {
                    println!("SUCCESS: File was created and contains the marker!");
                } else {
                    println!("Warning: Marker not found in output - file may not have been created correctly");
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 5: Context retention test
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 5: Context retention ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Without reading any files again, from our earlier conversation: what was the project name in data.json and what marker did you write to output.txt?".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                let output = results.all_output.join("\n");
                let remembered_name = output.contains("test-project");
                let remembered_marker = output.contains("COMPREHENSIVE_TEST_MARKER") || output.contains("12345");

                if remembered_name && remembered_marker {
                    println!("SUCCESS: Context was retained across turns!");
                } else {
                    println!("Warning: Context retention may be incomplete");
                    println!("  Remembered project name: {}", remembered_name);
                    println!("  Remembered marker: {}", remembered_marker);
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 6: Modify existing file
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 6: Modify existing file ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Edit config.txt to change 'debug=false' to 'debug=true'. Then show me the updated contents.".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                let output = results.all_output.join("\n");
                if output.contains("debug=true") {
                    println!("SUCCESS: File was modified correctly!");
                } else {
                    println!("Warning: File modification may not have worked");
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 7: Run a bash command
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 7: Run bash command ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Run 'python3 src/main.py' and show me the output.".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                    conversation_succeeded = false;
                }
                let output = results.all_output.join("\n");
                if output.contains("Hello") || output.contains("World") {
                    println!("SUCCESS: Python script executed correctly!");
                } else {
                    println!("Warning: Python output not found (may have worked differently)");
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
                conversation_succeeded = false;
            }
        }
    }

    // ============================================
    // Turn 8: Summarize the conversation
    // ============================================
    if conversation_succeeded {
        println!("\n========== TURN 8: Conversation summary ==========");
        turn_count += 1;

        let input_msg = VsockMessage::Input {
            data: "Give me a brief summary of everything we did in this conversation. List each action in one line.".to_string(),
        };

        match send_and_receive(&mut stream, input_msg, turn_count) {
            Ok(results) => {
                if !results.got_result {
                    errors.push(format!("Turn {}: No result received", turn_count));
                }
                println!("Turn {} complete", turn_count);
            }
            Err(e) => {
                errors.push(e);
            }
        }
    }

    // ============================================
    // Final verification
    // ============================================
    println!("\n========================================");
    println!("         TEST RESULTS SUMMARY          ");
    println!("========================================\n");

    println!("Total turns completed: {}", turn_count);
    println!("Conversation succeeded: {}", conversation_succeeded);

    if !errors.is_empty() {
        println!("\nErrors encountered:");
        for error in &errors {
            println!("  - {}", error);
        }
    }

    // Cleanup
    vm.stop();

    // Assertions
    assert!(turn_count >= 5, "Should complete at least 5 turns, got {}", turn_count);
    assert!(conversation_succeeded, "Conversation should succeed without critical errors");
    assert!(errors.is_empty(), "Should have no errors: {:?}", errors);

    println!("\n========================================");
    println!("  COMPREHENSIVE CONVERSATION TEST PASSED!");
    println!("========================================\n");
}
