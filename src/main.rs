use pieced::app::{GameOptions, game_app};

fn main() -> anyhow::Result<()> {
    pieced::telemetry::mark_process_start();
    let args: Vec<String> = std::env::args().collect();
    let mut app = game_app(GameOptions::from_args(&args))?;
    #[cfg(target_os = "macos")]
    if let Some(main_thread) = objc2::MainThreadMarker::new() {
        // A command-line launch should take focus just like a Finder launch.
        #[allow(deprecated)]
        objc2_app_kit::NSApplication::sharedApplication(main_thread)
            .activateIgnoringOtherApps(true);
    }
    app.run();
    Ok(())
}
