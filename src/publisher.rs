use std::collections::HashMap;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use consul::kv::KV;
use consul::Client;
use derive_more::Add;
use regex::Regex;

use base64::{engine::general_purpose, Engine as _};

use log::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::kv::KVConfig;
use crate::kv::ServiceConfig;

/// Config publishing statistics
#[derive(Default, Add)]
pub struct PublishStats {
    count: usize,
    changed: usize,
    existing: usize,
    removed: usize,
}

/// Config publisher
pub struct Publisher {
    client: Client,
    root_path: PathBuf,
    config: Config,
}

impl Publisher {
    /// Creates a new publisher instance
    pub fn new(config: Config) -> Result<Publisher, Error> {
        let client = Self::create_consul_client(&config)?;
        let root_path = Path::new(&config.config_path).to_path_buf();
        let publisher = Publisher {
            client,
            root_path,
            config,
        };
        publisher.validate()?;
        Ok(publisher)
    }

    /// Validate configuration
    fn validate(&self) -> Result<(), Error> {
        // Validate template string
        if !self.config.key_template.ends_with("/{key}") {
            return Err(Error::Template(format!(
                "key must be at the end of template {}",
                self.config.key_template
            )));
        }
        Ok(())
    }

    /// Create a Consul client instance
    fn create_consul_client(config: &Config) -> Result<Client, Error> {
        let mut consul_config = consul::Config::new().map_err(Error::Consul)?;
        consul_config.address = config.consul_addr.clone();
        consul_config.token = if config.consul_token.is_empty() {
            None
        } else {
            Some(config.consul_token.clone())
        };
        Ok(consul::Client::new(consul_config))
    }

    /// Retrieve a set of existing keys and values from Consul
    fn read_kv_from_consul(
        &self,
        service_config: &ServiceConfig,
    ) -> Result<HashMap<String, String>, Error> {
        debug!("Reading existing keyset");
        let consul_key_prefix = service_config.consul_key("")?;
        // Ensure it ends with / - because we need to produce pure keys without slashes
        if !consul_key_prefix.ends_with('/') {
            return Err(Error::Template(String::from("Key prefix must end with /")));
        }
        // list() returns empty vector if no prefix matched
        let res_keys = self
            .client
            .list(&consul_key_prefix, None)
            .map_err(Error::Consul)?;
        match res_keys
            .0
            .into_iter()
            .map(|rec| {
                (
                    rec.Key.strip_prefix(&consul_key_prefix).map(String::from),
                    rec.Value,
                )
            })
            .map(|rec| match rec {
                (Some(x), y) => Some((x, y)),
                (None, _y) => None,
            })
            .collect::<Option<HashMap<String, String>>>()
        {
            Some(keys) => Ok(keys),
            None => Err(Error::Generic),
        }
    }

    /// Return a list of keys that was changed in local config compared to remote `existing_kvs` in Consul
    fn changed_keys(
        &self,
        service_config: &ServiceConfig,
        kv_config: &KVConfig,
    ) -> Result<HashSet<String>, Error> {
        debug!("Deduce changed keys");
        let mut result: HashSet<String> = HashSet::new();

        for key in kv_config.keys() {
            let consul_key = service_config.consul_key(key.trim_matches(' '))?;
            let resp = self.client.get(&consul_key, None);
            match resp {
                Ok(kv_pair) => {
                    // Remote value from consul
                    let consul_raw_value = kv_pair.0.ok_or(Error::Generic)?;
                    let decoded: Vec<u8> = general_purpose::STANDARD
                        .decode(consul_raw_value.Value)
                        .map_err(|_| Error::Generic)?;
                    let consul_value_str =
                        std::str::from_utf8(&decoded).map_err(|_| Error::Generic)?;
                    let consul_value = self.postprocess_value(consul_value_str);

                    // Local value from kv config
                    let config_value = kv_config.get(key).ok_or(Error::Generic)?;
                    let existing_value = self.postprocess_value(config_value);
                    if consul_value != existing_value {
                        result.insert(key.clone());
                    }
                }
                Err(_) => {
                    // Treat 404 (consul-rust says "Failed to parse JSON response") as a missing value
                    result.insert(key.clone());
                }
            };
        }

        Ok(result)
    }

