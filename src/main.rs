mod handler;
mod herdr;
mod html;
mod inject;
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

#[tokio::main]
async fn main() {
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
            eprintln!("\n[ERROR] ポート {PORT} は既に使用中です{pid_str}");
            eprintln!("  古いVoiceBridgeが残っている可能性があります。");
            eprintln!("  手動で停止してから再起動してください。");
            std::process::exit(1);
        }
    };

    println!("{}", "=".repeat(50));
    println!("  VoiceBridge サーバー起動");
    println!("{}", "=".repeat(50));
    println!("  ローカルIP : {local_ip}");
    println!("  ポート     : {PORT}");
    println!("  トークン   : {}", state.session_token);
    println!();
    println!("  QRをスキャンしてiPhoneで開く:");
    println!("    {ui_url}");
    println!();
    print_qr(&ui_url);
    println!();
    println!("  停止: Ctrl+C");
    println!("{}", "=".repeat(50));

    let app = handler::build_router(state)
        .into_make_service_with_connect_info::<SocketAddr>();

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    println!("\nサーバーを停止しました。");
}

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}
