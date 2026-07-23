#[cfg(unix)]
mod unix {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::Value;
    use t4e::catalog::loader::load_catalog;
    use t4e::catalog::models::Platform;
    use t4e::installer::engine::{InstallPolicy, build_install_task};

    #[test]
    fn managed_yewtube_launcher_repairs_only_an_unavailable_player() {
        let root = temporary_directory();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin created");
        write_executable(
            &fake_bin.join("sudo"),
            "#!/bin/sh\n[ \"$1\" = '-n' ] && shift\nexec \"$@\"\n",
        );
        write_executable(&fake_bin.join("apt-get"), "#!/bin/sh\nexit 0\n");
        write_executable(&fake_bin.join("pipx"), "#!/bin/sh\nexit 0\n");
        write_executable(&fake_bin.join("mpv"), "#!/bin/sh\nexit 0\n");
        write_executable(&fake_bin.join("yt"), "#!/bin/sh\nexit 0\n");

        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "yewtube")
            .expect("yewtube exists");
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
            .env_remove("XDG_CONFIG_HOME")
            .status()
            .expect("installer runs");
        assert!(install.success());

        let launcher = root.join(".local/bin/t4e-yewtube");
        let first_run = Command::new(&launcher)
            .env("HOME", &root)
            .env("PATH", &path)
            .env_remove("XDG_CONFIG_HOME")
            .status()
            .expect("launcher runs");
        assert!(first_run.success());
        assert_eq!(configured_player(&root), "mpv");

        write_executable(&fake_bin.join("custom-player"), "#!/bin/sh\nexit 0\n");
        let config = root.join(".config/mps-youtube/config.json");
        fs::write(&config, r#"{"PLAYER":"custom-player","OTHER":"kept"}"#)
            .expect("custom config written");

        let second_run = Command::new(&launcher)
            .env("HOME", &root)
            .env("PATH", &path)
            .env_remove("XDG_CONFIG_HOME")
            .status()
            .expect("launcher runs again");
        assert!(second_run.success());
        assert_eq!(configured_player(&root), "custom-player");
        let value: Value =
            serde_json::from_str(&fs::read_to_string(config).expect("config readable"))
                .expect("config remains valid JSON");
        assert_eq!(value["OTHER"], "kept");

        fs::remove_dir_all(root).expect("test directory removed");
    }

    fn configured_player(root: &Path) -> String {
        let config = fs::read_to_string(root.join(".config/mps-youtube/config.json"))
            .expect("config exists");
        serde_json::from_str::<Value>(&config).expect("config is valid JSON")["PLAYER"]
            .as_str()
            .expect("player is a string")
            .to_string()
    }

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("t4e-yewtube-{}-{nonce}", std::process::id()));
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
