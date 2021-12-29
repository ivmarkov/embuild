use std::collections::HashMap;
use std::ffi::OsStr;
use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context};
use embuild::espidf::EspIdfBuildInfo;
use embuild::utils::CmdError;
use embuild::{cmd, path_buf};
use structopt::StructOpt;

use crate::build::{self, BuildError};

#[derive(Debug, thiserror::Error)]
#[error("Could not open menuconfig")]
pub enum MenuconfigError {
    Build(#[from] BuildError),
    Cmd(#[from] CmdError),
    Anyhow(#[from] anyhow::Error),
    Io(#[from] std::io::Error),
    Serde(#[from] serde_json::Error),
}

#[derive(StructOpt)]
pub struct MenuconfigOpts {
    #[structopt(flatten)]
    build_opts: build::BuildOpts,
    /// Path to the esp-idf build info json file.
    ///
    /// If this option is not specified cargo-idf will perform a `cargo build` in the
    /// current directory.
    #[structopt(long)]
    idf_build_info: Option<PathBuf>,
}

pub fn run(opts: MenuconfigOpts) -> Result<(), MenuconfigError> {
    let build_info_json = if let Some(path) = opts.idf_build_info {
        path
    } else {
        build::run(opts.build_opts)?.esp_idf_build_info_json
    };

    let EspIdfBuildInfo {
        venv_python,
        esp_idf_dir,
        build_dir,
        project_dir,
        sdkconfig_defaults,
        ..
    } = embuild::espidf::EspIdfBuildInfo::from_json(&build_info_json).with_context(|| {
        anyhow!(
            "Failed to get esp-idf build info from '{}'",
            build_info_json.display()
        )
    })?;
    let sdkconfig_defaults = sdkconfig_defaults.unwrap_or_default();

    std::env::set_var("IDF_PATH", &esp_idf_dir);

    let prepare_kconfig_py = path_buf![
        &esp_idf_dir,
        "tools",
        "kconfig_new",
        "prepare_kconfig_files.py"
    ];
    let confgen_py = path_buf![&esp_idf_dir, "tools", "kconfig_new", "confgen.py"];

    let kconfig = path_buf![&esp_idf_dir, "Kconfig"];
    let sdkconfig_rename = path_buf![&esp_idf_dir, "sdkconfig.rename"];
    let build_sdkconfig = path_buf![&project_dir, "sdkconfig"];
    let config_env = path_buf![&build_dir, "config.env"];

    cmd!(&venv_python, &prepare_kconfig_py, "--env-file", &config_env)?;

    let defaults = sdkconfig_defaults
        .iter()
        .map(|d| [OsStr::new("--defaults"), d.as_os_str()])
        .flatten();

    cmd!(
        &venv_python, &confgen_py,
            "--kconfig", &kconfig,
            "--sdkconfig-rename", &sdkconfig_rename,
            "--config", &build_sdkconfig,
            @defaults,
            "--env-file", &config_env,
            "--dont-write-deprecated",
            "--output", "config", &build_sdkconfig
    )?;

    let env: HashMap<String, String> = serde_json::from_reader(fs::File::open(&config_env)?)?;
    cmd!(
        &venv_python, "-m", "menuconfig", &kconfig;
            envs=(env),
            env=("KCONFIG_CONFIG", &build_sdkconfig)
    )?;

    Ok(())
}
