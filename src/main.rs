mod handler;
mod herdr;
mod html;
mod inject;
mod pidfile;
mod state;

use std::net::{SocketAddr, UdpSocket};
use std::process::Command;
use std::sync::Arc;

use inject::XdotoolInjector;
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
    use qrcode::QrCode;
    let code = match QrCode::new(url) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[WARN] QR code generation failed: {e}");
            return;
        }
    };
    let string = code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(2, 1)
        .build();
    println!("{string}");
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("serve");

    match cmd {
        "serve" => cmd_serve(),
        "qr" => cmd_qr(),
        "tunnel" => cmd_tunnel(),
        "stop" => cmd_stop(),
        other => {
            eprintln!("Unknown command: {other}");
            eprintln!("Usage: herdr-agents-bridge [serve|qr|tunnel|stop]");
            std::process::exit(1);
        }
    }
}

#[tokio::main]
async fn cmd_serve() {
    let local_ip = get_local_ip();
    let state = Arc::new(AppState::new(Box::new(XdotoolInjector)));
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

fn cmd_qr() {
    if !pidfile::is_running() {
        eprintln!("[herdr-agents-bridge] starting server...");
        let exe = std::env::current_exe().expect("failed to get executable path");
        Command::new(&exe)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to start server");

        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if pidfile::read_url().is_some() {
                break;
            }
        }
    }

    let local_url = pidfile::read_url();
    let tunnel_url = pidfile::read_tunnel_url();

    let url = tunnel_url.as_deref().or(local_url.as_deref());
    let Some(url) = url else {
        eprintln!("Failed to start server.");
        std::process::exit(1);
    };

    println!();
    if tunnel_url.is_some() {
        println!("  Tunnel (remote):");
    } else {
        println!("  Local network only:");
    }
    println!("  {url}");
    println!();
    print_qr(&url);
    println!();
    println!("  Press any key to close...");

    use crossterm::event;
    use crossterm::terminal;
    terminal::enable_raw_mode().ok();
    let _ = event::read();
    terminal::disable_raw_mode().ok();
}

fn cmd_tunnel() {
    if !pidfile::is_running() {
        eprintln!("[herdr-agents-bridge] starting server...");
        let exe = std::env::current_exe().expect("failed to get executable path");
        Command::new(&exe)
            .arg("serve")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("failed to start server");

        for _ in 0..50 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            if pidfile::read_url().is_some() {
                break;
            }
        }
    }

    let url = format!("http://localhost:{PORT}");
    eprintln!("[herdr-agents-bridge] starting tunnel...");

    let mut child = Command::new("cloudflared")
        .args(["tunnel", "--url", &url])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start cloudflared");

    let tunnel_pid = child.id();
    let stderr = child.stderr.take().unwrap();
    let reader = std::io::BufReader::new(stderr);

    use std::io::BufRead;
    let mut tunnel_url = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if let Some(pos) = line.find("https://") {
            let rest = &line[pos..];
            let end = rest.find(|c: char| c.is_whitespace() || c == '|').unwrap_or(rest.len());
            let url = rest[..end].trim_end_matches('|').trim().to_string();
            if url.contains("trycloudflare.com") {
                tunnel_url = Some(url);
                break;
            }
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

fn kill_pid(pid: u32) -> bool {
    Command::new("kill")
        .arg(pid.to_string())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn cmd_stop() {
    let mut stopped_any = false;

    if let Some(pid) = pidfile::read_tunnel_pid() {
        if kill_pid(pid) {
            eprintln!("[herdr-agents-bridge] tunnel stopped (PID {pid})");
        }
        pidfile::cleanup_tunnel();
        stopped_any = true;
    }

    if let Some(pid) = pidfile::read_pid() {
        if kill_pid(pid) {
            eprintln!("[herdr-agents-bridge] server stopped (PID {pid})");
        }
        pidfile::cleanup();
        stopped_any = true;
    }

    if !stopped_any {
        eprintln!("Nothing to stop.");
        std::process::exit(1);
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
