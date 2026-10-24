//! `voxtype reload` — ask the running daemon to apply config.toml and wait for
//! its answer. The file protocol lives in the library (`voxtype::reload`) next
//! to the daemon side that answers it.

use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use voxtype::{config, daemon_status, reload};

pub(crate) fn run_reload(timeout_secs: u64) -> anyhow::Result<()> {
    // Exits with a message when no daemon is running.
    daemon_status::check_daemon_running()?;

    let dir = config::Config::runtime_dir();
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let id = format!("{}-{}", std::process::id(), nanos);
    reload::request(&dir, &id)
        .map_err(|e| anyhow::anyhow!("Failed to write the reload request: {}", e))?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match reload::read_answer(&dir, &id) {
            Some(Ok(applied)) => {
                println!("{}", applied.describe());
                return Ok(());
            }
            Some(Err(message)) => {
                eprintln!("Settings not applied: {}", message);
                eprintln!("The daemon keeps running with its previous settings.");
                std::process::exit(1);
            }
            None => {}
        }
        if Instant::now() >= deadline {
            reload::withdraw(&dir, &id);
            eprintln!(
                "The daemon did not apply the settings within {}s (still dictating, or a daemon \
                 older than this command); the request was withdrawn.",
                timeout_secs
            );
            std::process::exit(3);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
