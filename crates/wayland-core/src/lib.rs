use gst::prelude::*;
use gstreamer as gst;
use smithay_client_toolkit::reexports::client::Connection;
use std::{env, error::Error, io, path::PathBuf};

pub type DynError = Box<dyn Error>;

pub fn play_video(input: &str, loop_playback: bool) -> Result<(), DynError> {
    let uri = to_uri(input)?;
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    let _wayland_connection = Connection::connect_to_env().map_err(|error| {
        io::Error::other(format!(
            "failed to connect to Wayland display '{wayland_display}' via SCTK: {error}"
        ))
    })?;

    gst::init()
        .map_err(|error| io::Error::other(format!("failed to initialize GStreamer: {error}")))?;

    let playbin = gst::ElementFactory::make("playbin")
        .name("player")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'playbin' is unavailable"))?;

    let waylandsink = gst::ElementFactory::make("waylandsink")
        .name("wallpaper_sink")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support.",
            )
        })?;

    playbin.set_property("video-sink", &waylandsink);
    playbin.set_property("uri", &uri);

    let bus = playbin
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    playbin.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    println!("Playing on Wayland display '{wayland_display}': {uri} (loop={loop_playback})");

    for message in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match message.view() {
            MessageView::Eos(..) => {
                if loop_playback {
                    playbin
                        .seek_simple(
                            gst::SeekFlags::FLUSH | gst::SeekFlags::KEY_UNIT,
                            gst::ClockTime::ZERO,
                        )
                        .map_err(|error| {
                            io::Error::other(format!(
                                "failed to seek to start for looped playback: {error}"
                            ))
                        })?;
                } else {
                    println!("End of stream.");
                    break;
                }
            }
            MessageView::Error(error) => {
                let source = error
                    .src()
                    .map(|src| src.path_string())
                    .unwrap_or_else(|| "unknown".into());
                return Err(io::Error::other(format!(
                    "GStreamer error from {source}: {} ({:?})",
                    error.error(),
                    error.debug()
                ))
                .into());
            }
            _ => {}
        }
    }

    playbin
        .set_state(gst::State::Null)
        .map_err(|error| io::Error::other(format!("failed to set pipeline to Null: {error:?}")))?;

    Ok(())
}

fn to_uri(input: &str) -> Result<String, io::Error> {
    if input.contains("://") {
        return Ok(input.to_string());
    }

    let input_path = PathBuf::from(input);
    let absolute_path = if input_path.is_absolute() {
        input_path
    } else {
        env::current_dir()?.join(input_path)
    };

    let normalized_path = absolute_path
        .canonicalize()
        .unwrap_or_else(|_| absolute_path.clone());

    gst::glib::filename_to_uri(&normalized_path, None)
        .map(|uri| uri.to_string())
        .map_err(|error| {
            io::Error::other(format!(
                "failed to convert '{}' into a file URI: {error}",
                normalized_path.display()
            ))
        })
}
