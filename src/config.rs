#[derive(Debug)]
pub struct Config {
    pub consul_addr: String,
    pub consul_token: String,
    pub service: Option<String>,
    pub env: Option<String>,
    pub filter_env: Option<String>,
    pub config_path: String,
    pub key_template: String,
    pub timeout: u64,
}
