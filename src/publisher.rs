use std::collections::HashSet;
use std::path::{Path, PathBuf};

use consul::kv::KV;
use consul::Client;
use regex::Regex;

use log::{debug, info, warn};

use crate::config::Config;
use crate::error::Error;
use crate::kv::KVConfig;
use crate::kv::ServiceConfig;

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

    /// Retrieve a set of existing keys from Consul
    fn read_keys_from_consul(
        &self,
        service_config: &ServiceConfig,
    ) -> Result<HashSet<String>, Error> {
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
            .map(|rec| rec.Key.strip_prefix(&consul_key_prefix).map(String::from))
            .collect::<Option<HashSet<String>>>()
        {
            Some(keys) => Ok(keys),
            None => Err(Error::Generic),
        }
    }

    /// Put all keys from config to Consul
    fn update_keys_in_consul(
        &self,
        kv_config: &KVConfig,
        service_config: &ServiceConfig,
    ) -> Result<(), Error> {
        for (key, value) in kv_config.iter() {
            let consul_key = service_config.consul_key(key.trim_matches(' '))?;
            let consul_val = String::from(value.trim_matches(' ').trim_matches('"'));
            debug!("Put key {}", key);
            let kv_pair = consul::kv::KVPair {
                Key: consul_key,
                Value: consul_val,
                ..Default::default()
            };
            self.client.put(&kv_pair, None).map_err(Error::Consul)?;
        }
        Ok(())
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
        let re: Regex = Regex::new(r"^(?P<service>[[:alnum:]_]+)\.(?P<env>[[:alnum:]_]+)\.conf$")
            .map_err(|_| Error::Generic)?;
        re.captures(filename)
            .map(|cap| {
                (
                    cap.name("service").unwrap().as_str().to_string(),
                    cap.name("env").unwrap().as_str().to_string(),
                )
            })
            .ok_or_else(|| Error::Template(format!("Cannot parse filename {}", filename)))
    }

    /// Read config files in a directory
    fn enumerate_files(&self) -> Result<Vec<PathBuf>, std::io::Error> {
        self.root_path
            .read_dir()?
            .into_iter()
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

    /// Process one KV config file
    pub fn handle_config(&self, config_path: &Path, dryrun: bool) -> Result<(), Error> {
        // Get service and env from config if given. Otherwise parse from filename
        let (service, env): (String, String) =
            if let (Some(the_service), Some(the_env)) = (&self.config.service, &self.config.env) {
                info!("Use service and env name from command line");
                (the_service.clone(), the_env.clone())
            } else {
                info!("Use service and env name from config filename");
                Self::deduce_service_env_from_filename(
                    &config_path
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_string(),
                )?
            };

        let service_config = ServiceConfig::new(self.config.key_template.clone(), service, env);

        info!(
            "Processing config file '{}' with service config {}",
            config_path.to_str().unwrap_or(""),
            service_config,
        );

        let kv_config = KVConfig::new(config_path)?;
        let existing_keys = self.read_keys_from_consul(&service_config)?;
        let removed_keys = kv_config.missing_keys(&existing_keys);

        info!(
            "Read {} keys from config, found {} keys in Consul, will delete {}",
            kv_config.iter().len(),
            existing_keys.len(),
            removed_keys.len()
        );

        if !dryrun {
            self.update_keys_in_consul(&kv_config, &service_config)?;
            info!("Updated keys in consul");

            self.remove_keys_from_consul(&removed_keys, &service_config)?;
            info!("Removed keys from consul");
        }

        Ok(())
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
        // Handle each config file
        info!("Processing {} files", config_paths.len());
        config_paths
            .into_iter()
            .map(|config_path| self.handle_config(&config_path, dryrun))
            .collect::<Result<Vec<_>, Error>>()?;
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
    }
}
