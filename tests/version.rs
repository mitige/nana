//! --version must print the crate name and the package version, nothing more:
//! users script against this line, so it stays "nana x.y.z".

use std::process::Command;

#[test]
fn version_prints_name_and_package_version() {
    for flag in ["--version", "-V"] {
        let out = Command::new(env!("CARGO_BIN_EXE_nana"))
            .arg(flag)
            .output()
            .expect("nana should run");
        assert!(out.status.success(), "{flag} must exit 0");
        let stdout = String::from_utf8(out.stdout).expect("version output is utf-8");
        assert_eq!(
            stdout,
            format!("nana {}\n", env!("CARGO_PKG_VERSION")),
            "{flag} must print the name and the package version"
        );
    }
}
