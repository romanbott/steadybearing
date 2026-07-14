use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug, Clone)]
#[command(name = "sb-daemon")]
pub struct Args {
    #[arg(long, default_value = "/tmp/sb.sock")]
    pub socket_path: String,

    #[arg(long, default_value = "/var/log/sb/telemetry.db")]
    pub db_path: String,

    #[arg(long, default_value = "/var/log/sb/archive.db")]
    pub db_archive_path: String,

    #[arg(long)]
    pub config_file: Option<String>,

    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    #[serde(default = "default_db_path")]
    pub db_path: String,

    #[serde(default = "default_db_archive_path")]
    pub db_archive_path: String,

    #[serde(default = "default_pwm_baseline")]
    pub pwm_baseline: u8,

    #[serde(default = "default_fan_soft_min")]
    pub fan_soft_min: f32,

    #[serde(default = "default_fan_soft_max")]
    pub fan_soft_max: f32,

    #[serde(default = "default_fan_hard_max")]
    pub fan_hard_max: f32,

    #[serde(default = "default_fan_dwell_secs")]
    pub fan_dwell_secs: f32,

    #[serde(default = "default_fan_step")]
    pub fan_step: u8,

    #[serde(default = "default_fan_hard_ramp_secs")]
    pub fan_hard_ramp_secs: f32,

    #[serde(default = "default_fan_drift_temp")]
    pub fan_drift_temp: f32,

    #[serde(default = "default_fan_drift_secs")]
    pub fan_drift_secs: f32,

    #[serde(default = "default_db_flush_secs")]
    pub db_flush_secs: f32,

    #[serde(default = "default_db_retention_days")]
    pub db_retention_days: u32,
}

fn default_socket_path() -> String {
    "/tmp/sb.sock".into()
}

fn default_db_path() -> String {
    "/var/log/sb/telemetry.db".into()
}

fn default_db_archive_path() -> String {
    "/var/log/sb/archive.db".into()
}

fn default_pwm_baseline() -> u8 {
    110
}

fn default_fan_soft_min() -> f32 {
    40.0
}

fn default_fan_soft_max() -> f32 {
    60.0
}

fn default_fan_hard_max() -> f32 {
    80.0
}

fn default_fan_dwell_secs() -> f32 {
    10.0
}

fn default_fan_step() -> u8 {
    5
}

fn default_fan_hard_ramp_secs() -> f32 {
    10.0
}

fn default_fan_drift_temp() -> f32 {
    50.0
}

fn default_fan_drift_secs() -> f32 {
    30.0
}

fn default_db_flush_secs() -> f32 {
    600.0
}

fn default_db_retention_days() -> u32 {
    7
}

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: String,
    pub db_path: String,
    pub db_archive_path: String,
    pub db_flush_secs: f32,
    pub db_retention_days: u32,
    pub pwm_baseline: u8,
    pub fan_soft_min: f32,
    pub fan_soft_max: f32,
    pub fan_hard_max: f32,
    pub fan_dwell_secs: f32,
    pub fan_step: u8,
    pub fan_hard_ramp_secs: f32,
    pub fan_drift_temp: f32,
    pub fan_drift_secs: f32,
    pub verbose: bool,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let args = Args::parse();

        let config_file = if let Some(ref path) = args.config_file {
            Some(Self::load_config_file(path)?)
        } else if std::path::Path::new("/etc/sb/sb.toml").exists() {
            Some(Self::load_config_file("/etc/sb/sb.toml")?)
        } else if std::path::Path::new("sb.toml").exists() {
            Some(Self::load_config_file("sb.toml")?)
        } else {
            None
        };

        Ok(Config {
            socket_path: config_file
                .as_ref()
                .map(|c| c.socket_path.clone())
                .unwrap_or(args.socket_path),
            db_path: config_file
                .as_ref()
                .map(|c| c.db_path.clone())
                .unwrap_or(args.db_path),
            db_archive_path: config_file
                .as_ref()
                .map(|c| c.db_archive_path.clone())
                .unwrap_or(args.db_archive_path),
            db_flush_secs: config_file
                .as_ref()
                .map(|c| c.db_flush_secs)
                .unwrap_or(default_db_flush_secs()),
            db_retention_days: config_file
                .as_ref()
                .map(|c| c.db_retention_days)
                .unwrap_or(default_db_retention_days()),
            pwm_baseline: config_file
                .as_ref()
                .map(|c| c.pwm_baseline)
                .unwrap_or(default_pwm_baseline()),
            fan_soft_min: config_file
                .as_ref()
                .map(|c| c.fan_soft_min)
                .unwrap_or(default_fan_soft_min()),
            fan_soft_max: config_file
                .as_ref()
                .map(|c| c.fan_soft_max)
                .unwrap_or(default_fan_soft_max()),
            fan_hard_max: config_file
                .as_ref()
                .map(|c| c.fan_hard_max)
                .unwrap_or(default_fan_hard_max()),
            fan_dwell_secs: config_file
                .as_ref()
                .map(|c| c.fan_dwell_secs)
                .unwrap_or(default_fan_dwell_secs()),
            fan_step: config_file
                .as_ref()
                .map(|c| c.fan_step)
                .unwrap_or(default_fan_step()),
            fan_hard_ramp_secs: config_file
                .as_ref()
                .map(|c| c.fan_hard_ramp_secs)
                .unwrap_or(default_fan_hard_ramp_secs()),
            fan_drift_temp: config_file
                .as_ref()
                .map(|c| c.fan_drift_temp)
                .unwrap_or(default_fan_drift_temp()),
            fan_drift_secs: config_file
                .as_ref()
                .map(|c| c.fan_drift_secs)
                .unwrap_or(default_fan_drift_secs()),
            verbose: args.verbose,
        })
    }

    fn load_config_file(path: &str) -> anyhow::Result<ConfigFile> {
        let contents = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }
}
