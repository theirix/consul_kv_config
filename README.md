# Consul KV Config

[![Crates.io](https://img.shields.io/crates/v/consul_kv_config.svg)](https://crates.io/crates/consul_kv_config)
[![Build](https://github.com/theirix/consul_kv_config/actions/workflows/build.yml/badge.svg)](https://github.com/theirix/consul_kv_config/actions/workflows/build.yml)

**consul_kv_config** is a tool to publish multiple key-value configs to the Consul KV. The tool is aimed for publishing configs in the CI in GitOps-style.

## Installation

Compile from Cargo

    cargo install consul_kv_config

or download prebuilt artefacts from GitHub reeleases.

## Usage

To publish a config file `myservice.production.conf` for the automatically detected
service `myservice` at environment `production`, launch

		consul_kv_config -c myservice.production.conf --consul-addr=http://consul.example.org:8500

This invocation fetches all key-value pairs `KEY=VALUE` from the file and puts value `VALUE` into the `config/service/myservice/production/KEY` Consul key.

A config file must be named `{service}.{env}.conf` so the tool can detect service and environment. It can be overridden by specifying `--service` and ``--env` flags.

To publish all config files (ending in `.conf`) from the specified directory, use:

		consul_kv_config -c configs/


## Advanced usage

		consul_kv_config -c configs/ \
		  --consul-addr=http://consul.example.org:8500 --consul-token=SECRET \
			--key-template="another/template/{service}/envs/{env}/{key}"

The tool can fetch Consul address and token from the standard environment variables `CONSUL_HTTP_ADDR` and `CONSUL_HTTP_TOKEN`.
Path template for Consul key can be overriden with a `key-template` variable.

The value cannot be empty but can contain quotes, equal signs and other string characters.

The log level can be adjusted with `RUST_LOG` variable. For example, set `export RUST_LOG=error` for silent execution.

## Reference

Check `--help` for more actual information.

```
USAGE:
    consul_kv_config [FLAGS] [OPTIONS] --config-path <config-path>

FLAGS:
    -d, --dryrun     Dry run mode (no writes done)
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -c, --config-path <config-path>      Path to config file or directory with configs
        --consul-addr <consul-addr>      Consul address [env: CONSUL_HTTP_ADDR=]  [default: localhost:8500]
        --consul-token <consul-token>    Consul token [env: CONSUL_HTTP_TOKEN=]  [default: ]
    -e, --env <env>                      Environment
        --key-template <key-template>    Consul full key template [default: config/service/{service}/{env}/{key}]
    -s, --service <service>              Service name
```

## Portability

Works on Linux and macOS.


## License

BSD 3-Clause
