mod handler;
mod herdr;
mod html;
mod inject;
mod pidfile;
mod state;

use std::net::{SocketAddr, UdpSocket};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::Arc;

use inject::HerdrInjector;
use state::{AppState, PORT};

fn get_local_ip() -> String {
    UdpSocket::bind("0.0.0.0:0")
        .and_then(|s| {
            s.connect("8.8.8.8:80")?;
            Ok(s.local_addr()?.ip().to_string())
        })
        .unwrap_or_else(|_| "127.0.0.1".to_string())
}

fn find_port_listeners(port: u16) -> Vec<u32> {
    let output = Command::new("ss")
        .args(["-tlnp", &format!("sport = :{port}")])
        .output();
    let Ok(output) = output else { return vec![] };
    let text = String::from_utf8_lossy(&output.stdout);
    let mut pids = vec![];
    for line in text.lines() {
        if let Some(start) = line.find("pid=") {
            let rest = &line[start + 4..];
            if let Some(end) = rest.find(|c: char| !c.is_ascii_digit()) {
                if let Ok(pid) = rest[..end].parse::<u32>() {
                    pids.push(pid);
                }
            }
        }
    }
    pids.sort();
    pids.dedup();
    pids
}

fn print_qr(url: &str) {
    use qrcode::{QrCode, Color};
    let code = match QrCode::new(url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] QR code generation failed: {e}");
            return;
        }
    };
    let width = code.width();
    let colors: Vec<bool> = code.into_colors().iter().map(|c| *c == Color::Dark).collect();
    let rows: Vec<&[bool]> = colors.chunks(width).collect();

    for pair in rows.chunks(2) {
        for col in 0..width {
            let top = pair[0][col];
            let bot = pair.get(1).map_or(false, |r| r[col]);
            let ch = match (top, bot) {
                (true, true) => "\u{2588}",
                (true, false) => "\u{2580}",
                (false, true) => "\u{2584}",
                (false, false) => " ",
            };
            print!("{ch}");
        }
        println!();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("serve");

    match cmd {
        "serve" => cmd_serve(),
        "qr" => cmd_qr(false),
        "qr-tunnel" => cmd_qr(true),
        "stop" => cmd_stop(),
        "status" => cmd_status(),
        other => {
            eprintln!("Unknown command: {other}");
            eprintln!("Usage: herdr-agents-bridge [serve|qr|qr-tunnel|stop|status]");
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn cmd_serve() {
    let local_ip = get_local_ip();
    let state = Arc::new(AppState::new(Box::new(HerdrInjector)));
    let ui_url = format!("http://{local_ip}:{PORT}/?t={}", state.session_token);

    let addr = SocketAddr::from(([0, 0, 0, 0], PORT));
    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(_) => {
            let pids = find_port_listeners(PORT);
            let pid_str = if pids.is_empty() {
                String::new()
            } else {
                format!(
                    " (PID: {})",
                    pids.iter()
                        .map(|p| p.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            eprintln!("[ERROR] port {PORT} is already in use{pid_str}");
            std::process::exit(1);
        }
    };

    let pid = std::process::id();
    if let Err(e) = pidfile::write(pid, &ui_url) {
        eprintln!("[WARN] failed to write PID/URL files: {e}");
    }

    eprintln!("[herdr-agents-bridge] started on port {PORT} (PID {pid})");

    let app = handler::build_router(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    pidfile::cleanup();
    eprintln!("[herdr-agents-bridge] stopped");
}

fn ensure_server() {
    if pidfile::is_running() {
        if check_server_health() {
            return;
        }
        eprintln!("[herdr-agents-bridge] server PID exists but not responding, restarting...");
        stop_all();
    }
    eprintln!("[herdr-agents-bridge] starting server...");

    let log_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join("herdr-agents-bridge");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("server.log");
    let log_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_path)
        .expect("failed to open server log");

    let exe = std::env::current_exe().expect("failed to get executable path");
    Command::new(&exe)
        .arg("serve")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log_file)
        .process_group(0)
        .spawn()
        .expect("failed to start server");

    for i in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        if pidfile::read_url().is_some() && check_server_health() {
            return;
        }
        if i == 49 {
            eprintln!("[WARN] server may not be ready (check {})", log_path.display());
        }
    }
}

fn cmd_status() {
    let server_pid = pidfile::read_pid();
    let server_running = server_pid.map_or(false, |pid| {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    });
    let server_healthy = server_running && check_server_health();

    println!("  Server:  {}", if server_healthy {
        "running"
    } else if server_running {
        "running (not responding)"
    } else {
        "stopped"
    });

    let local_url = pidfile::read_url();
    let tunnel_url = pidfile::read_tunnel_url();

    let tunnel_pid = pidfile::read_tunnel_pid();
    let tunnel_running = tunnel_pid.map_or(false, |pid| {
        std::path::Path::new(&format!("/proc/{pid}")).exists()
    });

    if tunnel_url.is_some() {
        if tunnel_running {
            println!("  Tunnel:  running");
        } else {
            println!("  Tunnel:  stopped");
        }
    } else {
        println!("  Tunnel:  not started");
    }

    let display_url = match (&tunnel_url, &local_url) {
        (Some(tun), Some(local)) if tunnel_running => {
            let token = local.split("?t=").nth(1).unwrap_or("");
            Some(format!("{tun}/?t={token}"))
        }
        (_, Some(local)) => Some(local.clone()),
        _ => None,
    };

    if let Some(url) = &display_url {
        println!();
        if tunnel_url.is_some() && tunnel_running {
            println!("  Tunnel (remote):");
        } else {
            println!("  Local network only:");
        }
        println!("  {url}");
        println!();
        print_qr(url);
    } else {
        println!();
        println!("  No URL available (server not running?)");
    }
}

fn check_server_health() -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], PORT)),
        std::time::Duration::from_millis(200),
    )
    .is_ok()
}

