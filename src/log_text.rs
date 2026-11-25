//! The log of this run, to send to the server (Remux's `logbuf`): what someone has to do who has
//! no way to read the log of the device (a TV, a phone). NSH's logger is the one Rust's `log`
//! goes to, so the lines are not kept here; Android lets an app read its own lines of the system
//! log, which has ExoPlayer's too.

/// What the server takes of a log is a megabyte; a little under it.
const MAX_BYTES: usize = 900_000;

/// The last of the log, as a text, under a head that says what this is. The system log is read
/// on a thread of its own: reading it takes a while, which the screen does not wait for.
pub async fn text() -> String {
    let head = format!("Remux {} (NSH) on {}\n\n", crate::app_host::app_host().remux_version, std::env::consts::OS);
    let (send, receive) = futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = send.send(read());
    });
    let lines = receive.await.unwrap_or_default();
    log::info!("send log: {} bytes read", lines.len());
    // The newest lines, as many as fit.
    let mut kept = Vec::new();
    let mut size = head.len();
    for line in lines.lines().rev() {
        size += line.len() + 1;
        if size > MAX_BYTES {
            break;
        }
        kept.push(line);
    }
    kept.reverse();
    head + &kept.join("\n")
}

#[cfg(target_os = "android")]
fn read() -> String {
    let pid = std::process::id().to_string();
    match std::process::Command::new("/system/bin/logcat")
        .args(["-d", "-v", "time", "--pid", &pid])
        .stdin(std::process::Stdio::null())
        .output()
    {
        Ok(output) => String::from_utf8_lossy(&output.stdout).into_owned(),
        Err(error) => format!("(the log could not be read: {error})"),
    }
}

#[cfg(not(target_os = "android"))]
fn read() -> String {
    "(no log on this platform)".to_owned()
}
