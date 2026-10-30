#![cfg(target_os = "macos")]

use std::{
    fs,
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

// Run the real macOS IPC plugin in separate processes with a mock UI runtime,
// so the check cannot interfere with the user's running Franconvert windows.
#[test]
#[ignore = "subprocess fixture, invoked by duplicate_copy_exits_and_notifies_primary"]
fn instance_fixture() {
    let root = PathBuf::from(std::env::var("FRANCONVERT_INSTANCE_TEST_DIR").unwrap());
    let identifier = std::env::var("FRANCONVERT_INSTANCE_TEST_ID").unwrap();
    let role = std::env::var("FRANCONVERT_INSTANCE_TEST_ROLE").unwrap();
    let (sender, receiver) = mpsc::channel();
    let mut context = tauri::test::mock_context(tauri::test::noop_assets());
    context.config_mut().identifier = identifier;
    let app = tauri::test::mock_builder()
        .plugin(tauri_plugin_single_instance::init(move |_, args, _| {
            sender.send(args).unwrap();
        }))
        .build(context)
        .unwrap();

    // A secondary process must exit inside plugin setup, before this point.
    fs::write(root.join(format!("{role}.started")), b"started").unwrap();
    let result = receiver.recv_timeout(Duration::from_secs(10));
    tauri_plugin_single_instance::destroy(&app);
    let args = result.expect("primary must receive the second launch");
    fs::write(root.join("notified"), &args[0]).unwrap();
}

#[test]
fn duplicate_copy_exits_and_notifies_primary() {
    let identifier = format!("com.franconvert.instance-test-{}", std::process::id());
    let root = std::env::temp_dir().join(&identifier);
    fs::create_dir_all(&root).unwrap();
    let executable = std::env::current_exe().unwrap();
    let other_copy = root.join("another-copy");
    fs::copy(&executable, &other_copy).unwrap();
    let socket = PathBuf::from(format!(
        "/tmp/{}_si.sock",
        identifier.replace(['.', '-'], "_")
    ));

    let launch = |path: &std::path::Path, role: &str| {
        Command::new(path)
            .args(["--exact", "instance_fixture", "--ignored", "--nocapture"])
            .env("FRANCONVERT_INSTANCE_TEST_DIR", &root)
            .env("FRANCONVERT_INSTANCE_TEST_ID", &identifier)
            .env("FRANCONVERT_INSTANCE_TEST_ROLE", role)
            .stdout(Stdio::null())
            .spawn()
            .unwrap()
    };

    // Repeat to verify the instance lock is released and a later launch works.
    for _ in 0..2 {
        let mut primary = launch(&executable, "primary");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !socket.exists() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        if !socket.exists() {
            let _ = primary.kill();
            let _ = primary.wait();
            panic!("single-instance listener did not start");
        }
        let secondary = launch(&other_copy, "secondary").wait().unwrap();
        let primary_status = primary.wait().unwrap();
        assert!(secondary.success(), "duplicate launch must exit cleanly");
        assert!(
            primary_status.success(),
            "primary must receive the launch notification"
        );
        assert!(
            !root.join("secondary.started").exists(),
            "duplicate reached application setup"
        );
        assert_eq!(
            fs::read_to_string(root.join("notified")).unwrap(),
            other_copy.to_string_lossy()
        );
        assert!(!socket.exists(), "instance lock must be released on exit");
    }
    fs::remove_dir_all(root).unwrap();
}