fn ensure_tunnel() {
    if pidfile::read_tunnel_url().is_some() {
        return;
    }
    eprintln!("[herdr-agents-bridge] starting tunnel...");

    let log_path = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::path::PathBuf::from("/tmp"))
        .join("herdr-agents-bridge")
        .join("tunnel.log");

    let log_file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&log_path)
        .expect("failed to open tunnel log");

    let child = Command::new("cloudflared")
        .args(["tunnel", "--url", &format!("http://localhost:{PORT}")])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log_file)
        .process_group(0)
        .spawn()
        .expect("failed to start cloudflared");

    let tunnel_pid = child.id();

    let mut tunnel_url = None;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Ok(log) = std::fs::read_to_string(&log_path) {
            for line in log.lines() {
                if let Some(pos) = line.find("https://") {
                    let rest = &line[pos..];
                    let end = rest
                        .find(|c: char| c.is_whitespace() || c == '|')
                        .unwrap_or(rest.len());
                    let url = rest[..end].trim_end_matches('|').trim().to_string();
                    if url.contains("trycloudflare.com") {
                        tunnel_url = Some(url);
                        break;
                    }
                }
            }
        }
        if tunnel_url.is_some() {
            break;
        }
    }

    let Some(tunnel_url) = tunnel_url else {
        eprintln!("[ERROR] failed to get tunnel URL");
        let _ = Command::new("kill").arg(tunnel_pid.to_string()).status();
        std::process::exit(1);
    };

    if let Err(e) = pidfile::write_tunnel(tunnel_pid, &tunnel_url) {
        eprintln!("[WARN] failed to write tunnel files: {e}");
    }

    eprintln!("[herdr-agents-bridge] tunnel ready: {tunnel_url}");
}

fn cmd_qr(with_tunnel: bool) {
    stop_all();
    ensure_server();

    if with_tunnel {
        ensure_tunnel();
    }

    let local_url = pidfile::read_url();
    let tunnel_url = pidfile::read_tunnel_url();

    let display_url = match (&tunnel_url, &local_url) {
        (Some(tun), Some(local)) => {
            let token = local.split("?t=").nth(1).unwrap_or("");
            format!("{tun}/?t={token}")
        }
        (None, Some(local)) => local.clone(),
        _ => {
            eprintln!("Failed to start server.");
            std::process::exit(1);
        }
    };

    println!();
    if tunnel_url.is_some() {
        println!("  Tunnel (remote):");
    } else {
        println!("  Local network only:");
    }
    println!("  {display_url}");
    println!();
    print_qr(&display_url);
    println!();
    println!("  Press any key to close...");

    use crossterm::event;
    use crossterm::terminal;
    terminal::enable_raw_mode().ok();
    let _ = event::read();
    terminal::disable_raw_mode().ok();
}

fn kill_pid(pid: u32) -> bool {
    Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn wait_pid_exit(pid: u32, timeout_ms: u64) {
    let proc_path = format!("/proc/{pid}");
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
    while std::time::Instant::now() < deadline {
        if !std::path::Path::new(&proc_path).exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn stop_all() {
    if let Some(pid) = pidfile::read_tunnel_pid() {
        if kill_pid(pid) {
            wait_pid_exit(pid, 3000);
            eprintln!("[herdr-agents-bridge] tunnel stopped (PID {pid})");
        }
        pidfile::cleanup_tunnel();
    }

    if let Some(pid) = pidfile::read_pid() {
        if kill_pid(pid) {
            wait_pid_exit(pid, 3000);
            eprintln!("[herdr-agents-bridge] server stopped (PID {pid})");
        }
        pidfile::cleanup();
    }
}

fn cmd_stop() {
    let had_tunnel = pidfile::read_tunnel_pid().is_some();
    let had_server = pidfile::read_pid().is_some();
    stop_all();
    if !had_tunnel && !had_server {
        eprintln!("Nothing to stop.");
        std::process::exit(1);
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
