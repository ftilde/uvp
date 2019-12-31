use crate::data::{find_in_active, make_active, remove_from_active, set_duration, set_playbackpos};
use rusqlite::Connection;

const END_DETECTION_TOLERANCE_SECONDS: f64 = 1.0;

pub fn play(conn: &Connection, url: &str) -> Result<(), rusqlite::Error> {
    crate::ignore_constraint_errors(make_active(conn, url))?;
    let active = find_in_active(conn, url)?.unwrap();

    let tmp_dir = tempfile::tempdir().unwrap();

    let pipe_path = tmp_dir.path().join("mpv.pipe");

    let mut output = std::process::Command::new("mpv")
        .arg(&active.url)
        .arg("--input-ipc-server")
        .arg(&pipe_path)
        .arg(format!("--start=+{}", active.playbackpos))
        .spawn()
        .unwrap();
    while !pipe_path.exists() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut mpv = mpvipc::Mpv::connect(pipe_path.as_path().to_str().unwrap()).unwrap();

    //TODO get title?

    mpv.observe_property(&0, "playback-time").unwrap();
    mpv.observe_property(&1, "duration").unwrap();
    let mut playback_time = 0.0;
    let mut duration_secs = None;
    while let Ok(e) = mpv.event_listen() {
        if let mpvipc::Event::PropertyChange { property, .. } = e {
            match property {
                mpvipc::Property::PlaybackTime(Some(t)) => {
                    playback_time = t;
                }
                mpvipc::Property::Duration(Some(d)) => {
                    duration_secs = Some(d);
                }
                _ => {}
            }
        }
    }
    if duration_secs.is_some()
        && playback_time >= duration_secs.unwrap() - END_DETECTION_TOLERANCE_SECONDS
    {
        remove_from_active(conn, &active.url)?;
    } else {
        set_playbackpos(conn, &active.url, playback_time)?;
        if let Some(d) = duration_secs {
            set_duration(conn, &active.url, d)?;
        }
    }
    output.wait().unwrap();
    Ok(())
}
