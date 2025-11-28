use std::process::Command;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

fn main() {
    let git_hash = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .and_then(|output| {
            if output.status.success() {
                let trimmed = String::from_utf8_lossy(&output.stdout).trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed)
                }
            } else {
                None
            }
        });

    let build_stamp = git_hash.or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| i64::try_from(duration.as_secs()).ok())
            .map(|secs| secs.to_string())
    });

    if let Some(stamp) = build_stamp {
        println!("cargo:rustc-env=CODEX_BUILD_HASH={stamp}");
    }
}
