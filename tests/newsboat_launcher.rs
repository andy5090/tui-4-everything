#[cfg(unix)]
mod unix {
    use std::fs;
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};
    use std::time::{SystemTime, UNIX_EPOCH};

    use t4e::catalog::loader::load_catalog;
    use t4e::catalog::models::Platform;
    use t4e::installer::engine::{InstallPolicy, build_install_task};

    #[test]
    fn managed_newsboat_launcher_collects_and_reuses_the_first_feed() {
        let root = temporary_directory();
        let fake_bin = root.join("bin");
        fs::create_dir_all(&fake_bin).expect("fake bin created");
        write_executable(
            &fake_bin.join("sudo"),
            "#!/bin/sh\n[ \"$1\" = '-n' ] && shift\nexec \"$@\"\n",
        );
        write_executable(&fake_bin.join("snap"), "#!/bin/sh\nexit 0\n");
        write_executable(
            &fake_bin.join("newsboat"),
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$HOME/newsboat-args\"\n",
        );

        let catalog = load_catalog(Path::new("registry/catalog.yaml")).expect("catalog loads");
        let tool = catalog
            .tools
            .iter()
            .find(|tool| tool.id == "newsboat")
            .expect("newsboat exists");
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

        let launcher = root.join(".local/bin/t4e-newsboat");
        let mut child = Command::new(&launcher)
            .env("HOME", &root)
            .env("PATH", &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .expect("launcher starts");
        child
            .stdin
            .take()
            .expect("launcher stdin")
            .write_all(b"https://example.com/feed.xml\n")
            .expect("feed entered");
        let output = child.wait_with_output().expect("launcher exits");
        assert!(output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .contains("Newsboat needs at least one RSS or Atom feed")
        );

        let urls = root.join("snap/newsboat/common/t4e/urls");
        assert_eq!(
            fs::read_to_string(urls).expect("managed URL file exists"),
            "https://example.com/feed.xml\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("newsboat-args")).expect("newsboat was invoked"),
            format!("-u\n{}/snap/newsboat/common/t4e/urls\n", root.display())
        );

        fs::remove_dir_all(root).expect("test directory removed");
    }

    fn temporary_directory() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("t4e-newsboat-{}-{nonce}", std::process::id()));
        fs::create_dir(&path).expect("test directory created");
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
