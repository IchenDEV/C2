#[tokio::main]
async fn main() {
    if std::env::args().any(|argument| argument == "--codetwo-assistant-mcp") {
        if let Err(error) = codetwo_core::assistant_bridge::run_stdio() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = codetwo_desktop_host::configure_stdio() {
        eprintln!("C2 cannot configure stdio: {error}");
        std::process::exit(1);
    }
    if std::env::args().any(|argument| argument == "--codetwo-scene-mcp") {
        if let Err(error) = codetwo_desktop_host::run_scene_mcp() {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return;
    }
    if let Err(error) = codetwo_desktop_host::run().await {
        eprintln!("C2 desktop host failed: {error}");
        std::process::exit(1);
    }
}
