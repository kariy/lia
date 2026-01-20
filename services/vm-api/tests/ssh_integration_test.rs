//! Integration test for SSH connectivity to Firecracker VMs
//!
//! This test requires:
//! - Running as root (for TAP device creation)
//! - Firecracker binary installed
//! - Kernel and rootfs available
//! - Network bridge (lia-br0) configured
//!
//! Run with: sudo cargo test --test ssh_integration_test -- --nocapture

use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;
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
const TEST_VM_IP: &str = "172.16.0.250";
const TEST_TAP_NAME: &str = "tap-sshtest";

/// Check if all prerequisites are available
fn check_prerequisites() -> Result<(), String> {
    // Check if running as root
    if !is_root() {
        return Err("This test must be run as root".to_string());
    }

    // Check Firecracker binary
    if !PathBuf::from(FIRECRACKER_BIN).exists() {
        return Err(format!("Firecracker binary not found at {}", FIRECRACKER_BIN));
    }

    // Check kernel
    if !PathBuf::from(KERNEL_PATH).exists() {
        return Err(format!("Kernel not found at {}", KERNEL_PATH));
    }

    // Check rootfs
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

    Ok(())
}

/// Generate an SSH key pair for testing
fn generate_ssh_keypair() -> Result<(String, String), String> {
    let temp_dir = tempfile::tempdir().map_err(|e| format!("Failed to create temp dir: {}", e))?;
    let key_path = temp_dir.path().join("test_key");
    let key_path_str = key_path.to_string_lossy();

    // Generate key pair using ssh-keygen
    let output = Command::new("ssh-keygen")
        .args([
            "-t",
            "ed25519",
            "-f",
            &key_path_str,
            "-N",
            "", // No passphrase
            "-q",
        ])
        .output()
        .map_err(|e| format!("Failed to generate SSH key: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "ssh-keygen failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    // Read private key
    let private_key = fs::read_to_string(&key_path)
        .map_err(|e| format!("Failed to read private key: {}", e))?;

    // Read public key
    let pub_key_path = format!("{}.pub", key_path_str);
    let public_key =
        fs::read_to_string(&pub_key_path).map_err(|e| format!("Failed to read public key: {}", e))?;

    Ok((private_key, public_key.trim().to_string()))
}

/// Create a TAP device and attach it to the bridge
fn create_tap_device(tap_name: &str) -> Result<(), String> {
    // Delete if exists
    let _ = Command::new("ip")
        .args(["link", "delete", tap_name])
        .output();

    // Create TAP device
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

    // Bring it up
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

    // Attach to bridge
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

/// Delete the TAP device
fn delete_tap_device(tap_name: &str) {
    let _ = Command::new("ip")
        .args(["link", "set", tap_name, "down"])
        .output();
    let _ = Command::new("ip")
        .args(["link", "delete", tap_name])
        .output();
}

/// Generate MAC address from IP last octet
fn generate_mac(ip: &str) -> String {
    let last_octet: u8 = ip.split('.').last().unwrap().parse().unwrap_or(100);
    format!("02:FC:00:00:00:{:02X}", last_octet)
}

/// Firecracker VM configuration structures
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

    // Check for error in response
    let response = String::from_utf8_lossy(&output.stdout);
    if response.contains("fault_message") {
        return Err(format!("Firecracker error: {}", response));
    }

    Ok(())
}

/// Start a Firecracker VM with SSH access
struct TestVm {
    socket_path: PathBuf,
    rootfs_copy: PathBuf,
    process: std::process::Child,
    tap_name: String,
}

impl TestVm {
    fn start(ssh_public_key: &str, vm_ip: &str) -> Result<Self, String> {
        let test_id = format!("ssh-test-{}", std::process::id());
        let socket_path = PathBuf::from(format!("/tmp/{}.sock", test_id));
        let rootfs_copy = PathBuf::from(format!("/tmp/{}-rootfs.ext4", test_id));
        let log_path = PathBuf::from(format!("/tmp/{}.log", test_id));

        // Clean up any existing socket
        let _ = fs::remove_file(&socket_path);

        // Copy rootfs (each VM needs its own writable copy)
        println!("Copying rootfs...");
        fs::copy(ROOTFS_PATH, &rootfs_copy)
            .map_err(|e| format!("Failed to copy rootfs: {}", e))?;

        // Create TAP device
        println!("Creating TAP device {}...", TEST_TAP_NAME);
        create_tap_device(TEST_TAP_NAME)?;

        // Start Firecracker process
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

        // Wait for socket to be ready
        println!("Waiting for Firecracker socket...");
        let start = Instant::now();
        while !socket_path.exists() {
            if start.elapsed() > Duration::from_secs(10) {
                return Err("Timeout waiting for Firecracker socket".to_string());
            }
            thread::sleep(Duration::from_millis(100));
        }
        thread::sleep(Duration::from_millis(200)); // Extra time for socket to be ready

        let socket_path_str = socket_path.to_string_lossy().to_string();

        // Configure the VM
        println!("Configuring VM...");

        // Encode SSH key for kernel command line (replace spaces with +)
        let ssh_key_encoded = ssh_public_key.replace(' ', "+");

        // Boot source with network config
        let boot_args = format!(
            "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/init lia.ip={} lia.gateway={} lia.ssh_key={}",
            vm_ip, BRIDGE_IP, ssh_key_encoded
        );

        fc_put(
            &socket_path_str,
            "/boot-source",
            &BootSource {
                kernel_image_path: KERNEL_PATH.to_string(),
                boot_args,
            },
        )?;

        // Machine config
        fc_put(
            &socket_path_str,
            "/machine-config",
            &MachineConfig {
                vcpu_count: 2,
                mem_size_mib: 512,
            },
        )?;

        // Root drive
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

        // Start the VM
        println!("Starting VM instance...");
        fc_put(
            &socket_path_str,
            "/actions",
            &InstanceActionInfo {
                action_type: "InstanceStart".to_string(),
            },
        )?;

        Ok(TestVm {
            socket_path,
            rootfs_copy,
            process,
            tap_name: TEST_TAP_NAME.to_string(),
        })
    }

    fn stop(&mut self) {
        println!("Stopping VM...");

        // Kill the Firecracker process
        let _ = self.process.kill();
        let _ = self.process.wait();

        // Clean up files
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.rootfs_copy);

        // Delete TAP device
        delete_tap_device(&self.tap_name);
    }
}

impl Drop for TestVm {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Wait for SSH to become available
fn wait_for_ssh(ip: &str, timeout: Duration) -> Result<(), String> {
    println!("Waiting for SSH to become available on {}...", ip);
    let start = Instant::now();

    while start.elapsed() < timeout {
        match TcpStream::connect_timeout(
            &format!("{}:22", ip).parse().unwrap(),
            Duration::from_secs(2),
        ) {
            Ok(_) => {
                println!("SSH port is open!");
                // Give sshd a moment to fully initialize
                thread::sleep(Duration::from_secs(1));
                return Ok(());
            }
            Err(_) => {
                thread::sleep(Duration::from_secs(1));
            }
        }
    }

    Err(format!(
        "Timeout waiting for SSH on {} after {:?}",
        ip, timeout
    ))
}

/// Test SSH connection using the ssh2 crate
fn test_ssh_connection(ip: &str, private_key: &str) -> Result<String, String> {
    println!("Connecting via SSH to {}...", ip);

    // Connect TCP
    let tcp = TcpStream::connect(format!("{}:22", ip))
        .map_err(|e| format!("TCP connection failed: {}", e))?;

    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("Failed to set read timeout: {}", e))?;

    // Create SSH session
    let mut session = Session::new().map_err(|e| format!("Failed to create SSH session: {}", e))?;

    session.set_tcp_stream(tcp);
    session
        .handshake()
        .map_err(|e| format!("SSH handshake failed: {}", e))?;

    // Authenticate with private key
    println!("Authenticating with private key...");
    session
        .userauth_pubkey_memory("root", None, private_key, None)
        .map_err(|e| format!("SSH authentication failed: {}", e))?;

    if !session.authenticated() {
        return Err("SSH authentication failed".to_string());
    }

    println!("SSH authenticated successfully!");

    // Execute a test command
    println!("Executing test command...");
    let mut channel = session
        .channel_session()
        .map_err(|e| format!("Failed to open channel: {}", e))?;

    channel
        .exec("echo 'SSH_TEST_SUCCESS' && hostname && uname -a")
        .map_err(|e| format!("Failed to execute command: {}", e))?;

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .map_err(|e| format!("Failed to read output: {}", e))?;

    channel.wait_close().ok();

    Ok(output)
}

#[test]
fn test_firecracker_ssh_connectivity() {
    println!("\n=== Firecracker SSH Integration Test ===\n");

    // Check prerequisites
    if let Err(e) = check_prerequisites() {
        println!("Skipping test: {}", e);
        println!("\nTo run this test:");
        println!("  1. Run as root: sudo cargo test --test ssh_integration_test -- --nocapture");
        println!("  2. Ensure vm/setup.sh has been run");
        println!("  3. Ensure rootfs has been built with SSH support");
        return;
    }

    // Generate SSH key pair
    println!("Generating SSH key pair...");
    let (private_key, public_key) = match generate_ssh_keypair() {
        Ok(keys) => keys,
        Err(e) => {
            panic!("Failed to generate SSH key pair: {}", e);
        }
    };
    println!("SSH key pair generated");

    // Start VM
    println!("\nStarting Firecracker VM...");
    let mut vm = match TestVm::start(&public_key, TEST_VM_IP) {
        Ok(vm) => vm,
        Err(e) => {
            panic!("Failed to start VM: {}", e);
        }
    };
    println!("VM started");

    // Wait for SSH to become available (up to 60 seconds for boot)
    if let Err(e) = wait_for_ssh(TEST_VM_IP, Duration::from_secs(60)) {
        vm.stop();
        panic!("SSH not available: {}", e);
    }

    // Test SSH connection
    match test_ssh_connection(TEST_VM_IP, &private_key) {
        Ok(output) => {
            println!("\n=== SSH Command Output ===");
            println!("{}", output);
            println!("=========================\n");

            // Verify the output contains our test string
            assert!(
                output.contains("SSH_TEST_SUCCESS"),
                "Expected 'SSH_TEST_SUCCESS' in output"
            );

            println!("SSH connectivity test PASSED!");
        }
        Err(e) => {
            vm.stop();
            panic!("SSH test failed: {}", e);
        }
    }

    // Cleanup is handled by Drop
    println!("\nTest completed successfully!");
}

/// Network test VM helper struct for cleanup
struct NetworkTestVm {
    process: std::process::Child,
    socket_path: PathBuf,
    rootfs_copy: PathBuf,
    tap_name: String,
}

impl NetworkTestVm {
    fn cleanup(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        let _ = fs::remove_file(&self.socket_path);
        let _ = fs::remove_file(&self.rootfs_copy);
        delete_tap_device(&self.tap_name);
    }
}

impl Drop for NetworkTestVm {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Additional test: verify network connectivity from inside VM
/// This test uses SSH to run ping from inside the VM
#[test]
fn test_vm_network_connectivity() {
    println!("\n=== Firecracker Network Connectivity Test ===\n");

    // Check prerequisites
    if let Err(e) = check_prerequisites() {
        println!("Skipping test: {}", e);
        return;
    }

    // Generate SSH key pair
    let (private_key, public_key) = match generate_ssh_keypair() {
        Ok(keys) => keys,
        Err(e) => {
            panic!("Failed to generate SSH key pair: {}", e);
        }
    };

    // Use a different IP and TAP for this test
    let test_ip = "172.16.0.251";
    let tap_name = "tap-nettest";

    // Clean up any leftover TAP device
    delete_tap_device(tap_name);

    // Create TAP for this test
    if let Err(e) = create_tap_device(tap_name) {
        panic!("Failed to create TAP device: {}", e);
    }

    // Start VM with custom settings
    println!("Starting VM for network test...");
    let test_id = format!("net-test-{}", std::process::id());
    let socket_path = PathBuf::from(format!("/tmp/{}.sock", test_id));
    let rootfs_copy = PathBuf::from(format!("/tmp/{}-rootfs.ext4", test_id));
    let log_path = PathBuf::from(format!("/tmp/{}.log", test_id));

    // Clean up any existing socket
    let _ = fs::remove_file(&socket_path);

    // Copy rootfs
    if let Err(e) = fs::copy(ROOTFS_PATH, &rootfs_copy) {
        delete_tap_device(tap_name);
        panic!("Failed to copy rootfs: {}", e);
    }

    // Start Firecracker
    let process = match Command::new(FIRECRACKER_BIN)
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
    {
        Ok(p) => p,
        Err(e) => {
            delete_tap_device(tap_name);
            let _ = fs::remove_file(&rootfs_copy);
            panic!("Failed to start Firecracker: {}", e);
        }
    };

    // Create VM struct for cleanup (Drop will handle cleanup)
    let vm = NetworkTestVm {
        process,
        socket_path: socket_path.clone(),
        rootfs_copy: rootfs_copy.clone(),
        tap_name: tap_name.to_string(),
    };

    // Wait for socket
    let start = Instant::now();
    while !socket_path.exists() {
        if start.elapsed() > Duration::from_secs(10) {
            panic!("Timeout waiting for Firecracker socket");
        }
        thread::sleep(Duration::from_millis(100));
    }
    thread::sleep(Duration::from_millis(200));

    let socket_path_str = socket_path.to_string_lossy().to_string();
    let ssh_key_encoded = public_key.replace(' ', "+");

    // Configure VM
    let boot_args = format!(
        "console=ttyS0 reboot=k panic=1 pci=off init=/sbin/init lia.ip={} lia.gateway={} lia.ssh_key={}",
        test_ip, BRIDGE_IP, ssh_key_encoded
    );

    fc_put(
        &socket_path_str,
        "/boot-source",
        &BootSource {
            kernel_image_path: KERNEL_PATH.to_string(),
            boot_args,
        },
    )
    .expect("Failed to configure boot source");

    fc_put(
        &socket_path_str,
        "/machine-config",
        &MachineConfig {
            vcpu_count: 2,
            mem_size_mib: 512,
        },
    )
    .expect("Failed to configure machine");

    fc_put(
        &socket_path_str,
        "/drives/rootfs",
        &Drive {
            drive_id: "rootfs".to_string(),
            path_on_host: rootfs_copy.to_string_lossy().to_string(),
            is_root_device: true,
            is_read_only: false,
        },
    )
    .expect("Failed to configure drive");

    let mac_address = generate_mac(test_ip);
    fc_put(
        &socket_path_str,
        "/network-interfaces/eth0",
        &NetworkInterface {
            iface_id: "eth0".to_string(),
            guest_mac: mac_address,
            host_dev_name: tap_name.to_string(),
        },
    )
    .expect("Failed to configure network");

    fc_put(
        &socket_path_str,
        "/actions",
        &InstanceActionInfo {
            action_type: "InstanceStart".to_string(),
        },
    )
    .expect("Failed to start VM");

    // Wait for SSH
    wait_for_ssh(test_ip, Duration::from_secs(60)).expect("SSH not available");

    // Test network connectivity via SSH
    println!("Testing network connectivity from inside VM...");

    // First verify basic SSH works
    test_ssh_connection(test_ip, &private_key).expect("SSH test failed");

    // Now test ping to the gateway
    let tcp = TcpStream::connect(format!("{}:22", test_ip)).expect("TCP connect failed");
    tcp.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("Set timeout failed");
    let mut session = Session::new().expect("Session creation failed");
    session.set_tcp_stream(tcp);
    session.handshake().expect("SSH handshake failed");
    session
        .userauth_pubkey_memory("root", None, &private_key, None)
        .expect("SSH auth failed");

    let mut channel = session.channel_session().expect("Channel open failed");
    channel
        .exec(&format!("ping -c 3 {} && echo PING_SUCCESS", BRIDGE_IP))
        .expect("Exec failed");

    let mut output = String::new();
    channel
        .read_to_string(&mut output)
        .expect("Read output failed");
    channel.wait_close().ok();

    println!("Ping output:\n{}", output);

    assert!(
        output.contains("PING_SUCCESS"),
        "Ping to gateway failed - output: {}",
        output
    );

    println!("Network connectivity test PASSED!");

    // Cleanup handled by Drop
    drop(vm);
    println!("\nNetwork test completed successfully!");
}
