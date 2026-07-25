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
    fn managed_youtube_tui_launcher_routes_only_external_mpv_playback() {
        let root = temporary_directory();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin created");
        write_executable(
            &fake_bin.join("sudo"),
            "#!/bin/sh\n[ \"$1\" = '-n' ] && shift\nexec \"$@\"\n",
        );
        write_executable(&fake_bin.join("apt-get"), "#!/bin/sh\nexit 0\n");
        write_executable(&fake_bin.join("cargo"), "#!/bin/sh\nexit 0\n");
        write_executable(
            &fake_bin.join("python3"),
            concat!(
                "#!/bin/sh\n",
                "if [ \"$1\" = '-m' ] && [ \"$2\" = 'venv' ]; then\n",
                "  mkdir -p \"$3/bin\"\n",
                "  printf '%s\\n' '#!/bin/sh' 'exit 0' > \"$3/bin/python\"\n",
                "  chmod +x \"$3/bin/python\"\n",
                "  exit 0\n",
                "fi\n",
                "exec /usr/bin/python3 \"$@\"\n"
            ),
        );
        write_executable(
            &fake_bin.join("mpv"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$T4E_TEST_MPV_LOG\"\n",
        );
        write_executable(
            &fake_bin.join("youtube-tui"),
            "#!/bin/sh\nexec mpv video-url\n",
        );

        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "youtube-tui")
            .expect("YouTube TUI exists");
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
            .env_remove("XDG_DATA_HOME")
            .status()
            .expect("installer runs");
        assert!(install.success());

        let launcher = root.join(".local/bin/t4e-youtube-tui");
        let log = root.join("mpv.log");
        for (renderer, expected) in [
            ("MPV", "video-url\n"),
            (
                "TCT",
                "--vo=tct\n--profile=sw-fast\n--really-quiet\nvideo-url\n",
            ),
            ("CACA", "--vo=caca\n--really-quiet\nvideo-url\n"),
        ] {
            let run = Command::new(&launcher)
                .args(["--renderer", renderer])
                .env("HOME", &root)
                .env("PATH", &path)
                .env("T4E_TEST_MPV_LOG", &log)
                .env_remove("XDG_DATA_HOME")
                .status()
                .expect("launcher runs");
            assert!(run.success(), "{renderer} launcher succeeds");
            assert_eq!(
                fs::read_to_string(&log).expect("mpv invocation logged"),
                expected,
                "{renderer} arguments"
            );
        }

        fs::remove_dir_all(root).expect("test directory removed");
    }

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("t4e-youtube-tui-{}-{nonce}", std::process::id()));
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
