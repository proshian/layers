use std::process::Command;

fn which_surreal() -> Option<String> {
    // Check if `surreal` is on PATH
    if Command::new("surreal").arg("--version").output().is_ok() {
        return Some("surreal".to_string());
    }
    // Check ~/.surrealdb/surreal (default install location)
    if let Some(home) = dirs::home_dir() {
        let path = home.join(".surrealdb").join("surreal");
        if path.exists() {
            return Some(path.to_string_lossy().to_string());
        }
    }
    None
}

fn main() {
    let port: u16 = std::env::args()
        .position(|a| a == "--port")
        .and_then(|i| std::env::args().nth(i + 1))
        .and_then(|s| s.parse().ok())
        .unwrap_or(8000);

    let data_dir = std::env::args()
        .position(|a| a == "--data")
        .and_then(|i| std::env::args().nth(i + 1))
        .unwrap_or_else(|| {
            let home = dirs::home_dir().expect("Cannot determine home directory");
            home.join(".layers").join("server.db").to_string_lossy().to_string()
        });

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(&data_dir).parent() {
        std::fs::create_dir_all(parent).ok();
    }

    println!("Starting SurrealDB server...");
    println!("  Data directory: {data_dir}");
    println!("  Listening on:   ws://0.0.0.0:{port}");
    println!();
    println!("Clients connect with: --db-url ws://localhost:{port}");
    println!();

    let bind = format!("0.0.0.0:{port}");

    // Try to find the surreal binary: PATH first, then ~/.surrealdb/surreal
    let surreal_bin = which_surreal().unwrap_or_else(|| "surreal".to_string());

    let status = Command::new(&surreal_bin)
        .arg("start")
        .arg("--bind").arg(&bind)
        .arg("--unauthenticated")
        .arg(format!("rocksdb:{data_dir}"))
        .status();

    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("SurrealDB exited with status: {s}");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("Failed to start SurrealDB: {e}");
            eprintln!();
            eprintln!("Make sure 'surreal' is installed:");
            eprintln!("  curl -sSf https://install.surrealdb.com | sh");
            std::process::exit(1);
        }
    }
}