    /// Put all keys from `keys` hashset from config to Consul
    fn update_keys_in_consul(
        &self,
        kv_config: &KVConfig,
        service_config: &ServiceConfig,
        keys: &HashSet<String>,
    ) -> Result<(), Error> {
        debug!("Put keys to Consul");
        for (key, value) in kv_config.iter() {
            if !keys.contains(key) {
                debug!("Skip unchanged key {}", key);
            } else {
                let consul_key = service_config.consul_key(key.trim_matches(' '))?;
                let consul_val = self.postprocess_value(value);
                debug!("Put key {}", key);
                let kv_pair = consul::kv::KVPair {
                    Key: consul_key,
                    Value: consul_val,
                    ..Default::default()
                };
                self.client.put_raw(&kv_pair, None).map_err(Error::Consul)?;
            }
        }
        Ok(())
    }

    /// Postprocess value read from KV config or Consul
    fn postprocess_value(&self, value: &str) -> String {
        value.trim_matches(' ').trim_matches('"').into()
    }

    /// Remove specified keys (like in KV config, not full) from Consul
    fn remove_keys_from_consul(
        &self,
        keys: &HashSet<String>,
        service_config: &ServiceConfig,
    ) -> Result<(), Error> {
        for key in keys.iter() {
            let consul_key = service_config.consul_key(key.trim_matches(' '))?;
            debug!("Remove key {}", key);
            if consul_key.starts_with('/') {
                return Err(Error::Template(String::from(
                    "Key prefix must start with /",
                )));
            }
            self.client
                .delete(&consul_key, None)
                .map_err(Error::Consul)?;
        }
        Ok(())
    }

    /// Deduce service and env from confug filename
    fn deduce_service_env_from_filename(filename: &String) -> Result<(String, String), Error> {
        let re: Regex = Regex::new(r"^(?P<service>[[:alnum:]_-]+)\.(?P<env>[[:alnum:]_-]+)\.conf$")
            .map_err(|_| Error::Generic)?;
        re.captures(filename)
            .map(|cap| {
                (
                    cap.name("service").unwrap().as_str().to_string(),
                    cap.name("env").unwrap().as_str().to_string(),
                )
            })
            .ok_or_else(|| Error::Template(format!("Cannot parse filename {filename}")))
    }

