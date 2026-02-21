use gst::prelude::*;
use gstreamer as gst;
use smithay_client_toolkit::reexports::client::Connection;
use std::{env, error::Error, io, path::PathBuf};

pub type DynError = Box<dyn Error>;
const BLANK_VIDEO_URI: &str = "blank://";
const ARCH_CODEC_HINT: &str = "Arch Linux codec hint: install `gstreamer gst-plugins-base gst-plugins-good gst-plugins-bad gst-plugins-ugly gst-libav ffmpeg` with pacman.";

pub fn play_video(input: &str, loop_playback: bool) -> Result<(), DynError> {
    let wayland_display = env::var("WAYLAND_DISPLAY").unwrap_or_else(|_| "wayland-0".to_string());
    let _wayland_connection = Connection::connect_to_env().map_err(|error| {
        io::Error::other(format!(
            "failed to connect to Wayland display '{wayland_display}' via SCTK: {error}"
        ))
    })?;

    gst::init()
        .map_err(|error| io::Error::other(format!("failed to initialize GStreamer: {error}")))?;

    warn_about_codec_runtime();

    if is_blank_source(input) {
        return play_blank_video(loop_playback, &wayland_display);
    }

    let uri = to_uri(input)?;

    let playbin = gst::ElementFactory::make("playbin")
        .name("player")
        .build()
        .map_err(|_| io::Error::other("GStreamer element 'playbin' is unavailable"))?;

    let waylandsink = gst::ElementFactory::make("waylandsink")
        .name("wallpaper_sink")
        .build()
        .map_err(|_| {
            io::Error::other(
                format!(
                    "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support. {ARCH_CODEC_HINT}"
                ),
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

fn is_blank_source(input: &str) -> bool {
    let normalized = input.trim().to_ascii_lowercase();
    normalized == "blank" || normalized == "none" || normalized == BLANK_VIDEO_URI
}

fn warn_about_codec_runtime() {
    let has_ffmpeg_bridge = ["avdec_h264", "avdec_hevc", "avdec_vp9", "avdec_av1"]
        .iter()
        .any(|decoder| gst::ElementFactory::find(decoder).is_some());
    let has_av1_decoder = ["dav1ddec", "av1dec", "avdec_av1"]
        .iter()
        .any(|decoder| gst::ElementFactory::find(decoder).is_some());

    if !has_ffmpeg_bridge || !has_av1_decoder {
        eprintln!("{ARCH_CODEC_HINT}");
        if !has_ffmpeg_bridge {
            eprintln!("Codec runtime warning: no gst-libav ffmpeg decoder was detected.");
        }
        if !has_av1_decoder {
            eprintln!(
                "Codec runtime warning: no AV1 decoder detected (`dav1ddec`, `av1dec`, or `avdec_av1`)."
            );
        }
    }
}

fn play_blank_video(loop_playback: bool, wayland_display: &str) -> Result<(), DynError> {
    let source = gst::ElementFactory::make("videotestsrc")
        .name("blank_src")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'videotestsrc' is unavailable. Install gst-plugins-base.",
            )
        })?;
    source.set_property_from_str("pattern", "black");
    source.set_property("is-live", true);
    if !loop_playback {
        source.set_property("num-buffers", 1u32);
    }

    let convert = gst::ElementFactory::make("videoconvert")
        .name("blank_convert")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'videoconvert' is unavailable. Install gst-plugins-base.",
            )
        })?;

    let sink = gst::ElementFactory::make("waylandsink")
        .name("blank_sink")
        .build()
        .map_err(|_| {
            io::Error::other(
                "GStreamer element 'waylandsink' is unavailable. Install gst-plugins-bad with Wayland support.",
            )
        })?;

    let pipeline = gst::Pipeline::new();
    pipeline
        .add_many([&source, &convert, &sink])
        .map_err(|error| io::Error::other(format!("failed to build blank pipeline: {error}")))?;

    gst::Element::link_many([&source, &convert, &sink])
        .map_err(|error| io::Error::other(format!("failed to link blank pipeline: {error}")))?;

    let bus = pipeline
        .bus()
        .ok_or_else(|| io::Error::other("failed to retrieve GStreamer bus"))?;

    pipeline.set_state(gst::State::Playing).map_err(|error| {
        io::Error::other(format!("failed to set pipeline to Playing: {error:?}"))
    })?;

    println!(
        "Playing blank background on Wayland display '{wayland_display}' (loop={loop_playback})"
    );

    for message in bus.iter_timed(gst::ClockTime::NONE) {
        use gst::MessageView;

        match message.view() {
            MessageView::Eos(..) => {
                break;
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

    pipeline
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

#[cfg(test)]
mod tests {
    use super::is_blank_source;

    #[test]
    fn blank_source_aliases_are_supported() {
        assert!(is_blank_source("blank"));
        assert!(is_blank_source("blank://"));
        assert!(is_blank_source("none"));
        assert!(!is_blank_source("video.mp4"));
    }
}
