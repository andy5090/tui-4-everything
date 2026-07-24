#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use t4e::catalog::loader::load_catalog;
    use t4e::catalog::models::Platform;
    use t4e::installer::engine::{InstallPolicy, build_install_task};

    #[test]
    fn managed_ascii_camera_launcher_maps_device_and_forwards_render_options() {
        let root = temporary_directory();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin created");
        write_executable(
            &fake_bin.join("sudo"),
            "#!/bin/sh\n[ \"$1\" = '-n' ] && shift\nexec \"$@\"\n",
        );
        write_executable(&fake_bin.join("apt-get"), "#!/bin/sh\nexit 0\n");
        write_executable(
            &fake_bin.join("mpv"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/mpv-args\"\n",
        );

        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "ascii-camera")
            .expect("ASCII Camera exists");
        let installer = tool
            .installers
            .iter()
            .find(|installer| installer.platform == Platform::Linux)
            .expect("Linux installer exists");
        let task = build_install_task(tool, installer, &InstallPolicy::default())
            .expect("install task builds");
        let path = format!("{}:/usr/bin:/bin", fake_bin.display());

        let install = Command::new("sh")
            .arg("-c")
            .arg(&task.command)
            .env("HOME", &root)
            .env("PATH", &path)
            .status()
            .expect("installer runs");
        assert!(install.success());

        let launcher = root.join(".local/bin/t4e-ascii-camera");
        let run = Command::new(launcher)
            .args(["--device", "2", "--vo", "caca", "--vf=hflip"])
            .env("HOME", &root)
            .env("PATH", &path)
            .status()
            .expect("launcher runs");
        assert!(run.success());

        let args = fs::read_to_string(root.join("mpv-args")).expect("mpv arguments recorded");
        assert!(args.contains("--profile=low-latency\n"));
        assert!(args.contains("--no-audio\n"));
        assert!(args.contains("--vo\ncaca\n"));
        assert!(args.contains("--vf=hflip\n"));
        assert!(args.ends_with("av://v4l2:/dev/video2\n"));
        assert!(!args.contains("--device"));

        fs::remove_dir_all(root).expect("test directory removed");
    }

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("t4e-ascii-camera-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).expect("temporary directory created");
        path
    }

    fn write_executable(path: &Path, contents: &str) {
        fs::write(path, contents).expect("fake executable written");
        let mut permissions = fs::metadata(path)
            .expect("fake executable metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("fake executable made executable");
    }
}
