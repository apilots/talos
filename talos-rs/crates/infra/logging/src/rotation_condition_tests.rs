use super::*;
use std::io::Write;

#[test]
fn configured_midnight_rotates_but_dst_repeated_hour_does_not() {
    for (zone, before, after, rotates) in [
        (
            "Asia/Shanghai",
            "2026-10-22T15:59:59Z",
            "2026-10-22T16:00:00Z",
            true,
        ),
        (
            "Asia/Shanghai",
            "2026-10-22T23:59:59Z",
            "2026-10-23T00:00:00Z",
            false,
        ),
        (
            "America/New_York",
            "2026-11-01T05:30:00Z",
            "2026-11-01T06:30:00Z",
            false,
        ),
        (
            "America/New_York",
            "2026-11-02T04:59:59Z",
            "2026-11-02T05:00:00Z",
            true,
        ),
    ] {
        let path = crate::tests::temp_log_path("calendar.log");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let limits = LogRotation::default();
        let mut writer = open(&path, &limits, zone.parse().unwrap()).unwrap();
        for (text, time) in [(b"before\n".as_slice(), before), (b"after\n", after)] {
            let time = time
                .parse::<DateTime<chrono::Utc>>()
                .unwrap()
                .with_timezone(&Local);
            writer.write_with_datetime(text, &time).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);
        let archived = path.with_file_name("calendar.log.1");
        if rotates {
            assert_eq!(fs::read(&archived).unwrap(), b"before\n");
            assert_eq!(fs::read(&path).unwrap(), b"after\n");
        } else {
            assert!(!archived.exists(), "{zone}: unexpected rotation");
            assert_eq!(fs::read(&path).unwrap(), b"before\nafter\n");
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[test]
fn restart_recovers_the_configured_calendar_day_from_the_existing_file() {
    let path = crate::tests::temp_log_path("restart.log");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "saved\n").unwrap();
    let previous = "2026-10-22T15:59:59Z"
        .parse::<DateTime<chrono::Utc>>()
        .unwrap();
    fs::File::open(&path)
        .unwrap()
        .set_modified(previous.into())
        .unwrap();
    let mut writer = open(&path, &LogRotation::default(), chrono_tz::Asia::Shanghai).unwrap();
    let next = "2026-10-22T16:00:00Z"
        .parse::<DateTime<chrono::Utc>>()
        .unwrap()
        .with_timezone(&Local);
    writer.write_with_datetime(b"next-day\n", &next).unwrap();
    writer.flush().unwrap();
    drop(writer);
    assert_eq!(
        fs::read(path.with_file_name("restart.log.1")).unwrap(),
        b"saved\n"
    );
    assert_eq!(fs::read(&path).unwrap(), b"next-day\n");
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn library_size_retention_restart_and_guard_flush_keep_complete_records() {
    let path = crate::tests::temp_log_path("talos.log");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "legacy\n").unwrap();
    let limits = LogRotation {
        max_bytes: 128,
        max_files: 5,
    };
    let mut expected = String::from("legacy\n");
    for label in ["first", "restart"] {
        let (mut writer, guard) =
            crate::create_file_writer(path.to_str().unwrap(), Some(&limits), chrono_tz::UTC)
                .unwrap();
        for sequence in 0..20 {
            let line = format!("{label}-{sequence:03}\n");
            expected.push_str(&line);
            writer.write_all(line.as_bytes()).unwrap();
        }
        drop(writer);
        drop(guard);
    }
    let all = (0..limits.max_files)
        .rev()
        .filter_map(|index| {
            let file = if index == 0 {
                path.clone()
            } else {
                path.with_file_name(format!("talos.log.{index}"))
            };
            match fs::read_to_string(file) {
                Ok(text) => Some(text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => panic!("read retained archive: {error}"),
            }
        })
        .collect::<String>();
    assert_eq!(all, expected);
    assert!(
        path.with_file_name("talos.log.1").exists(),
        "size condition did not rotate"
    );
    let (mut writer, guard) =
        crate::create_file_writer(path.to_str().unwrap(), Some(&limits), chrono_tz::UTC).unwrap();
    for sequence in 0..200 {
        writer
            .write_all(format!("tail-{sequence:03}\n").as_bytes())
            .unwrap();
    }
    drop(writer);
    drop(guard);
    assert_eq!(
        fs::read_dir(path.parent().unwrap()).unwrap().count(),
        limits.max_files
    );
    for file in fs::read_dir(path.parent().unwrap()).unwrap() {
        let text = fs::read_to_string(file.unwrap().path()).unwrap();
        assert!(text.ends_with('\n'));
        assert!(text
            .lines()
            .all(|line| line.starts_with("tail-") && line.len() == 8));
    }
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
