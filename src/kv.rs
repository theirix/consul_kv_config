use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::io::BufRead;
use std::ops::Deref;
use std::path::Path;

use strfmt::strfmt;

use crate::error::Error;

/// Represent service configuration
pub struct ServiceConfig {
    key_template: String,
    service: String,
    env: String,
}

impl fmt::Display for ServiceConfig {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(fmt, "service '{}' with env '{}'", self.service, self.env)
    }
}

impl ServiceConfig {
    pub fn new(key_template: String, service: String, env: String) -> ServiceConfig {
        ServiceConfig {
            key_template,
            service,
            env,
        }
    }

    /// Create full Consul key from simple key
    pub fn consul_key(&self, key: &str) -> Result<String, Error> {
        let vars: HashMap<String, String> = HashMap::from([
            (String::from("service"), self.service.clone()),
            (String::from("env"), self.env.clone()),
            (String::from("key"), String::from(key)),
        ]);

        match strfmt(&self.key_template, &vars) {
            Ok(s) => Ok(s),
            Err(e) => Err(Error::Template(e.to_string())),
        }
    }
}

/// Represents KV configuration file
pub struct KVConfig {
    kv: HashMap<String, String>,
}

impl KVConfig {
    /// Create KV config from the config file
    pub fn new(file_path: &Path) -> Result<Self, Error> {
        let file = std::fs::File::open(file_path).map_err(Error::ConfigFile)?;
        let res_lines: Result<Vec<_>, _> = std::io::BufReader::new(file)
            .lines()
            .into_iter()
            .map(|line| Self::handle_line(&line.unwrap()))
            .collect();
        let lines: Vec<_> = res_lines.map_err(|_| Error::Generic)?;
        let mut keys = HashSet::new();
        // Do not allow duplicate keys
        for (key, _) in &lines {
            if !keys.insert(key) {
                return Err(Error::DuplicateKey(key.clone()));
            }
        }
        let hash_map: HashMap<String, String> = lines
            .into_iter()
            // skip items starting with underscore
            .filter(|(k, _)| !k.starts_with('_') && !k.starts_with('#'))
            .collect();
        Ok(KVConfig { kv: hash_map })
    }

    /// Find keys that are in `existing_keys` but not in this config
    pub fn missing_keys(&self, existing_keys: &HashSet<String>) -> HashSet<String> {
        existing_keys
            .iter()
            .filter_map(|existing_key| {
                if self.kv.contains_key(existing_key) {
                    None
                } else {
                    Some(existing_key.clone())
                }
            })
            .collect()
    }

    /// Parse one key-value from the config line
    fn handle_line(line: &str) -> Result<(String, String), Error> {
        let (k, v) = line
            .split_once('=')
            .ok_or_else(|| Error::ConfigFormat(String::from("No delimiter found")))?;
        if k.is_empty() {
            return Err(Error::ConfigFormat("Empty key".to_string()));
        }
        if v.is_empty() {
            return Err(Error::ConfigFormat("Empty value".to_string()));
        }
        Ok((k.trim().to_string(), v.trim().to_string()))
    }
}

// Allow to construct iterator for KVConfig
impl Deref for KVConfig {
    type Target = HashMap<String, String>;

    fn deref(&self) -> &Self::Target {
        &self.kv
    }
}

/// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_kv_line() {
        assert_eq!(
            KVConfig::handle_line("foo=bar").unwrap(),
            ("foo".to_string(), "bar".to_string())
        );
        assert_eq!(
            KVConfig::handle_line("foo = bar ").unwrap(),
            ("foo".to_string(), "bar".to_string())
        );
        assert!(KVConfig::handle_line("foo=").is_err());
        assert!(KVConfig::handle_line("=bar").is_err());
        assert!(KVConfig::handle_line("foo").is_err());
        assert_eq!(
            KVConfig::handle_line("foo=bar=baz").unwrap(),
            ("foo".to_string(), "bar=baz".to_string())
        );
    }

    #[test]
    fn test_create_key() {
        let res = ServiceConfig::new(
            "config/{service}_x_{env}*{key}".to_string(),
            "my".to_string(),
            "MYENV".to_string(),
        )
        .consul_key(&"KEY".to_string());
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), "config/my_x_MYENV*KEY");
    }

    #[test]
    fn test_create_key_omit() {
        // Can omit one of templates
        let res = ServiceConfig::new(
            "config/_x_{key}*{env}".to_string(),
            "my".to_string(),
            "MYENV".to_string(),
        )
        .consul_key(&"KEY".to_string());
        assert!(res.is_ok());
    }
}
