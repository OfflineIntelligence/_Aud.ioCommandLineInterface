//! Network tools for connectivity testing and system operations.
//!
//! This module provides tools for:
//! - Network connectivity testing
//! - System network information
//! - Port availability checking
//! - Network diagnostics

use serde::Serialize;
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;
use tracing::debug;

/// Tool definition for network tools
#[derive(Debug, Clone, Serialize)]
pub struct NetworkToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: &'static str,
}

/// Get all network tool definitions
pub fn network_tool_definitions() -> Vec<NetworkToolDef> {
    vec![
        NetworkToolDef {
            name: "test_connectivity",
            description: "Test network connectivity to various services and hosts",
            parameters: r#"{"type":"object","properties":{"hosts":{"type":"array","items":{"type":"string"},"description":"Hosts to test connectivity to"},"timeout_seconds":{"type":"number","description":"Timeout for each connection test"}},"required":[]}"#,
        },
        NetworkToolDef {
            name: "check_port_availability",
            description: "Check if specific ports are available/open on localhost",
            parameters: r#"{"type":"object","properties":{"ports":{"type":"array","items":{"type":"number"},"description":"Ports to check"},"host":{"type":"string","description":"Host to check (default: localhost)"}},"required":["ports"]}"#,
        },
        NetworkToolDef {
            name: "get_network_info",
            description: "Get detailed network interface and configuration information",
            parameters: r#"{"type":"object","properties":{},"required":[]}"#,
        },
        NetworkToolDef {
            name: "dns_lookup",
            description: "Perform DNS lookup for hostnames",
            parameters: r#"{"type":"object","properties":{"hostname":{"type":"string","description":"Hostname to lookup"},"record_type":{"type":"string","description":"DNS record type (A, AAAA, MX, etc.)"}},"required":["hostname"]}"#,
        },
        NetworkToolDef {
            name: "network_diagnostics",
            description: "Run comprehensive network diagnostics",
            parameters: r#"{"type":"object","properties":{"verbose":{"type":"boolean","description":"Include detailed diagnostic information"}},"required":[]}"#,
        },
    ]
}

/// Result from network tool execution
#[derive(Debug, Serialize)]
pub struct NetworkToolResult {
    pub success: bool,
    pub output: String,
}

/// Execute a network tool by name with JSON params
pub fn execute_network_tool(
    name: &str,
    params: &serde_json::Value,
    _project_root: &Path,
) -> NetworkToolResult {
    debug!("Executing network tool '{}' with params: {}", name, params);
    
    match name {
        "test_connectivity" => tool_test_connectivity(params),
        "check_port_availability" => tool_check_port_availability(params),
        "get_network_info" => tool_get_network_info(),
        "dns_lookup" => tool_dns_lookup(params),
        "network_diagnostics" => tool_network_diagnostics(params),
        _ => NetworkToolResult {
            success: false,
            output: format!("Unknown network tool: {}", name),
        },
    }
}

/// Test network connectivity to various services
fn tool_test_connectivity(params: &serde_json::Value) -> NetworkToolResult {
    let hosts = params["hosts"].as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.to_string())
                .collect::<Vec<String>>()
        })
        .unwrap_or_else(|| {
            // Default hosts to test
            vec![
                "8.8.8.8:53".to_string(),      // Google DNS
                "1.1.1.1:53".to_string(),      // Cloudflare DNS
                "google.com:80".to_string(),   // Google HTTP
                "github.com:443".to_string(),  // GitHub HTTPS
            ]
        });
    
    let timeout_seconds = params["timeout_seconds"].as_f64().unwrap_or(5.0);
    let timeout = Duration::from_secs(timeout_seconds as u64);
    
    let mut output = String::new();
    output.push_str(&format!("🌐 Network Connectivity Test\n"));
    output.push_str(&format!("===========================\n\n"));
    output.push_str(&format!("Testing {} hosts with {}s timeout\n\n", hosts.len(), timeout_seconds));
    
    let mut successful = 0;
    let mut failed = 0;
    
    for host in &hosts {
        match test_single_connection(host, timeout) {
            Ok(latency) => {
                successful += 1;
                output.push_str(&format!("✅ {} - Connected ({:.2}ms)\n", host, latency));
            }
            Err(e) => {
                failed += 1;
                output.push_str(&format!("❌ {} - Failed: {}\n", host, e));
            }
        }
    }
    
    output.push_str(&format!("\n📊 Summary: {} successful, {} failed\n", successful, failed));
    
    if successful > 0 {
        let success_rate = (successful as f64 / hosts.len() as f64) * 100.0;
        output.push_str(&format!("Success Rate: {:.1}%\n", success_rate));
    }
    
    NetworkToolResult {
        success: successful > 0,
        output,
    }
}

