//! macOS window placement. Scenario windows join every Space and float in front,
//! so evidence runs render even while another app is full screen.

use bevy::prelude::*;

pub struct NativeWindowPlugin;

impl Plugin for NativeWindowPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, configure_native_window);
    }
}

fn configure_native_window(
    mut done: Local<bool>,
    scenario: Option<Res<crate::scenario::ScenarioRun>>,
) {
    if *done {
        return;
    }
    *done = bring_to_front(scenario.is_some());
}

/// Returns true once a window was found and configured.
#[cfg(target_os = "macos")]
fn bring_to_front(scenario: bool) -> bool {
    use objc2_app_kit::{NSApplication, NSWindowCollectionBehavior};
    let Some(main_thread) = objc2::MainThreadMarker::new() else {
        return false;
    };
    let application = NSApplication::sharedApplication(main_thread);
    let windows = application.windows();
    if windows.is_empty() {
        return false;
    }
    for window in windows {
        if scenario {
            window.setCollectionBehavior(
                NSWindowCollectionBehavior::CanJoinAllSpaces
                    | NSWindowCollectionBehavior::FullScreenAuxiliary,
            );
        }
        window.orderFrontRegardless();
        if window.canBecomeKeyWindow() {
            window.makeKeyWindow();
        }
    }
    #[allow(deprecated)]
    application.activateIgnoringOtherApps(true);
    true
}

#[cfg(not(target_os = "macos"))]
fn bring_to_front(_scenario: bool) -> bool {
    true
}
