pub mod big_clock;

/// Shell command that launches a builtin app inside a managed terminal.
///
/// Builtin apps ship inside the t4e executable, so the command targets the
/// current binary with the hidden `builtin` subcommand. This keeps them
/// working for packaged releases outside the source tree.
pub fn launch_command(app: &str) -> String {
    let exe = std::env::current_exe()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|_| "t4e".to_string());
    format!("{exe} builtin {app}")
}