/// Check port availability on localhost
fn tool_check_port_availability(params: &serde_json::Value) -> NetworkToolResult {
    let ports = match params["ports"].as_array() {
        Some(arr) => arr.iter().filter_map(|v| v.as_u64()).map(|n| n as u16).collect::<Vec<u16>>(),
        None => return NetworkToolResult {
            success: false,
            output: "Missing 'ports' parameter".into(),
        },
    };
    
    let host = params["host"].as_str().unwrap_or("localhost");
    
    let mut output = String::new();
    output.push_str(&format!("🔌 Port Availability Check\n"));
    output.push_str(&format!("=========================\n\n"));
    output.push_str(&format!("Checking {} ports on {}\n\n", ports.len(), host));
    
    let mut open_ports = Vec::new();
    let mut closed_ports = Vec::new();
    let mut error_ports = Vec::new();
    
    for &port in &ports {
        let addr = format!("{}:{}", host, port);
        match TcpStream::connect_timeout(&addr.parse().unwrap(), Duration::from_secs(3)) {
            Ok(_) => {
                open_ports.push(port);
                output.push_str(&format!("✅ Port {}: Open\n", port));
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::TimedOut || 
                   e.kind() == std::io::ErrorKind::ConnectionRefused {
                    closed_ports.push(port);
                    output.push_str(&format!("❌ Port {}: Closed\n", port));
                } else {
                    error_ports.push(port);
                    output.push_str(&format!("⚠️  Port {}: Error - {}\n", port, e));
                }
            }
        }
    }
    
    output.push_str(&format!("\n📊 Summary:\n"));
    output.push_str(&format!("  Open Ports: {} ({:?})\n", open_ports.len(), open_ports));
    output.push_str(&format!("  Closed Ports: {} ({:?})\n", closed_ports.len(), closed_ports));
    output.push_str(&format!("  Error Ports: {} ({:?})\n", error_ports.len(), error_ports));
    
    NetworkToolResult {
        success: true,
        output,
    }
}

/// Get detailed network information
fn tool_get_network_info() -> NetworkToolResult {
    let mut output = String::new();
    output.push_str(&format!("📡 Network Information\n"));
    output.push_str(&format!("=====================\n\n"));
    
    // Get hostname
    if let Ok(hostname) = get_hostname() {
        output.push_str(&format!("Hostname: {}\n", hostname));
    }
    
    // Get IP addresses
    output.push_str(&format!("\n🌐 IP Addresses:\n"));
    match get_local_ip_addresses() {
        Ok(ips) => {
            for ip in ips {
                output.push_str(&format!("  - {}\n", ip));
            }
        }
        Err(e) => output.push_str(&format!("  Failed to get IP addresses: {}\n", e)),
    }
    
    // Get network interfaces
    output.push_str(&format!("\n🔌 Network Interfaces:\n"));
    match get_network_interfaces() {
        Ok(interfaces) => {
            for interface in interfaces {
                output.push_str(&format!("  - {}\n", interface));
            }
        }
        Err(e) => output.push_str(&format!("  Failed to get interfaces: {}\n", e)),
    }
    
    // Get default gateway and DNS
    output.push_str(&format!("\n🧭 Routing Information:\n"));
    match get_routing_info() {
        Ok(info) => output.push_str(&info),
        Err(e) => output.push_str(&format!("  Failed to get routing info: {}\n", e)),
    }
    
    // Check internet connectivity
    output.push_str(&format!("\n🌍 Internet Connectivity:\n"));
    match check_internet_connectivity() {
        Ok(is_connected) => {
            output.push_str(&format!("  Status: {}\n", 
                if is_connected { "Connected" } else { "Disconnected" }));
        }
        Err(e) => output.push_str(&format!("  Check failed: {}\n", e)),
    }
    
    NetworkToolResult {
        success: true,
        output,
    }
}

