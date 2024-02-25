use crate::data::{
    find_in_active, make_active, remove_from_active, set_duration, set_position_secs, set_title,
};
use rusqlite::Connection;

const END_DETECTION_TOLERANCE_SECONDS: f64 = 1.0;

pub fn play(conn: &Connection, url: &str, mpv_binary: &str) -> Result<(), rusqlite::Error> {
    crate::ignore_constraint_errors(make_active(conn, url))?;
    let active = find_in_active(conn, url)?.unwrap();

    let tmp_dir = tempfile::tempdir().unwrap();

    let pipe_path = tmp_dir.path().join("mpv.pipe");

    let mut output = std::process::Command::new(mpv_binary)
        .arg(&active.url)
        .arg(format!(
            "--input-ipc-server={}",
            pipe_path.to_string_lossy()
        ))
        .arg(format!("--start=+{}", active.position_secs))
        .arg("--force-window=immediate")
        .spawn()
        .unwrap();
    while !pipe_path.exists() {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let mut mpv = mpvipc::Mpv::connect(pipe_path.as_path().to_str().unwrap()).unwrap();

    //TODO get title?

    mpv.observe_property(0, "playback-time").unwrap();
    mpv.observe_property(1, "duration").unwrap();
    mpv.observe_property(2, "media-title").unwrap();

    let mut playback_time = None;
    let mut duration_secs = None;
    let mut title = None;
    while let Ok(e) = mpv.event_listen() {
        if let mpvipc::Event::PropertyChange { property, .. } = e {
            match property {
                mpvipc::Property::PlaybackTime(Some(t)) => {
                    playback_time = Some(t);
                }
                mpvipc::Property::Duration(Some(d)) => {
                    duration_secs = Some(d);
                }
                mpvipc::Property::Unknown {
                    name,
                    data: mpvipc::MpvDataType::String(t),
                } if name == "media-title" => {
                    title = Some(t);
                }
                _ => {}
            }
        }
    }
    if duration_secs.is_some()
        && playback_time.is_some()
        && playback_time.unwrap() >= duration_secs.unwrap() - END_DETECTION_TOLERANCE_SECONDS
    {
        remove_from_active(conn, &active.url)?;
    } else {
        if let Some(t) = playback_time {
            set_position_secs(conn, &active.url, t)?;
        }
        if let Some(d) = duration_secs {
            set_duration(conn, &active.url, d)?;
        }
    }
    if let (Some(new_title), None) = (title, active.title) {
        set_title(conn, &active.url, &new_title)?;
    }
    output.wait().unwrap();
    Ok(())
}
