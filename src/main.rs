mod config;
mod error;
mod kv;
mod publisher;

use crate::config::Config;
use crate::error::Error;
use crate::publisher::Publisher;

use log::{error, info};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Opt {
    /// Consul address
    #[structopt(
        long = "consul-addr",
        default_value = "http://localhost:8500",
        env = "CONSUL_HTTP_ADDR"
    )]
    consul_addr: String,

    /// Consul token
    #[structopt(long = "consul-token", env = "CONSUL_HTTP_TOKEN", default_value = "")]
    consul_token: String,

    /// Service name
    #[structopt(short, long)]
    service: Option<String>,

    /// Environment
    #[structopt(short, long)]
    env: Option<String>,

    /// Filter by environment
    #[structopt(short, long)]
    filter_env: Option<String>,

    /// Dry run mode (no writes done)
    #[structopt(short, long)]
    dryrun: bool,

    /// Path to config file or directory with configs
    #[structopt(short, long)]
    config_path: String,

    /// Consul full key template
    #[structopt(
        long = "key-template",
        default_value = "config/service/{service}/{env}/{key}"
    )]
    key_template: String,

    /// Timeout for Consul to be ready in seconds
    #[structopt(short, long, default_value = "60")]
    timeout: u64,
}

fn main() -> Result<(), Error> {
    env_logger::Builder::from_default_env()
        .write_style(if atty::is(atty::Stream::Stdout) {
            env_logger::WriteStyle::Auto
        } else {
            env_logger::WriteStyle::Never
        })
        .init();

    let opt = Opt::from_args();
    let config = Config {
        consul_addr: opt.consul_addr,
        consul_token: opt.consul_token,
        config_path: opt.config_path,
        service: opt.service,
        env: opt.env,
        filter_env: opt.filter_env,
        key_template: opt.key_template,
        timeout: opt.timeout,
    };

    let result: Result<(), Error> = match Publisher::new(config) {
        Ok(publisher) => publisher.process(opt.dryrun),
        Err(err) => Err(err),
    };
    match result {
        Ok(_) => {
            info!("Done");
            Ok(())
        }
        Err(err) => {
            error!("Error: {}", err);
            Err(err)
        }
    }
}
