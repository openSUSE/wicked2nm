use crate::MIGRATION_SETTINGS;
use agama_dbus_server::network::model::{
    self, IpConfig, IpRoute, Ipv4Method, Ipv6Method, MacAddress,
};
use agama_lib::network::types::BondMode as AgamaBondMode;
use cidr::IpInet;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{serde_as, skip_serializing_none, DeserializeFromStr, SerializeDisplay};
use std::{collections::HashMap, net::IpAddr, str::FromStr};
use strum_macros::{Display, EnumString};

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Interface {
    pub name: String,
    pub link: Link,
    pub ipv4: Ipv4,
    #[serde(rename = "ipv4-static", skip_serializing_if = "Option::is_none")]
    pub ipv4_static: Option<Ipv4Static>,
    pub ipv6: Ipv6,
    #[serde(rename = "ipv6-static", skip_serializing_if = "Option::is_none")]
    pub ipv6_static: Option<Ipv6Static>,
    #[serde(rename = "ipv6-dhcp", skip_serializing_if = "Option::is_none")]
    pub ipv6_dhcp: Option<Ipv6Dhcp>,
    #[serde(rename = "ipv6-auto", skip_serializing_if = "Option::is_none")]
    pub ipv6_auto: Option<Ipv6Auto>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dummy: Option<Dummy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ethernet: Option<Ethernet>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bond: Option<Bond>,
    #[serde(rename = "@origin")]
    pub origin: String,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
#[skip_serializing_none]
pub struct Link {
    pub master: Option<String>,
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Route {
    pub destination: Option<String>,
    #[serde(rename = "nexthop")]
    pub nexthops: Option<Vec<Nexthop>>,
    pub priority: Option<u32>,
}

#[derive(Debug, PartialEq, Serialize, Clone, Deserialize)]
pub enum ParentKind {
    Bond,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum FailOverMac {
    None,
    Active,
    Follow,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum XmitHashPolicy {
    Layer2,
    Layer23,
    Layer34,
    Encap23,
    Encap34,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum LacpRate {
    Slow,
    Fast,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum AdSelect {
    Stable,
    Bandwidth,
    Count,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum PrimaryReselect {
    Always,
    Better,
    Failure,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum WickedBondMode {
    BalanceRr = 0,
    ActiveBackup,
    BalanceXor,
    Broadcast,
    #[strum(serialize = "802.3ad")]
    IEEE8023ad,
    BalanceTlb,
    BalanceAlb,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[skip_serializing_none]
#[serde(rename_all = "kebab-case")]
pub struct Bond {
    pub mode: WickedBondMode,
    pub miimon: Option<Miimon>,
    pub arpmon: Option<ArpMon>,
    #[serde(deserialize_with = "unwrap_slaves")]
    pub slaves: Vec<Slave>,
    /* only on mode=[802.3ad, balance_xor] */
    pub xmit_hash_policy: Option<XmitHashPolicy>,
    /* only on mode=balance_rr */
    pub packets_per_slave: Option<u32>,
    /* only on mode=balance_tlb */
    pub tlb_dynamic_lb: Option<bool>,
    /* only on mode=802.3ad */
    pub lacp_rate: Option<LacpRate>,
    /* only on mode=802.3ad */
    pub ad_select: Option<AdSelect>,
    /* only on mode=802.3ad */
    pub ad_user_port_key: Option<u32>,
    /* only on mode=802.3ad */
    pub ad_actor_sys_prio: Option<u32>,
    /* only on mode=802.3ad */
    pub ad_actor_system: Option<String>,
    /* only on mode=802.3ad */
    pub min_links: Option<u32>,
    /* only on mode=active-backup */
    pub primary_reselect: Option<PrimaryReselect>,
    /* only on mode=active-backup */
    pub fail_over_mac: Option<FailOverMac>,
    /* only on mode=active-backup */
    pub num_grat_arp: Option<u32>,
    /* only on mode=active-backup */
    pub num_unsol_na: Option<u32>,
    /* only on mode=[balance_tlb|balance_alb] */
    pub lp_interval: Option<u32>,
    /* only on mode=[balance_tlb|balance_alb|balance_RR|active-backup] */
    pub resend_igmp: Option<u32>,
    pub all_slaves_active: Option<bool>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
}

impl Bond {
    pub fn primary(self: &Bond) -> Option<&String> {
        for s in self.slaves.iter() {
            if s.primary.unwrap_or(false) {
                return Some(&s.device);
            }
        }
        None
    }
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Slave {
    pub device: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub primary: Option<bool>,
}

#[derive(Debug, PartialEq, Default, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum CarrierDetect {
    Ioctl = 0,
    #[default]
    Netif = 1,
}

#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Miimon {
    pub frequency: u32,
    #[serde(rename = "carrier-detect")]
    pub carrier_detect: CarrierDetect,
    pub downdelay: Option<u32>,
    pub updelay: Option<u32>,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum ArpValidateTargets {
    Any = 0,
    All = 1,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "snake_case")]
pub enum ArpValidate {
    None = 0,
    Active = 1,
    Backup = 2,
    All = 3,
    Filter = 4,
    FilterActive = 5,
    FilterBackup = 6,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct ArpMon {
    pub interval: u32,
    pub validate: ArpValidate,
    #[serde(rename = "validate-targets", skip_serializing_if = "Option::is_none")]
    pub validate_targets: Option<ArpValidateTargets>,
    #[serde(deserialize_with = "unwrap_arpmon_targets")]
    pub targets: Vec<String>,
}

fn unwrap_arpmon_targets<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
    pub struct ArpMonTargetAddressV4 {
        #[serde(rename = "ipv4-address")]
        pub ipv4_address: Vec<String>,
    }
    Ok(ArpMonTargetAddressV4::deserialize(deserializer)?.ipv4_address)
}

fn unwrap_slaves<'de, D>(deserializer: D) -> Result<Vec<Slave>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
    struct Slaves {
        slave: Vec<Slave>,
    }
    Ok(Slaves::deserialize(deserializer)?.slave)
}

impl From<&WickedBondMode> for AgamaBondMode {
    fn from(bondmode: &WickedBondMode) -> AgamaBondMode {
        match bondmode {
            WickedBondMode::BalanceRr => AgamaBondMode::RoundRobin,
            WickedBondMode::ActiveBackup => AgamaBondMode::ActiveBackup,
            WickedBondMode::BalanceXor => AgamaBondMode::BalanceXOR,
            WickedBondMode::Broadcast => AgamaBondMode::Broadcast,
            WickedBondMode::IEEE8023ad => AgamaBondMode::LACP,
            WickedBondMode::BalanceTlb => AgamaBondMode::BalanceTLB,
            WickedBondMode::BalanceAlb => AgamaBondMode::BalanceALB,
        }
    }
}

impl From<&Bond> for model::ConnectionConfig {
    fn from(bond: &Bond) -> model::ConnectionConfig {
        let mut h: HashMap<String, String> = HashMap::new();

        if let Some(p) = bond.primary() {
            h.insert(String::from("primary"), p.clone());
        }

        if let Some(m) = &bond.miimon {
            h.insert(String::from("miimon"), format!("{}", m.frequency));
            h.insert(
                String::from("use_carrier"),
                match m.carrier_detect {
                    CarrierDetect::Ioctl => 0,
                    CarrierDetect::Netif => 1,
                }
                .to_string(),
            );
            if let Some(v) = m.downdelay {
                h.insert(String::from("downdelay"), v.to_string());
            }
            if let Some(v) = m.updelay {
                h.insert(String::from("updelay"), v.to_string());
            }
        }

        if let Some(a) = &bond.arpmon {
            h.insert(String::from("arp_interval"), format!("{}", a.interval));
            h.insert(String::from("arp_validate"), a.validate.to_string());

            if !a.targets.is_empty() {
                let sv = a
                    .targets
                    .iter()
                    .map(|c| c.as_ref())
                    .collect::<Vec<&str>>()
                    .join(",");
                h.insert(String::from("arp_ip_target"), sv);
            }

            if let Some(v) = &a.validate_targets {
                h.insert(String::from("arp_all_targets"), v.to_string());
            }
        }

        if let Some(fom) = &bond.fail_over_mac {
            h.insert(String::from("fail_over_mac"), fom.to_string());
        }

        if let Some(v) = &bond.xmit_hash_policy {
            h.insert(String::from("xmit_hash_policy"), v.to_string());
        }

        if let Some(v) = &bond.packets_per_slave {
            h.insert(String::from("packets_per_slave"), v.to_string());
        }

        if let Some(v) = &bond.tlb_dynamic_lb {
            h.insert(
                String::from("tlb_dynamic_lb"),
                if *v { 1.to_string() } else { 0.to_string() },
            );
        }

        if let Some(v) = &bond.lacp_rate {
            h.insert(String::from("lacp_rate"), v.to_string());
        }

        if let Some(v) = &bond.ad_select {
            h.insert(String::from("ad_select"), v.to_string());
        }
        if let Some(v) = &bond.ad_user_port_key {
            h.insert(String::from("ad_user_port_key"), v.to_string());
        }
        if let Some(v) = &bond.ad_actor_sys_prio {
            h.insert(String::from("ad_actor_sys_prio"), v.to_string());
        }
        if let Some(v) = &bond.ad_actor_system {
            h.insert(String::from("ad_actor_system"), v.clone());
        }
        if let Some(v) = &bond.min_links {
            h.insert(String::from("min_links"), v.to_string());
        }
        if let Some(v) = &bond.primary_reselect {
            h.insert(String::from("primary_reselect"), v.to_string());
        }
        if let Some(v) = &bond.num_grat_arp {
            h.insert(String::from("num_grat_arp"), v.to_string());
        }
        if let Some(v) = &bond.num_unsol_na {
            h.insert(String::from("num_unsol_na"), v.to_string());
        }
        if let Some(v) = &bond.lp_interval {
            h.insert(String::from("lp_interval"), v.to_string());
        }
        if let Some(v) = &bond.resend_igmp {
            h.insert(String::from("resend_igmp"), v.to_string());
        }
        if let Some(v) = &bond.all_slaves_active {
            h.insert(
                String::from("all_slaves_active"),
                if *v { 1.to_string() } else { 0.to_string() },
            );
        }

        model::ConnectionConfig::Bond(model::BondConfig {
            options: model::BondOptions(h),
            mode: AgamaBondMode::from(&bond.mode),
        })
    }
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
pub struct Ethernet {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub address: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Nexthop {
    pub gateway: String,
}

pub struct ConnectionResult {
    pub connection: model::Connection,
    pub warnings: Vec<anyhow::Error>,
}

pub struct IpConfigResult {
    ip_config: IpConfig,
    warnings: Vec<anyhow::Error>,
}

impl Interface {
    pub fn to_connection(&self) -> Result<ConnectionResult, anyhow::Error> {
        let ip_config = self.to_ip_config()?;
        let mut connection = model::Connection {
            id: self.name.clone(),
            interface: Some(self.name.clone()),
            ip_config: ip_config.ip_config,
            status: model::Status::Down,
            ..Default::default()
        };

        if let Some(settings) = MIGRATION_SETTINGS.get() {
            if settings.activate_connections {
                connection.status = model::Status::Up;
            }
        }

        if let Some(ethernet) = &self.ethernet {
            connection.mac_address = MacAddress::from_str(&ethernet.address)?;
            connection.config = model::ConnectionConfig::Ethernet
        } else if let Some(dummy) = &self.dummy {
            connection.mac_address = MacAddress::from_str(&dummy.address)?;
            connection.config = model::ConnectionConfig::Dummy
        } else if let Some(bond) = &self.bond {
            connection.mac_address = MacAddress::from_str(&bond.address)?;
            connection.config = bond.into()
        }

        Ok(ConnectionResult {
            connection,
            warnings: ip_config.warnings,
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

    #[test]
    fn test_static_interface_to_connection() {
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
            static_interface.to_connection().unwrap().connection;
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
        let static_interface = Interface {
            ipv4: Ipv4 { enabled: true },
            ipv6: Ipv6 { enabled: true },
            ..Default::default()
        };

        let static_connection: model::Connection =
            static_interface.to_connection().unwrap().connection;
        assert_eq!(static_connection.ip_config.method4, Ipv4Method::Auto);
        assert_eq!(static_connection.ip_config.method6, Ipv6Method::Auto);
        assert_eq!(static_connection.ip_config.addresses.len(), 0);
    }

    #[test]
    fn test_dummy_interface_to_connection() {
        let dummy_interface = Interface {
            dummy: Some(Dummy {
                address: "12:34:56:78:9A:BC".to_string(),
            }),
            ..Default::default()
        };

        let connection: model::Connection = dummy_interface.to_connection().unwrap().connection;
        assert!(matches!(connection.config, model::ConnectionConfig::Dummy));
        assert_eq!(connection.mac_address.to_string(), "12:34:56:78:9A:BC");

        let dummy_interface = Interface {
            dummy: Some(Dummy {
                ..Default::default()
            }),
            ..Default::default()
        };

        let connection: model::Connection = dummy_interface.to_connection().unwrap().connection;
        assert!(matches!(connection.config, model::ConnectionConfig::Dummy));
        assert_eq!(dummy_interface.dummy.unwrap().address, String::from(""));
        assert!(matches!(connection.mac_address, MacAddress::Unset));
    }

    #[test]
    fn test_bond_options() {
        let bond_interface = Interface {
            bond: Some(Bond {
                mode: WickedBondMode::IEEE8023ad,
                xmit_hash_policy: Some(XmitHashPolicy::Encap34),
                fail_over_mac: Some(FailOverMac::Active),
                packets_per_slave: Some(23),
                tlb_dynamic_lb: Some(true),
                lacp_rate: Some(LacpRate::Slow),
                ad_select: Some(AdSelect::Bandwidth),
                ad_user_port_key: Some(42),
                ad_actor_sys_prio: Some(5),
                ad_actor_system: Some(String::from("00:de:ad:be:ef:00")),
                min_links: Some(3),
                primary_reselect: Some(PrimaryReselect::Better),
                num_grat_arp: Some(7),
                num_unsol_na: Some(13),
                lp_interval: Some(17),
                resend_igmp: Some(19),
                all_slaves_active: Some(true),
                miimon: Some(Miimon {
                    frequency: 42,
                    carrier_detect: CarrierDetect::Netif,
                    downdelay: Some(23),
                    updelay: Some(5),
                }),
                arpmon: Some(ArpMon {
                    interval: 23,
                    validate: ArpValidate::FilterBackup,
                    validate_targets: Some(ArpValidateTargets::Any),
                    targets: vec![String::from("1.2.3.4"), String::from("4.3.2.1")],
                }),
                slaves: vec![],
                address: String::from("02:11:22:33:44:55"),
            }),
            ..Default::default()
        };

        let connection: model::Connection = bond_interface.to_connection().unwrap().connection;
        assert!(matches!(
            connection.config,
            model::ConnectionConfig::Bond(_)
        ));
        assert_eq!(connection.mac_address.to_string(), "02:11:22:33:44:55");

        if let model::ConnectionConfig::Bond(bond) = connection.config {
            assert_eq!(bond.mode, AgamaBondMode::LACP);
            let s = HashMap::from([
                ("xmit_hash_policy", String::from("encap34")),
                ("packets_per_slave", 23.to_string()),
                ("tlb_dynamic_lb", 1.to_string()),
                ("lacp_rate", String::from("slow")),
                ("ad_select", String::from("bandwidth")),
                ("ad_user_port_key", 42.to_string()),
                ("ad_actor_sys_prio", 5.to_string()),
                ("ad_actor_system", String::from("00:de:ad:be:ef:00")),
                ("min_links", 3.to_string()),
                ("primary_reselect", String::from("better")),
                ("fail_over_mac", String::from("active")),
                ("num_grat_arp", 7.to_string()),
                ("num_unsol_na", 13.to_string()),
                ("lp_interval", 17.to_string()),
                ("resend_igmp", 19.to_string()),
                ("all_slaves_active", 1.to_string()),
                // miimon
                ("miimon", 42.to_string()),
                ("use_carrier", 1.to_string()),
                ("downdelay", 23.to_string()),
                ("updelay", 5.to_string()),
                // arpmon
                ("arp_validate", String::from("filter_backup")),
                ("arp_all_targets", String::from("any")),
                ("arp_ip_target", String::from("1.2.3.4,4.3.2.1")),
                ("arp_interval", 23.to_string()),
            ]);

            for (k, v) in s.iter() {
                assert!(
                    bond.options.0.contains_key(*k),
                    "Missing key '{}' in bond hash {:?}",
                    *k,
                    bond.options.0
                );
                assert_eq!(
                    bond.options.0.get(*k).unwrap(),
                    v,
                    "Unexpected value '{}' in key '{}'",
                    *k,
                    v
                );
            }
        }
    }
}
