use serde::{Deserialize, Serialize};
use std::{net::IpAddr, path::Path, str::FromStr};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Netconfig {
    pub static_dns_servers: Option<Vec<String>>,
    pub static_dns_searchlist: Option<Vec<String>>,
}

impl Netconfig {
    pub fn static_dns_servers(&self) -> Result<Vec<IpAddr>, std::net::AddrParseError> {
        if let Some(static_dns_servers) = &self.static_dns_servers {
            static_dns_servers
                .iter()
                .map(|x| IpAddr::from_str(x))
                .collect()
        } else {
            Ok(vec![])
        }
    }
}

pub fn read_netconfig(path: impl AsRef<Path>) -> Result<Option<Netconfig>, anyhow::Error> {
    if let Err(e) = dotenv::from_filename(path) {
        return Err(e.into());
    };
    if let Ok(dns_policy) = dotenv::var("NETCONFIG_DNS_POLICY") {
        let dns_policies: Vec<&str> = dns_policy.split(' ').collect();
        if dns_policies.len() > 1 {
            println!("{:?}", dns_policies);
            return Err(anyhow::anyhow!(
                "For NETCONFIG_DNS_POLICY only single policies are supported"
            ));
        }
        let dns_policy = dns_policies[0];
        match dns_policy {
            "" => return Ok(None),
            "STATIC" => (),
            "auto" => (),
            _ => {
                return Err(anyhow::anyhow!(
                    "For NETCONFIG_DNS_POLICY only \"STATIC\" and \"auto\" are supported"
                ))
            }
        }
    }
    let mut netconfig = Netconfig::default();
    if let Ok(static_dns_servers) = dotenv::var("NETCONFIG_DNS_STATIC_SERVERS") {
        if !static_dns_servers.is_empty() {
            netconfig.static_dns_servers = Some(
                static_dns_servers
                    .split(' ')
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            );
        }
    }
    if let Ok(static_dns_searchlist) = dotenv::var("NETCONFIG_DNS_STATIC_SEARCHLIST") {
        if !static_dns_searchlist.is_empty() {
            netconfig.static_dns_searchlist = Some(
                static_dns_searchlist
                    .split(' ')
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            );
        }
    }
    Ok(Some(netconfig))
}