/// Perform DNS lookup
fn tool_dns_lookup(params: &serde_json::Value) -> NetworkToolResult {
    let hostname = match params["hostname"].as_str() {
        Some(h) => h,
        None => return NetworkToolResult {
            success: false,
            output: "Missing 'hostname' parameter".into(),
        },
    };
    
    let record_type = params["record_type"].as_str().unwrap_or("A");
    
    let mut output = String::new();
    output.push_str(&format!("🔍 DNS Lookup\n"));
    output.push_str(&format!("=============\n\n"));
    output.push_str(&format!("Hostname: {}\n", hostname));
    output.push_str(&format!("Record Type: {}\n\n", record_type));
    
    match perform_dns_lookup(hostname, record_type) {
        Ok(results) => {
            output.push_str(&format!("Results ({} records):\n", results.len()));
            for result in results {
                output.push_str(&format!("  - {}\n", result));
            }
        }
        Err(e) => {
            output.push_str(&format!("DNS lookup failed: {}\n", e));
        }
    }
    
    NetworkToolResult {
        success: true,
        output,
    }
}

/// Run comprehensive network diagnostics
fn tool_network_diagnostics(params: &serde_json::Value) -> NetworkToolResult {
    let verbose = params["verbose"].as_bool().unwrap_or(false);
    
    let mut output = String::new();
    output.push_str(&format!("🏥 Network Diagnostics\n"));
    output.push_str(&format!("======================\n\n"));
    
    // Run connectivity tests
    output.push_str(&format!("1. Connectivity Tests:\n"));
    output.push_str(&format!("---------------------\n"));
    let connectivity_params = serde_json::json!({
        "hosts": ["8.8.8.8:53", "1.1.1.1:53", "google.com:80"],
        "timeout_seconds": 5.0
    });
    let connectivity_result = tool_test_connectivity(&connectivity_params);
    output.push_str(&format!("{}\n\n", connectivity_result.output.lines().skip(3).collect::<Vec<_>>().join("\n")));
    
    // Check common ports
    output.push_str(&format!("2. Port Availability:\n"));
    output.push_str(&format!("--------------------\n"));
    let port_params = serde_json::json!({
        "ports": [22, 80, 443, 3389, 5432],
        "host": "localhost"
    });
    let port_result = tool_check_port_availability(&port_params);
    output.push_str(&format!("{}\n\n", port_result.output.lines().skip(3).collect::<Vec<_>>().join("\n")));
    
    // Get network info
    output.push_str(&format!("3. Network Configuration:\n"));
    output.push_str(&format!("------------------------\n"));
    let network_result = tool_get_network_info();
    if verbose {
        output.push_str(&format!("{}\n", network_result.output.lines().skip(3).collect::<Vec<_>>().join("\n")));
    } else {
        // Just show summary
        output.push_str(&format!("Network information retrieved successfully\n"));
    }
    
    // Overall assessment
    output.push_str(&format!("\n📋 Diagnostic Summary:\n"));
    output.push_str(&format!("=====================\n"));
    output.push_str(&format!("Connectivity: {}\n", 
        if connectivity_result.success { "✅ Good" } else { "❌ Poor" }));
    output.push_str(&format!("Local Services: Checked\n"));
    output.push_str(&format!("Configuration: Retrieved\n"));
    
    NetworkToolResult {
        success: true,
        output,
    }
}

// Helper functions

fn test_single_connection(host: &str, timeout: Duration) -> Result<f64, String> {
    let start = std::time::Instant::now();
    
    match TcpStream::connect_timeout(&host.parse::<std::net::SocketAddr>().map_err(|e| e.to_string())?, timeout) {
        Ok(_) => {
            let elapsed = start.elapsed().as_micros() as f64 / 1000.0;
            Ok(elapsed)
        }
        Err(e) => Err(e.to_string()),
    }
}

fn get_hostname() -> Result<String, String> {
    hostname::get()
        .map_err::<String, _>(|e| e.to_string())?
        .into_string()
        .map_err(|_| "Invalid hostname".to_string())
}

fn get_local_ip_addresses() -> Result<Vec<String>, String> {
    let mut ips = Vec::new();
    
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("hostname")
            .arg("-I")
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            ips.extend(stdout.trim().split_whitespace().map(|s| s.to_string()));
        }
    }
    
    #[cfg(windows)]
    {
        use std::process::Command;
        let output = Command::new("ipconfig")
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Simple regex to extract IPv4 addresses
            let re = regex::Regex::new(r"IPv4 Address[\. ]+: ([\d\.]+)").unwrap();
            for cap in re.captures_iter(&stdout) {
                if let Some(ip) = cap.get(1) {
                    ips.push(ip.as_str().to_string());
                }
            }
        }
    }
    
    Ok(ips)
}

