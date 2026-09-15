use std::path::{Path, PathBuf};

use qf_helper::config::{default_config_path, Config};
use qf_helper::heartbeat::{describe, Client, Heartbeat, Outcome};
use qf_helper::queue::{default_queue_path, Queue};
use qf_helper::{process, trade};

const USAGE: &str = "Usage: qf-helper [--config <path>] [--once] | qf-helper --parse <EE.log>";

#[tokio::main]
async fn main() {
    let mut config_path: Option<PathBuf> = None;
    let mut once = false;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" => match args.next() {
                Some(path) => config_path = Some(PathBuf::from(path)),
                None => exit_with(2, &format!("--config needs a path\n{USAGE}")),
            },
            "--parse" => match args.next() {
                Some(path) => {
                    let bytes = std::fs::read(&path).unwrap_or_else(|e| exit_with(2, &format!("Cannot read {path}: {e}")));
                    let events = qf_log_parser::scan_all(&bytes);
                    println!("{}", serde_json::to_string_pretty(&events).expect("events serialize"));
                    return;
                }
                None => exit_with(2, &format!("--parse needs a file\n{USAGE}")),
            },
            "--once" => once = true,
            "--help" | "-h" => {
                println!("{USAGE}");
                return;
            }
            other => exit_with(2, &format!("Unknown argument {other}\n{USAGE}")),
        }
    }

    let home = PathBuf::from(std::env::var("HOME").unwrap_or_default());
    let path = config_path.unwrap_or_else(|| default_config_path(std::env::var("XDG_CONFIG_HOME").ok().as_deref(), &home));
    let config = Config::load(&path, &home).unwrap_or_else(|e| exit_with(2, &e));
    println!(
        "qf-helper {} sending heartbeats to {} (config {}, EE.log {})",
        env!("CARGO_PKG_VERSION"),
        config.server_url,
        path.display(),
        config.ee_log_path.display()
    );

    let client = Client::new(&config.server_url, &config.device_key);
    if !once {
        let queue = Queue::new(default_queue_path(std::env::var("XDG_STATE_HOME").ok().as_deref(), &home));
        tokio::spawn(trade::run(config.clone(), queue));
    }
    let own_pid = std::process::id();
    let mut last_line = String::new();
    loop {
        let warframe_running = process::warframe_running(Path::new("/proc"), own_pid);
        let outcome = client
            .send(&Heartbeat { warframe_running, version: env!("CARGO_PKG_VERSION").to_string() })
            .await;
        let line = describe(warframe_running, &outcome);
        if once {
            println!("{line}");
            std::process::exit(if outcome == Outcome::Accepted { 0 } else { 1 });
        }
        if line != last_line {
            println!("{line}");
            last_line = line;
        }
        tokio::select! {
            _ = tokio::time::sleep(outcome.next_delay()) => {}
            _ = tokio::signal::ctrl_c() => {
                println!("qf-helper stopping");
                return;
            }
        }
    }
}

fn exit_with(code: i32, message: &str) -> ! {
    eprintln!("{message}");
    std::process::exit(code)
}
