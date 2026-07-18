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
            eprintln!("[WARN] QRコード生成失敗: {e}");
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
        "stop" => cmd_stop(),
        other => {
            eprintln!("Unknown command: {other}");
            eprintln!("Usage: herdr-agents-bridge [serve|qr|stop]");
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
            eprintln!("[ERROR] ポート {PORT} は既に使用中です{pid_str}");
            std::process::exit(1);
        }
    };

    let pid = std::process::id();
    if let Err(e) = pidfile::write(pid, &ui_url) {
        eprintln!("[WARN] PID/URLファイル書き出し失敗: {e}");
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
    let Some(url) = pidfile::read_url() else {
        eprintln!("サーバーが起動していません。先に start してください。");
        std::process::exit(1);
    };

    println!();
    println!("  スマホでスキャンしてください:");
    println!("  {url}");
    println!();
    print_qr(&url);
    println!();
    println!("  何かキーを押すと閉じます...");

    use crossterm::event;
    use crossterm::terminal;
    terminal::enable_raw_mode().ok();
    let _ = event::read();
    terminal::disable_raw_mode().ok();
}

fn cmd_stop() {
    let Some(pid) = pidfile::read_pid() else {
        eprintln!("PIDファイルが見つかりません。サーバーは起動していないようです。");
        std::process::exit(1);
    };

    let status = Command::new("kill")
        .arg(pid.to_string())
        .status();

    match status {
        Ok(s) if s.success() => {
            pidfile::cleanup();
            eprintln!("[herdr-agents-bridge] stopped (PID {pid})");
        }
        _ => {
            eprintln!("[ERROR] PID {pid} の停止に失敗しました");
            pidfile::cleanup();
            std::process::exit(1);
        }
    }
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