fn get_network_interfaces() -> Result<Vec<String>, String> {
    let mut interfaces = Vec::new();
    
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("ip")
            .args(&["link", "show"])
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains(": ") && !line.contains("loopback") {
                    if let Some(name) = line.split(':').nth(1) {
                        interfaces.push(name.trim().to_string());
                    }
                }
            }
        }
    }
    
    #[cfg(windows)]
    {
        use std::process::Command;
        let output = Command::new("netsh")
            .args(&["interface", "show", "interface"])
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines().skip(3) { // Skip header lines
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 4 {
                    interfaces.push(parts[3].to_string()); // Interface name is typically the last column
                }
            }
        }
    }
    
    Ok(interfaces)
}

fn get_routing_info() -> Result<String, String> {
    let mut info = String::new();
    
    #[cfg(unix)]
    {
        use std::process::Command;
        let output = Command::new("ip")
            .args(&["route", "show", "default"])
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            info.push_str(&format!("  Default Gateway: {}\n", stdout.trim()));
        }
    }
    
    #[cfg(windows)]
    {
        use std::process::Command;
        let output = Command::new("route")
            .args(&["print", "0.0.0.0"])
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Extract default gateway from route table
            for line in stdout.lines() {
                if line.contains("0.0.0.0") && line.contains("0.0.0.0") {
                    let parts: Vec<&str> = line.split_whitespace().collect();
                    if parts.len() >= 4 {
                        info.push_str(&format!("  Default Gateway: {}\n", parts[3]));
                        break;
                    }
                }
            }
        }
    }
    
    // Add DNS servers
    info.push_str(&format!("  DNS Servers: "));
    #[cfg(unix)]
    {
        if let Ok(resolv_conf) = std::fs::read_to_string("/etc/resolv.conf") {
            let mut dns_servers = Vec::new();
            for line in resolv_conf.lines() {
                if line.starts_with("nameserver") {
                    if let Some(server) = line.split_whitespace().nth(1) {
                        dns_servers.push(server.to_string());
                    }
                }
            }
            info.push_str(&dns_servers.join(", "));
        }
    }
    
    #[cfg(windows)]
    {
        info.push_str("System DNS configuration");
    }
    
    info.push('\n');
    Ok(info)
}

fn check_internet_connectivity() -> Result<bool, String> {
    // Test connectivity to reliable public DNS servers
    let test_hosts = ["8.8.8.8:53", "1.1.1.1:53"];
    
    for host in &test_hosts {
        if test_single_connection(host, Duration::from_secs(3)).is_ok() {
            return Ok(true);
        }
    }
    
    Ok(false)
}

fn perform_dns_lookup(hostname: &str, _record_type: &str) -> Result<Vec<String>, String> {
    use std::process::Command;
    
    let mut results = Vec::new();
    
    #[cfg(unix)]
    {
        let output = match record_type {
            "A" => Command::new("dig")
                .args(&[hostname, "+short"])
                .output(),
            "AAAA" => Command::new("dig")
                .args(&[hostname, "AAAA", "+short"])
                .output(),
            "MX" => Command::new("dig")
                .args(&[hostname, "MX", "+short"])
                .output(),
            _ => Command::new("nslookup")
                .arg(hostname)
                .output(),
        }.map_err(|e| e.to_string())?;
        
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            results.extend(stdout.lines().map(|s| s.to_string()).filter(|s| !s.is_empty()));
        }
    }
    
    #[cfg(windows)]
    {
        let output = Command::new("nslookup")
            .arg(hostname)
            .output()
            .map_err(|e| e.to_string())?;
            
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse nslookup output
            for line in stdout.lines() {
                if line.contains("Address:") && !line.contains("Server:") {
                    if let Some(addr) = line.split("Address:").nth(1) {
                        let clean_addr = addr.trim();
                        if !clean_addr.is_empty() && !clean_addr.contains(":") { // Skip IPv6 for simplicity
                            results.push(clean_addr.to_string());
                        }
                    }
                }
            }
        }
    }
    
    if results.is_empty() {
        Err("No DNS records found".to_string())
    } else {
        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_network_tool_definitions() {
        let tools = network_tool_definitions();
        assert!(!tools.is_empty());
        assert!(tools.iter().any(|t| t.name == "test_connectivity"));
    }
}