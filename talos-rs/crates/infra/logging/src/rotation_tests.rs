use super::*;
use crate::tests::temp_log_path;

fn retained(writer: &RotatingFile) -> String {
    writer
        .archives()
        .unwrap()
        .into_iter()
        .map(|(_, path)| fs::read_to_string(path).unwrap())
        .chain(std::iter::once(fs::read_to_string(&writer.path).unwrap()))
        .collect()
}

#[test]
fn rotation_preserves_existing_bytes_and_limits_complete_archives() {
    let path = temp_log_path("rotation.log");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let existing = "an existing log larger than the new rotation limit\n";
    fs::write(&path, existing).unwrap();
    let limits = LogRotation {
        max_bytes: 12,
        max_files: 3,
    };
    let mut writer = RotatingFile::open(path.clone(), limits.clone()).unwrap();
    writer.write_all(b"one\n").unwrap();
    assert_eq!(
        fs::read_to_string(&writer.archives().unwrap()[0].1).unwrap(),
        existing
    );
    drop(writer);
    let mut writer = RotatingFile::open(path.clone(), limits).unwrap();
    for record in [b"two\n".as_slice(), b"three\n", b"four\n", b"five\n"] {
        writer.write_all(record).unwrap();
    }
    assert_eq!(retained(&writer), "one\ntwo\nthree\nfour\nfive\n");
    assert_eq!(writer.archives().unwrap().len() + 1, 3);
    let oversized = "an oversized diagnostic must remain a complete record\n";
    writer.write_all(oversized.as_bytes()).unwrap();
    writer.flush().unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), oversized);
    assert_eq!(writer.archives().unwrap().len() + 1, 3);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn independent_writers_reopen_the_active_path_after_rotation() {
    let path = temp_log_path("shared.log");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let limits = LogRotation {
        max_bytes: 64,
        max_files: 32,
    };
    let writers: Vec<_> = ["left", "right"]
        .into_iter()
        .map(|name| {
            let (mut writer, guard) =
                crate::create_file_writer(path.to_str().unwrap(), Some(&limits)).unwrap();
            std::thread::spawn(move || {
                for sequence in 0..40 {
                    // tracing-subscriber submits a complete formatted event as one buffer.
                    writer
                        .write_all(format!("{name}-{sequence:03}\n").as_bytes())
                        .unwrap();
                }
                drop(writer);
                drop(guard);
            })
        })
        .collect();
    for writer in writers {
        writer.join().unwrap();
    }
    let writer = RotatingFile::open(path.clone(), limits).unwrap();
    let actual: std::collections::BTreeSet<_> =
        retained(&writer).lines().map(str::to_owned).collect();
    let expected: std::collections::BTreeSet<_> = ["left", "right"]
        .into_iter()
        .flat_map(|name| (0..40).map(move |sequence| format!("{name}-{sequence:03}")))
        .collect();
    assert_eq!(actual, expected);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn worker_guard_flushes_queued_records_across_rotations() {
    let path = temp_log_path("queued.log");
    let limits = LogRotation {
        max_bytes: 128,
        max_files: 32,
    };
    let (mut writer, guard) =
        crate::create_file_writer(path.to_str().unwrap(), Some(&limits)).unwrap();
    let mut expected = String::new();
    for sequence in 0..100 {
        let record = format!("queued-{sequence:03}\n");
        writer.write_all(record.as_bytes()).unwrap();
        expected.push_str(&record);
    }
    drop(writer);
    drop(guard);
    let writer = RotatingFile::open(path.clone(), limits).unwrap();
    assert_eq!(retained(&writer), expected);
    fs::remove_dir_all(path.parent().unwrap()).unwrap();
}
