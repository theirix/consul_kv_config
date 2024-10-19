#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("error locating config: {0}")]
    ConfigFile(std::io::Error),
    #[error("error in KV config: {0}")]
    ConfigFormat(String),
    #[error("duplicated key {0}")]
    DuplicateKey(String),
    #[error("consul error: {0}")]
    Consul(#[from] consul::errors::Error),
    #[error("template error: {0}")]
    Template(String),
    #[error("Consul is unreachable")]
    Unreachable,
    #[error("unknown error")]
    Generic,
}
