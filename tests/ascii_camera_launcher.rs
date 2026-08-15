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
            "#!/bin/sh\nif [ \"$1\" = '--vo=help' ]; then printf '  tct true-color\\n  caca libcaca\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" > \"$HOME/mpv-args\"\n",
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
        assert!(args.contains("--vo=caca\n"));
        assert!(args.contains("--vf=hflip\n"));
        if cfg!(target_os = "macos") {
            assert!(args.ends_with("av://avfoundation:2:none\n"));
        } else {
            assert!(args.ends_with("av://v4l2:/dev/video2\n"));
        }
        assert!(!args.contains("--device"));

        fs::remove_dir_all(root).expect("test directory removed");
    }

    #[test]
    fn managed_ascii_camera_uses_libcaca_when_mpv_lacks_caca() {
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
            "#!/bin/sh\nif [ \"$1\" = '--vo=help' ]; then printf '  tct true-color\\n'; exit 0; fi\nprintf '%s\\n' \"$@\" > \"$HOME/mpv-args\"\n",
        );
        write_executable(
            &fake_bin.join("ffmpeg"),
            "#!/bin/sh\nfor arg in \"$@\"; do frame=$arg; done\nprintf 'ppm' > \"$frame\"\n",
        );
        write_executable(
            &fake_bin.join("img2txt"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/img2txt-args\"\nprintf 'managed-caca-frame\\n'\n",
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

        let run = Command::new(root.join(".local/bin/t4e-ascii-camera"))
            .args(["--device", "0", "--vo", "caca", "--caca-dither", "ordered4"])
            .env("HOME", &root)
            .env("PATH", &path)
            .output()
            .expect("launcher runs");
        assert!(run.status.success());
        assert!(String::from_utf8_lossy(&run.stdout).contains("managed-caca-frame"));
        assert!(!root.join("mpv-args").exists());

        let args =
            fs::read_to_string(root.join("img2txt-args")).expect("img2txt arguments recorded");
        assert!(args.contains("--format=utf8\n"));
        assert!(args.contains("--dither=ordered4\n"));

        fs::remove_dir_all(root).expect("test directory removed");
    }

    #[test]
    fn termux_ascii_camera_captures_and_renders_a_bounded_frame() {
        let root = temporary_directory();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin created");
        write_executable(
            &fake_bin.join("pkg"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/pkg-args\"\n",
        );
        write_executable(
            &fake_bin.join("termux-camera-photo"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/camera-args\"\nprintf 'jpeg' > \"$3\"\n",
        );
        write_executable(
            &fake_bin.join("chafa"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/chafa-args\"\n",
        );
        write_executable(
            &fake_bin.join("tput"),
            "#!/bin/sh\ncase \"$1\" in cols) printf '90\\n' ;; lines) printf '30\\n' ;; esac\n",
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
            .find(|installer| installer.platform == Platform::Termux)
            .expect("Termux installer exists");
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
        assert_eq!(
            fs::read_to_string(root.join("pkg-args")).expect("pkg arguments recorded"),
            "install\n-y\ntermux-api\nchafa\n"
        );

        let run = Command::new(root.join(".local/bin/t4e-ascii-camera"))
            .args(["--device", "1", "--vo", "caca"])
            .env("HOME", &root)
            .env("PATH", &path)
            .env("TMPDIR", &root)
            .env("T4E_ASCII_CAMERA_FRAMES", "1")
            .status()
            .expect("launcher runs");
        assert!(run.success());

        let camera_args =
            fs::read_to_string(root.join("camera-args")).expect("camera arguments recorded");
        assert!(camera_args.starts_with("-c\n1\n"));
        let chafa_args =
            fs::read_to_string(root.join("chafa-args")).expect("chafa arguments recorded");
        assert!(chafa_args.contains("--probe\noff\n"));
        assert!(chafa_args.contains("--format\nsymbols\n"));
        assert!(chafa_args.contains("--colors\n16\n"));
        assert!(chafa_args.contains("--size\n90x29\n"));
        assert!(!task.requires_privileges);
        assert_eq!(task.check_command.as_deref(), Some("t4e-ascii-camera-v3"));

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
