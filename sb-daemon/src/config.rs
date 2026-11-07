use clap::Parser;
use serde::Deserialize;

#[derive(Parser, Debug, Clone)]
#[command(name = "sb-daemon")]
pub struct Args {
    #[arg(long, default_value_t = String::from(sb_protocol::types::DEFAULT_SOCKET_PATH))]
    pub socket_path: String,

    #[arg(long, default_value_t = String::from(sb_protocol::types::DEFAULT_DB_PATH))]
    pub db_path: String,

    #[arg(long)]
    pub config_file: Option<String>,

    #[arg(short, long, default_value = "false")]
    pub verbose: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigFile {
    #[serde(default = "default_socket_path")]
    pub socket_path: String,

    #[serde(default)]
    pub socket_gid: Option<u32>,

    #[serde(default = "default_db_path")]
    pub db_path: String,

    #[serde(default)]
    pub retention: RetentionConfig,

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
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_ten_second_days")]
    pub ten_second_days: u32,

    #[serde(default = "default_minute_days")]
    pub minute_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            ten_second_days: default_ten_second_days(),
            minute_days: default_minute_days(),
        }
    }
}

fn default_socket_path() -> String {
    sb_protocol::types::DEFAULT_SOCKET_PATH.into()
}

fn default_db_path() -> String {
    sb_protocol::types::DEFAULT_DB_PATH.into()
}

fn default_ten_second_days() -> u32 {
    1
}

fn default_minute_days() -> u32 {
    7
}

fn default_pwm_baseline() -> u8 {
    sb_protocol::types::PWM_BASELINE
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
    60.0
}

#[derive(Debug, Clone)]
pub struct Config {
    pub socket_path: String,
    pub socket_gid: Option<u32>,
    pub db_path: String,
    pub db_flush_secs: f32,
    pub retention: RetentionConfig,
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

        let config = Config {
            socket_path: config_file
                .as_ref()
                .map(|c| c.socket_path.clone())
                .unwrap_or(args.socket_path),
            socket_gid: config_file.as_ref().and_then(|c| c.socket_gid),
            db_path: config_file
                .as_ref()
                .map(|c| c.db_path.clone())
                .unwrap_or(args.db_path),
            db_flush_secs: config_file
                .as_ref()
                .map(|c| c.db_flush_secs)
                .unwrap_or(default_db_flush_secs()),
            retention: config_file
                .as_ref()
                .map(|c| c.retention.clone())
                .unwrap_or_default(),
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
        };

        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(self.fan_step >= 1, "fan_step must be at least 1");

        anyhow::ensure!(
            self.fan_soft_min.is_finite()
                && self.fan_soft_max.is_finite()
                && self.fan_hard_max.is_finite(),
            "fan temperature thresholds must be finite"
        );
        anyhow::ensure!(
            self.fan_soft_min < self.fan_soft_max,
            "fan_soft_min ({}) must be below fan_soft_max ({})",
            self.fan_soft_min,
            self.fan_soft_max
        );
        anyhow::ensure!(
            self.fan_soft_max < self.fan_hard_max,
            "fan_soft_max ({}) must be below fan_hard_max ({})",
            self.fan_soft_max,
            self.fan_hard_max
        );

        anyhow::ensure!(
            self.fan_dwell_secs.is_finite() && self.fan_dwell_secs >= 0.0,
            "fan_dwell_secs must be non-negative"
        );
        anyhow::ensure!(
            self.fan_hard_ramp_secs.is_finite() && self.fan_hard_ramp_secs > 0.0,
            "fan_hard_ramp_secs must be positive"
        );
        anyhow::ensure!(
            self.fan_drift_secs.is_finite() && self.fan_drift_secs > 0.0,
            "fan_drift_secs must be positive"
        );
        anyhow::ensure!(
            self.fan_drift_temp.is_finite(),
            "fan_drift_temp must be finite"
        );

        anyhow::ensure!(
            self.db_flush_secs.is_finite()
                && self.db_flush_secs > 0.0
                && self.db_flush_secs <= 3600.0,
            "db_flush_secs must be within (0, 3600] to fit the in-memory window"
        );

        Ok(())
    }

    fn load_config_file(path: &str) -> anyhow::Result<ConfigFile> {
        let contents = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&contents)?)
    }
}

#[cfg(test)]
mod tests {
    use super::{Config, ConfigFile, RetentionConfig};

    fn valid_config() -> Config {
        Config {
            socket_path: "/tmp/sb.sock".into(),
            socket_gid: None,
            db_path: "/tmp/sb.db".into(),
            db_flush_secs: 60.0,
            retention: RetentionConfig::default(),
            pwm_baseline: 110,
            fan_soft_min: 40.0,
            fan_soft_max: 60.0,
            fan_hard_max: 80.0,
            fan_dwell_secs: 10.0,
            fan_step: 5,
            fan_hard_ramp_secs: 10.0,
            fan_drift_temp: 50.0,
            fan_drift_secs: 30.0,
            verbose: false,
        }
    }

    #[test]
    fn accepts_default_config() {
        assert!(valid_config().validate().is_ok());
    }

    #[test]
    fn rejects_zero_fan_step() {
        let mut config = valid_config();
        config.fan_step = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_inverted_thresholds() {
        let mut config = valid_config();
        config.fan_soft_min = 70.0;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.fan_soft_max = 90.0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn retention_defaults_apply() {
        let parsed: ConfigFile =
            toml::from_str("db_path = \"/tmp/history.db\"").unwrap();
        assert_eq!(parsed.retention.ten_second_days, 1);
        assert_eq!(parsed.retention.minute_days, 7);

        let parsed: ConfigFile = toml::from_str(
            "[retention]\nten_second_days = 2\n",
        )
        .unwrap();
        assert_eq!(parsed.retention.ten_second_days, 2);
        assert_eq!(parsed.retention.minute_days, 7);
    }

    #[test]
    fn rejects_out_of_range_flush_interval() {
        let mut config = valid_config();
        config.db_flush_secs = 0.0;
        assert!(config.validate().is_err());

        let mut config = valid_config();
        config.db_flush_secs = 7200.0;
        assert!(config.validate().is_err());
    }
}
