use agama_network::{model::Connection, NetworkState};
use globset::Glob;
use serde::{Deserialize, Serialize};
use std::{net::IpAddr, path::Path};

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Netconfig {
    pub static_dns_servers: Vec<IpAddr>,
    pub static_dns_searchlist: Option<Vec<String>>,
    pub dns_policy: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn read_netconfig(path: impl AsRef<Path>) -> Result<Option<Netconfig>, anyhow::Error> {
    if let Err(e) = dotenv::from_filename(path) {
        return Err(e.into());
    };
    handle_netconfig_values()
}

fn handle_netconfig_values() -> Result<Option<Netconfig>, anyhow::Error> {
    let mut netconfig = Netconfig::default();

    if let Ok(dns_policy) = dotenv::var("NETCONFIG_DNS_POLICY") {
        if dns_policy == "auto" {
            netconfig.dns_policy = vec!["STATIC".to_string(), "*".to_string()];
        } else if !dns_policy.is_empty() {
            if dns_policy.contains(&"STATIC_FALLBACK".to_string()) {
                netconfig
                    .warnings
                    .push("NETCONFIG_DNS_POLICY \"STATIC_FALLBACK\" is not supported".to_string());
            } else {
                netconfig.dns_policy = dns_policy.split(' ').map(|s| s.to_string()).collect();
            }
        }
    }
    if let Ok(static_dns_servers) = dotenv::var("NETCONFIG_DNS_STATIC_SERVERS") {
        if !static_dns_servers.is_empty() {
            netconfig.static_dns_servers = static_dns_servers
                .split_whitespace()
                .filter_map(|ip_str| match ip_str.parse::<IpAddr>() {
                    Ok(x) => Some(x),
                    Err(_e) => {
                        netconfig.warnings.push(format!(
                            "Invalid value '{ip_str}' in NETCONFIG_DNS_STATIC_SERVERS"
                        ));
                        None
                    }
                })
                .collect();
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

    if let Ok(gratuitous_arp) = dotenv::var("SEND_GRATUITOUS_ARP") {
        if !gratuitous_arp.eq("auto") {
            netconfig.warnings.push("SEND_GRATUITOUS_ARP differs from 'auto', consider net.ipv4.conf.{{all,default}}.arp_notify variable in /etc/sysctl.conf".to_string());
        }
    }

    Ok(Some(netconfig))
}

pub fn apply_dns_policy(
    netconfig: &Netconfig,
    nm_state: &mut NetworkState,
) -> Result<(), anyhow::Error> {
    // Start at 10 because 0 is special global default in NM
    // and increase by 10 to give room for future changes.
    let mut i: i32 = 10;
    for policy in &netconfig.dns_policy {
        match policy.as_str() {
            "" => continue,
            "STATIC" => {
                let Some(loopback) = nm_state.get_connection_mut("lo") else {
                    anyhow::bail!("Failed to get loopback connection");
                };
                loopback.ip_config.dns_priority4 = Some(i);
                loopback.ip_config.dns_priority6 = Some(i);
            }
            _ => {
                let glob = Glob::new(policy)?.compile_matcher();
                for con in nm_state
                    .connections
                    .iter_mut()
                    .filter(|c| {
                        c.interface
                            .as_ref()
                            .is_some_and(|c_iface| glob.is_match(c_iface))
                            && c.ip_config.dns_priority4.is_none()
                            && c.ip_config.dns_priority6.is_none()
                    })
                    .collect::<Vec<&mut Connection>>()
                {
                    con.ip_config.dns_priority4 = Some(i);
                    con.ip_config.dns_priority6 = Some(i);
                }
            }
        }

        i += 10;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use agama_network::model::Connection;

    use super::*;
    use std::env;

    #[test]
    fn test_handle_netconfig_values() {
        env::set_var("NETCONFIG_DNS_POLICY", "STATIC_FALLBACK NetworkManager");
        assert!(handle_netconfig_values().unwrap().unwrap().warnings.len() == 1);

        env::set_var("NETCONFIG_DNS_POLICY", "STATIC_FALLBACK");
        assert!(handle_netconfig_values().unwrap().unwrap().warnings.len() == 1);

        env::set_var("NETCONFIG_DNS_POLICY", "");
        env::set_var(
            "NETCONFIG_DNS_STATIC_SERVERS",
            "192.168.0.10 192.168.1.10 2001:db8::10",
        );
        env::set_var("NETCONFIG_DNS_STATIC_SEARCHLIST", "suse.com suse.de");
        assert!(handle_netconfig_values()
            .unwrap()
            .unwrap()
            .dns_policy
            .is_empty());

        env::set_var("NETCONFIG_DNS_POLICY", "STATIC");
        assert_eq!(
            handle_netconfig_values().unwrap(),
            Some(Netconfig {
                static_dns_servers: vec![
                    "192.168.0.10".parse().unwrap(),
                    "192.168.1.10".parse().unwrap(),
                    "2001:db8::10".parse().unwrap()
                ],
                static_dns_searchlist: Some(vec!["suse.com".to_string(), "suse.de".to_string()]),
                dns_policy: vec!["STATIC".to_string()],
                ..Default::default()
            })
        );

        env::set_var("NETCONFIG_DNS_POLICY", "");
        env::set_var("NETCONFIG_DNS_STATIC_SERVERS", "");
        env::set_var("NETCONFIG_DNS_STATIC_SEARCHLIST", "");
        assert_eq!(
            handle_netconfig_values().unwrap(),
            Some(Netconfig {
                static_dns_servers: vec![],
                static_dns_searchlist: None,
                ..Default::default()
            })
        );

        env::set_var("NETCONFIG_DNS_POLICY", "auto");
        assert_eq!(
            handle_netconfig_values().unwrap().unwrap().dns_policy,
            vec!["STATIC".to_string(), "*".to_string(),]
        );

        env::set_var("NETCONFIG_DNS_POLICY", "STATIC eth* ppp?");
        assert_eq!(
            handle_netconfig_values().unwrap().unwrap().dns_policy,
            vec!["STATIC".to_string(), "eth*".to_string(), "ppp?".to_string()]
        );
    }

    #[test]
    fn test_apply_dns_policy() {
        let netconfig = Netconfig {
            dns_policy: vec![
                "STATIC".to_string(),
                "e???".to_string(),
                "ppp?".to_string(),
                "eth0.??".to_string(),
                "eth*".to_string(),
                "wlan?".to_string(),
            ],
            ..Default::default()
        };
        let mut nm_state = NetworkState::default();
        // Should match with e???
        assert!(nm_state
            .add_connection(Connection {
                id: "eth0".to_string(),
                interface: Some("eth0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should match with eth0.??
        assert!(nm_state
            .add_connection(Connection {
                id: "eth0.11".to_string(),
                interface: Some("eth0.11".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should match with eth*
        assert!(nm_state
            .add_connection(Connection {
                id: "eth0211".to_string(),
                interface: Some("eth0211".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should not match
        assert!(nm_state
            .add_connection(Connection {
                id: "neth0".to_string(),
                interface: Some("neth0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should match with ppp?
        assert!(nm_state
            .add_connection(Connection {
                id: "ppp0".to_string(),
                interface: Some("ppp0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should not match
        assert!(nm_state
            .add_connection(Connection {
                id: "en0".to_string(),
                interface: Some("en0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should match with wlan?
        assert!(nm_state
            .add_connection(Connection {
                id: "wlan0-0".to_string(),
                interface: Some("wlan0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Should match with wlan?
        assert!(nm_state
            .add_connection(Connection {
                id: "wlan0-1".to_string(),
                interface: Some("wlan0".to_string()),
                ..Default::default()
            })
            .is_ok());
        // Missing loopback
        assert!(apply_dns_policy(&netconfig, &mut nm_state).is_err());

        assert!(nm_state
            .add_connection(Connection {
                id: "lo".to_string(),
                config: agama_network::model::ConnectionConfig::Loopback,
                ..Default::default()
            })
            .is_ok());
        assert!(apply_dns_policy(&netconfig, &mut nm_state).is_ok());
        assert_eq!(
            nm_state
                .get_connection("lo")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(10)
        );
        assert_eq!(
            nm_state
                .get_connection("lo")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(10)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(20)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(20)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0.11")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(40)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0.11")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(40)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0211")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(50)
        );
        assert_eq!(
            nm_state
                .get_connection("eth0211")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(50)
        );
        assert_eq!(
            nm_state
                .get_connection("neth0")
                .unwrap()
                .ip_config
                .dns_priority4,
            None
        );
        assert_eq!(
            nm_state
                .get_connection("neth0")
                .unwrap()
                .ip_config
                .dns_priority6,
            None
        );
        assert_eq!(
            nm_state
                .get_connection("ppp0")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(30)
        );
        assert_eq!(
            nm_state
                .get_connection("ppp0")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(30)
        );
        assert_eq!(
            nm_state
                .get_connection("wlan0-0")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(60)
        );
        assert_eq!(
            nm_state
                .get_connection("wlan0-0")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(60)
        );
        assert_eq!(
            nm_state
                .get_connection("wlan0-1")
                .unwrap()
                .ip_config
                .dns_priority4,
            Some(60)
        );
        assert_eq!(
            nm_state
                .get_connection("wlan0-1")
                .unwrap()
                .ip_config
                .dns_priority6,
            Some(60)
        );
        assert_eq!(
            nm_state
                .get_connection("en0")
                .unwrap()
                .ip_config
                .dns_priority4,
            None
        );
        assert_eq!(
            nm_state
                .get_connection("en0")
                .unwrap()
                .ip_config
                .dns_priority6,
            None
        );
    }
}
