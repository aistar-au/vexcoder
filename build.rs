use std::process::Command;

fn main() {
    // GIT_COMMIT_SHORT: first 7 chars of HEAD; "unknown" if git is unavailable.
    let commit = Command::new("git")
        .args(["rev-parse", "--short=7", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    // BUILD_DATE: UTC date in YYYY-MM-DD format.
    // Uses the POSIX `date` utility rather than a platform-specific API.
    // Windows builders may not provide this command in the default environment.
    // When unavailable, BUILD_DATE falls back to "unknown".
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    println!("cargo:rustc-env=GIT_COMMIT_SHORT={commit}");
    println!("cargo:rustc-env=BUILD_DATE={date}");

    // Rerun only when HEAD changes (not on every source edit).
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs");
}
