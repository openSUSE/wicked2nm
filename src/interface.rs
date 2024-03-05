use crate::bond::Bond;
use crate::bridge::Bridge;
use crate::infiniband::{Infiniband, InfinibandChild};
use crate::tuntap::Tap;
use crate::tuntap::Tun;
use crate::vlan::Vlan;
use crate::wireless::Wireless;
use crate::MIGRATION_SETTINGS;
use agama_lib::network::types::Status;
use agama_server::network::model::{self, IpConfig, IpRoute, Ipv4Method, Ipv6Method, MacAddress};
use cidr::IpInet;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, skip_serializing_none};
use std::{net::IpAddr, str::FromStr};

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Interface {
    pub name: String,
    pub firewall: Firewall,
    pub link: Link,
    pub ipv4: Ipv4,
    #[serde(rename = "ipv4-static")]
    pub ipv4_static: Option<Ipv4Static>,
    pub ipv6: Ipv6,
    #[serde(rename = "ipv6-static")]
    pub ipv6_static: Option<Ipv6Static>,
    #[serde(rename = "ipv6-dhcp")]
    pub ipv6_dhcp: Option<Ipv6Dhcp>,
    #[serde(rename = "ipv6-auto")]
    pub ipv6_auto: Option<Ipv6Auto>,
    pub dummy: Option<Dummy>,
    pub ethernet: Option<Ethernet>,
    pub bond: Option<Bond>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wireless: Option<Wireless>,
    #[serde(rename = "@origin")]
    pub origin: String,
    pub vlan: Option<Vlan>,
    pub bridge: Option<Bridge>,
    pub infiniband: Option<Infiniband>,
    #[serde(rename = "infiniband-child")]
    pub infiniband_child: Option<InfinibandChild>,
    pub tun: Option<Tun>,
    pub tap: Option<Tap>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Firewall {
    pub zone: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Link {
    pub master: Option<String>,
    pub mtu: Option<u32>,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ipv4 {
    pub enabled: bool,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ipv6 {
    pub enabled: bool,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Ipv4Static {
    #[serde(rename = "address")]
    pub addresses: Option<Vec<Address>>,
    #[serde(rename = "route")]
    pub routes: Option<Vec<Route>>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Ipv6Static {
    #[serde(rename = "address")]
    pub addresses: Option<Vec<Address>>,
    #[serde(rename = "route")]
    pub routes: Option<Vec<Route>>,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Address {
    pub local: String,
}

#[serde_as]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ipv6Dhcp {
    pub enabled: bool,
    pub mode: String,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ipv6Auto {
    pub enabled: bool,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Dummy {
    pub address: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Route {
    pub destination: Option<String>,
    #[serde(rename = "nexthop")]
    pub nexthops: Option<Vec<Nexthop>>,
    pub priority: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Ethernet {
    pub address: Option<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Nexthop {
    pub gateway: String,
}

pub struct ConnectionResult {
    pub connections: Vec<model::Connection>,
    pub warnings: Vec<anyhow::Error>,
}

pub struct IpConfigResult {
    ip_config: IpConfig,
    warnings: Vec<anyhow::Error>,
}

impl Interface {
    pub fn to_connection(&self) -> Result<ConnectionResult, anyhow::Error> {
        let settings = MIGRATION_SETTINGS.get().unwrap();
        let ip_config = self.to_ip_config()?;
        let mut warnings = ip_config.warnings;
        let mut connection = model::Connection {
            id: self.name.clone(),
            firewall_zone: self.firewall.zone.clone(),
            interface: Some(self.name.clone()),
            ip_config: ip_config.ip_config,
            status: Status::Down,
            mtu: self.link.mtu.unwrap_or_default(),
            ..Default::default()
        };
        let mut connections: Vec<model::Connection> = vec![];

        if settings.activate_connections {
            connection.status = Status::Up;
        }

        if let Some(ethernet) = &self.ethernet {
            connection.mac_address = MacAddress::try_from(&ethernet.address)?;
            connection.config = model::ConnectionConfig::Ethernet;
            connections.push(connection);
        } else if let Some(dummy) = &self.dummy {
            connection.mac_address = MacAddress::try_from(&dummy.address)?;
            connection.config = model::ConnectionConfig::Dummy;
            connections.push(connection);
        } else if let Some(bond) = &self.bond {
            connection.mac_address = MacAddress::try_from(&bond.address)?;
            connection.config = bond.into();
            connections.push(connection);
        } else if let Some(vlan) = &self.vlan {
            connection.mac_address = MacAddress::try_from(&vlan.address)?;
            connection.config = vlan.into();
            connections.push(connection);
        } else if let Some(bridge) = &self.bridge {
            connection.mac_address = MacAddress::try_from(&bridge.address)?;
            connection.config = bridge.into();
            connections.push(connection);
        } else if let Some(wireless) = &self.wireless {
            if let Some(networks) = &wireless.networks {
                if networks.len() > 1 {
                    log::info!("{} has multiple networks defined, these will be split into different connections in NM", connection.id);
                }
                for (i, network) in networks.iter().enumerate() {
                    let mut wireless_connection = connection.clone();
                    if networks.len() > 1 {
                        wireless_connection.id.push_str(&format!("-{}", i));
                    }
                    wireless_connection.config = network.try_into()?;
                    connections.push(wireless_connection);
                }
            }
        } else if let Some(infiniband) = &self.infiniband {
            if infiniband.multicast.is_some() {
                warnings.push(anyhow::anyhow!(
                    "Infiniband multicast isn't supported by NetworkManager"
                ));
            }
            connection.config = infiniband.into();
            connections.push(connection)
        } else if let Some(infiniband_child) = &self.infiniband_child {
            if infiniband_child.multicast.is_some() {
                warnings.push(anyhow::anyhow!(
                    "Infiniband multicast isn't supported by NetworkManager"
                ));
            }
            connection.config = infiniband_child.into();
            connections.push(connection)
        } else if let Some(tun) = &self.tun {
            connection.config = tun.into();
            connections.push(connection)
        } else if let Some(tap) = &self.tap {
            connection.config = tap.into();
            connections.push(connection)
        } else {
            connections.push(connection);
        }

        Ok(ConnectionResult {
            connections,
            warnings,
        })
    }

    pub fn to_ip_config(&self) -> Result<IpConfigResult, anyhow::Error> {
        let mut connection_result = IpConfigResult {
            ip_config: IpConfig {
                ..Default::default()
            },
            warnings: vec![],
        };
        let method4 = if self.ipv4.enabled && self.ipv4_static.is_some() {
            Ipv4Method::Manual
        } else if !self.ipv4.enabled {
            Ipv4Method::Disabled
        } else {
            Ipv4Method::Auto
        };
        let method6 = if self.ipv6.enabled && self.ipv6_static.is_some() {
            Ipv6Method::Manual
        } else if self.ipv6.enabled
            && self.ipv6_dhcp.is_some()
            && self.ipv6_dhcp.as_ref().unwrap().mode == "managed"
        {
            Ipv6Method::Dhcp
        } else if !self.ipv6.enabled {
            Ipv6Method::Disabled
        } else {
            Ipv6Method::Auto
        };

        let mut addresses: Vec<IpInet> = vec![];
        let mut new_routes4: Vec<IpRoute> = vec![];
        let mut new_routes6: Vec<IpRoute> = vec![];
        if let Some(ipv4_static) = &self.ipv4_static {
            if let Some(addresses_in) = &ipv4_static.addresses {
                for addr in addresses_in {
                    addresses.push(IpInet::from_str(addr.local.as_str()).unwrap());
                }
            }
            if let Some(routes) = &ipv4_static.routes {
                for route in routes {
                    new_routes4.push(match route.try_into() {
                        Ok(route) => route,
                        Err(e) => {
                            connection_result.warnings.push(e);
                            continue;
                        }
                    });
                }
            }
        }
        if let Some(ipv6_static) = &self.ipv6_static {
            if let Some(addresses_in) = &ipv6_static.addresses {
                for addr in addresses_in {
                    addresses.push(IpInet::from_str(addr.local.as_str()).unwrap());
                }
            }
            if let Some(routes) = &ipv6_static.routes {
                for route in routes {
                    new_routes6.push(match route.try_into() {
                        Ok(route) => route,
                        Err(e) => {
                            connection_result.warnings.push(e);
                            continue;
                        }
                    });
                }
            }
        }

        let routes4 = if !new_routes4.is_empty() {
            Some(new_routes4)
        } else {
            None
        };
        let routes6 = if !new_routes6.is_empty() {
            Some(new_routes6)
        } else {
            None
        };

        connection_result.ip_config = IpConfig {
            addresses,
            method4,
            method6,
            routes4,
            routes6,
            ..Default::default()
        };
        Ok(connection_result)
    }
}

impl TryFrom<&Route> for IpRoute {
    type Error = anyhow::Error;
    fn try_from(route: &Route) -> Result<Self, Self::Error> {
        let mut next_hop: Option<IpAddr> = None;
        if let Some(nexthops) = &route.nexthops {
            if nexthops.len() > 1 {
                return Err(anyhow::anyhow!(
                    "Multipath routing isn't natively supported by NetworkManager"
                ));
            } else {
                next_hop = Some(IpAddr::from_str(&nexthops[0].gateway).unwrap());
            }
        }
        let destination = if route.destination.is_some() {
            IpInet::from_str(route.destination.clone().unwrap().as_str())?
        } else if next_hop.is_some() {
            // default route
            let default_ip = if next_hop.unwrap().is_ipv4() {
                IpAddr::from_str("0.0.0.0")?
            } else {
                IpAddr::from_str("::")?
            };
            IpInet::new(default_ip, 0)?
        } else {
            return Err(anyhow::anyhow!("Error occurred when parsing a route"));
        };
        let metric = route.priority;
        Ok(IpRoute {
            destination,
            next_hop,
            metric,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[allow(dead_code)]
    fn setup_default_migration_settings() {
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());
    }

    #[test]
    fn test_static_interface_to_connection() {
        setup_default_migration_settings();
        let static_interface = Interface {
            ipv4: Ipv4 { enabled: true },
            ipv4_static: Some(Ipv4Static {
                addresses: Some(vec![Address {
                    local: "127.0.0.1/8".to_string(),
                }]),
                routes: Some(vec![Route {
                    nexthops: Some(vec![Nexthop {
                        gateway: "127.0.0.1".to_string(),
                    }]),
                    ..Default::default()
                }]),
            }),
            ipv6: Ipv6 { enabled: true },
            ipv6_static: Some(Ipv6Static {
                addresses: Some(vec![Address {
                    local: "::1/128".to_string(),
                }]),
                routes: Some(vec![Route {
                    nexthops: Some(vec![Nexthop {
                        gateway: "::1".to_string(),
                    }]),
                    ..Default::default()
                }]),
            }),
            ..Default::default()
        };

        let static_connection: model::Connection =
            static_interface.to_connection().unwrap().connections[0].to_owned();
        assert_eq!(static_connection.ip_config.method4, Ipv4Method::Manual);
        assert_eq!(
            static_connection.ip_config.addresses[0].to_string(),
            "127.0.0.1/8"
        );
        assert_eq!(static_connection.ip_config.method6, Ipv6Method::Manual);
        assert_eq!(static_connection.ip_config.addresses[1].to_string(), "::1");
        assert_eq!(
            static_connection.ip_config.addresses[1]
                .network_length()
                .to_string(),
            "128"
        );
        assert!(static_connection.ip_config.routes4.is_some());
        assert!(static_connection.ip_config.routes4.clone().unwrap().len() == 1);
        assert_eq!(
            static_connection.ip_config.routes4.clone().unwrap()[0]
                .destination
                .to_string(),
            "0.0.0.0/0"
        );
        assert_eq!(
            static_connection.ip_config.routes4.clone().unwrap()[0]
                .next_hop
                .unwrap()
                .to_string(),
            "127.0.0.1"
        );
        assert!(static_connection.ip_config.routes6.is_some());
        assert!(static_connection.ip_config.routes6.clone().unwrap().len() == 1);
        assert_eq!(
            static_connection.ip_config.routes6.clone().unwrap()[0]
                .destination
                .to_string(),
            "::/0"
        );
        assert_eq!(
            static_connection.ip_config.routes6.clone().unwrap()[0]
                .next_hop
                .unwrap()
                .to_string(),
            "::1"
        );
    }

    #[test]
    fn test_dhcp_interface_to_connection() {
        setup_default_migration_settings();
        let static_interface = Interface {
            ipv4: Ipv4 { enabled: true },
            ipv6: Ipv6 { enabled: true },
            ..Default::default()
        };

        let static_connection: model::Connection =
            static_interface.to_connection().unwrap().connections[0].to_owned();
        assert_eq!(static_connection.ip_config.method4, Ipv4Method::Auto);
        assert_eq!(static_connection.ip_config.method6, Ipv6Method::Auto);
        assert_eq!(static_connection.ip_config.addresses.len(), 0);
    }

    #[test]
    fn test_dummy_interface_to_connection() {
        setup_default_migration_settings();
        let dummy_interface = Interface {
            dummy: Some(Dummy {
                address: Some("12:34:56:78:9A:BC".to_string()),
            }),
            ..Default::default()
        };

        let connection: &model::Connection =
            &dummy_interface.to_connection().unwrap().connections[0];
        assert!(matches!(connection.config, model::ConnectionConfig::Dummy));
        assert_eq!(connection.mac_address.to_string(), "12:34:56:78:9A:BC");

        let dummy_interface = Interface {
            dummy: Some(Dummy {
                ..Default::default()
            }),
            ..Default::default()
        };

        let connection: &model::Connection =
            &dummy_interface.to_connection().unwrap().connections[0];
        assert!(matches!(connection.config, model::ConnectionConfig::Dummy));
        assert_eq!(dummy_interface.dummy.unwrap().address, None);
        assert!(matches!(connection.mac_address, MacAddress::Unset));
    }

    #[test]
    fn test_firewall_zone_to_connection() {
        setup_default_migration_settings();
        let ifc = Interface {
            firewall: Firewall {
                zone: Some("topsecret".to_string()),
            },
            ..Default::default()
        };

        let con: model::Connection = ifc.to_connection().unwrap().connections[0].to_owned();
        assert_eq!(con.firewall_zone, Some("topsecret".to_string()));
    }
}