    /// Read config files in a directory
    fn enumerate_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        self.root_path
            .read_dir()?
            // drop non-conformant files
            .filter(|res| {
                res.as_ref()
                    .map(|e| {
                        // check extension to be '.conf'
                        e.path().extension().and_then(|s| s.to_str()).unwrap_or("") == "conf"
                    })
                    .unwrap_or(false)
            })
            .map(|res| res.map(|e| e.path()))
            .collect()
    }

    /// Parse service and env from config path to a tuple of (path, service, env)
    pub fn parse_config_paths<'a>(
        &self,
        config_path: &'a Path,
    ) -> Result<(&'a Path, String, String), Error> {
        if let (Some(the_service), Some(the_env)) = (&self.config.service, &self.config.env) {
            let (service, env) = (the_service.clone(), the_env.clone());
            info!(
                "Use service {} and env {} name from command line",
                &service, &env
            );
            Ok((config_path, service, env))
        } else {
            let config_filename: String = config_path
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .to_string();
            let (the_service, the_env) = Self::deduce_service_env_from_filename(&config_filename)?;
            info!(
                "Use service {} and env {} name from config filename",
                &the_service, &the_env
            );
            Ok((config_path, the_service, the_env))
        }
    }

    /// Process one KV config file
    pub fn handle_config(
        &self,
        config_path: &Path,
        service: String,
        env: String,
        dryrun: bool,
    ) -> Result<PublishStats, Error> {
        let service_config = ServiceConfig::new(self.config.key_template.clone(), service, env);

        info!(
            "Processing config file '{}' with service config {}",
            config_path.to_str().unwrap_or(""),
            service_config,
        );

        let kv_config = KVConfig::new(config_path)?;
        let existing_kvs = self.read_kv_from_consul(&service_config)?;
        let changed_keys = self.changed_keys(&service_config, &kv_config)?;
        let existing_keys: HashSet<String> = existing_kvs.keys().cloned().collect();
        let removed_keys = kv_config.missing_keys(&existing_keys);

        info!(
            "Read {} keys from config, found {} keys in Consul, will update {}, will delete {}",
            kv_config.iter().len(),
            existing_keys.len(),
            &changed_keys.len(),
            removed_keys.len()
        );

        if !dryrun {
            self.update_keys_in_consul(&kv_config, &service_config, &changed_keys)?;
            info!("Updated keys in consul");

            self.remove_keys_from_consul(&removed_keys, &service_config)?;
            info!("Removed keys from consul");
        }

        Ok(PublishStats {
            count: kv_config.iter().len(),
            existing: existing_keys.len(),
            changed: changed_keys.len(),
            removed: removed_keys.len(),
        })
    }

    // Entry point
    pub fn process(&self, dryrun: bool) -> Result<(), Error> {
        if dryrun {
            warn!("Running in dryrun mode, no changes allowed");
        }

        // Collect config files
        let mut config_paths: Vec<PathBuf> = if self.root_path.is_dir() {
            self.enumerate_files().map_err(Error::ConfigFile)?
        } else {
            vec![self.root_path.clone()]
        };
        config_paths.sort();
        let configs_count = &config_paths.len();
        // Handle each config file
        info!("Processing {} files", configs_count);
        let parsed_paths: Vec<(&Path, String, String)> = config_paths
            .iter()
            .map(|config_path| self.parse_config_paths(config_path))
            .collect::<Result<Vec<_>, Error>>()?;
        info!("Found {} config paths", &parsed_paths.len());
        let filtered_parsed_paths: Vec<(&Path, String, String)> = parsed_paths
            .into_iter()
            .filter(
                |(_config_path, _service, env)| match &self.config.filter_env {
                    Some(filter_env) => env == filter_env,
                    None => true,
                },
            )
            .collect();
        info!(
            "Found {} filtered config paths",
            &filtered_parsed_paths.len()
        );
        let per_config_stats = filtered_parsed_paths
            .into_iter()
            .map(|(config_path, service, env)| {
                self.handle_config(config_path, service, env, dryrun)
            })
            .collect::<Result<Vec<_>, Error>>()?;
        let total_stats = per_config_stats
            .into_iter()
            .fold(PublishStats::default(), |acc, item| acc + item);
        info!(
            "For {} files found {} keys, updated {}, deleted {}",
            configs_count, total_stats.count, total_stats.changed, total_stats.removed,
        );

        Ok(())
    }
}

/// Tests
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_filename() {
        let mut res =
            Publisher::deduce_service_env_from_filename(&"myservice.myenv.conf".to_owned());
        assert_eq!(res.unwrap(), ("myservice".to_string(), "myenv".to_string()));
        assert!(
            Publisher::deduce_service_env_from_filename(&"myservice.myenv.txt".to_owned()).is_err()
        );
        assert!(Publisher::deduce_service_env_from_filename(&"myenv.conf".to_owned()).is_err());
        assert!(Publisher::deduce_service_env_from_filename(&"..conf".to_owned()).is_err());
        assert!(Publisher::deduce_service_env_from_filename(&"s.e.conf".to_owned()).is_ok());
        res = Publisher::deduce_service_env_from_filename(&"my_service.my_env123.conf".to_owned());
        assert_eq!(
            res.unwrap(),
            ("my_service".to_string(), "my_env123".to_string())
        );
        assert!(Publisher::deduce_service_env_from_filename(
            &"myservice.second.myenv.conf".to_owned()
        )
        .is_err());
        res = Publisher::deduce_service_env_from_filename(&"my-service.my-env123.conf".to_owned());
        assert_eq!(
            res.unwrap(),
            ("my-service".to_string(), "my-env123".to_string())
        );
    }
}
