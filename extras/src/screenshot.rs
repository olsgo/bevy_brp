//! Screenshot handler for BRP extras
//!
//! This module provides screenshot functionality via the Bevy Remote Protocol.
//! It addresses common timing issues by supporting frame delays before capture.

use bevy::prelude::*;
use bevy::remote;
use bevy::remote::BrpError;
use bevy::remote::BrpResult;
use bevy::remote::error_codes::INTERNAL_ERROR;
use bevy::remote::error_codes::INVALID_PARAMS;
use bevy::render::view::screenshot::Screenshot;
use bevy::render::view::screenshot::ScreenshotCaptured;
use bevy::tasks::IoTaskPool;
use serde_json::Value;
use serde_json::json;

/// Default number of frames to wait before capturing screenshot.
/// This ensures the scene has rendered at least once to avoid white/blank screenshots.
const DEFAULT_DELAY_FRAMES: u32 = 2;

/// Component for pending screenshots that need to wait for frame delay
#[derive(Component)]
pub struct PendingScreenshot {
    /// Path to save the screenshot
    pub path: String,
    /// Remaining frames to wait before capture
    pub frames_remaining: u32,
}

/// System that processes pending screenshots, counting down frames and triggering capture
pub fn process_pending_screenshots(
    mut commands: Commands,
    mut query: Query<(Entity, &mut PendingScreenshot)>,
) {
    for (entity, mut pending) in query.iter_mut() {
        if pending.frames_remaining == 0 {
            // Time to take the screenshot
            let path = pending.path.clone();
            info!("Frame delay complete, capturing screenshot: {}", path);

            // Remove the pending component and add the actual Screenshot component
            commands.entity(entity).remove::<PendingScreenshot>();
            commands.entity(entity).insert(Screenshot::primary_window());

            // Add observer for when capture completes
            commands.entity(entity).observe(create_save_observer(path));
        } else {
            pending.frames_remaining -= 1;
            trace!(
                "Screenshot pending, {} frames remaining",
                pending.frames_remaining
            );
        }
    }
}

/// Creates an observer that saves the screenshot when captured
fn create_save_observer(path: String) -> impl FnMut(On<ScreenshotCaptured>) {
    move |screenshot_captured: On<ScreenshotCaptured>| {
        info!("Screenshot captured! Starting async save to: {}", path);
        let img = screenshot_captured.event().image.clone();
        let path_clone = path.clone();

        // Move file I/O to background thread to avoid blocking main thread
        IoTaskPool::get()
            .spawn(async move {
                match img.try_into_dynamic() {
                    Ok(dyn_img) => {
                        // Create parent directory if needed
                        if let Some(parent) = std::path::Path::new(&path_clone).parent()
                            && let Err(e) = std::fs::create_dir_all(parent)
                        {
                            error!(
                                "Failed to create directory for screenshot {}: {}",
                                path_clone, e
                            );
                            return;
                        }

                        // Convert to RGB8 to discard alpha channel which stores brightness
                        // values when HDR is enabled - this matches Bevy's save_to_disk behavior
                        let rgb_img = dyn_img.to_rgb8();

                        // Save the image
                        match rgb_img.save(&path_clone) {
                            Ok(()) => {
                                info!("Screenshot successfully saved to: {}", path_clone);
                            }
                            Err(e) => {
                                error!("Failed to save screenshot to {}: {}", path_clone, e);
                            }
                        }
                    }
                    Err(e) => error!("Failed to convert screenshot to dynamic image: {}", e),
                }
            })
            .detach();
    }
}

/// Handler for screenshot requests
///
/// Takes a screenshot of the primary window and saves it to the specified path.
///
/// # Parameters
/// - `path` (required): The file path to save the screenshot
/// - `delay_frames` (optional): Number of frames to wait before capturing (default: 2)
///   This helps avoid white/blank screenshots by ensuring the scene has rendered.
///
/// # Notes
/// - File I/O is performed asynchronously to avoid blocking the main thread
/// - The alpha channel is discarded (converted to RGB8) to handle HDR correctly
/// - Returns immediately after scheduling; actual save happens asynchronously
pub fn handler(In(params): In<Option<Value>>, world: &mut World) -> BrpResult {
    // Check if PNG support is available at runtime
    if bevy::image::ImageFormat::from_extension("png").is_none() {
        return Err(BrpError {
            code:    remote::error_codes::INTERNAL_ERROR,
            message: "PNG support not available. Enable the 'png' feature in your Bevy dependency"
                .to_string(),
            data:    None,
        });
    }

    // Get the path from params
    let path = params
        .as_ref()
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| BrpError {
            code:    INVALID_PARAMS,
            message: "Missing 'path' parameter".to_string(),
            data:    None,
        })?;

    // Get optional delay_frames parameter (default: DEFAULT_DELAY_FRAMES)
    let delay_frames = params
        .as_ref()
        .and_then(|v| v.get("delay_frames"))
        .and_then(|v| v.as_u64())
        .map_or(DEFAULT_DELAY_FRAMES, |v| v as u32);

    // Convert to absolute path
    let path_buf = std::path::Path::new(path);
    let absolute_path = if path_buf.is_absolute() {
        path_buf.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|e| BrpError {
                code:    INTERNAL_ERROR,
                message: format!("Failed to get current directory: {e}"),
                data:    None,
            })?
            .join(path_buf)
    };

    let absolute_path_str = absolute_path.to_string_lossy().to_string();

    // Log the screenshot request
    info!(
        "Screenshot requested for: {} (delay: {} frames)",
        absolute_path_str, delay_frames
    );

    // Check if we have a primary window
    let window_exists = world.query::<&Window>().iter(world).any(|w| {
        info!(
            "Found window - resolution: {:?}, visible: {:?}",
            w.resolution, w.visible
        );
        true
    });

    if !window_exists {
        return Err(BrpError {
            code:    INTERNAL_ERROR,
            message: "No windows found - cannot take screenshot".to_string(),
            data:    None,
        });
    }

    // Spawn entity based on delay setting
    let entity = if delay_frames == 0 {
        // Immediate capture (original behavior, but with RGB8 fix)
        let path_for_observer = absolute_path_str.clone();
        world
            .spawn((
                Screenshot::primary_window(),
                Name::new(format!("Screenshot_{absolute_path_str}")),
            ))
            .observe(create_save_observer(path_for_observer))
            .id()
    } else {
        // Delayed capture - spawn with PendingScreenshot component
        world
            .spawn((
                PendingScreenshot {
                    path:             absolute_path_str.clone(),
                    frames_remaining: delay_frames,
                },
                Name::new(format!("PendingScreenshot_{absolute_path_str}")),
            ))
            .id()
    };

    info!(
        "Screenshot entity spawned with ID: {:?} (delay: {} frames)",
        entity, delay_frames
    );

    Ok(json!({
        "success": true,
        "path": absolute_path_str,
        "delay_frames": delay_frames,
        "working_directory": std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("unknown"))
            .to_string_lossy(),
        "note": if delay_frames > 0 {
            format!("Screenshot will be captured after {} frame(s) to ensure scene is rendered. File I/O is asynchronous.", delay_frames)
        } else {
            "Screenshot capture initiated immediately. File I/O will be performed asynchronously.".to_string()
        }
    }))
}
