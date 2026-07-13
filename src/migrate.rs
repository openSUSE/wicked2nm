use crate::interface::{ConnectionResult, Link, LinkPort, LinkPortType};
use crate::netconfig::{apply_dns_policy, Netconfig};
use crate::reader::InterfacesResult;
use crate::MIGRATION_SETTINGS;
use agama_network::model::{Connection, ConnectionConfig, MatchConfig, StateConfig};
use agama_network::types::{IpConfig, Ipv4Method, Ipv6Method};
use agama_network::{model, Adapter, NetworkManagerAdapter, NetworkState};
use cidr::IpInet;
use nix::ifaddrs::getifaddrs;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

#[derive(Debug)]
struct ParentMatch {
    uuid: Uuid,
    tag: Option<u16>,
}

impl From<Uuid> for ParentMatch {
    fn from(value: Uuid) -> Self {
        ParentMatch {
            uuid: value,
            tag: None,
        }
    }
}

impl fmt::Display for ParentMatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag {
            Some(tag_value) => write!(f, "{}(tag: {tag_value})", self.uuid),
            None => write!(f, "{}", self.uuid),
        }
    }
}

fn get_parentmatch_ovsbridge(
    parent_connection: &Connection,
    connections: &[Connection],
) -> Option<ParentMatch> {
    let bridge_port = connections
        .iter()
        .find(|c| Some(c.uuid) == parent_connection.controller)?;

    let ConnectionConfig::OvsPort(config) = &bridge_port.config else {
        return None;
    };

    let bridge = connections
        .iter()
        .find(|c| Some(c.uuid) == bridge_port.controller)?;

    Some(ParentMatch {
        uuid: bridge.uuid,
        tag: config.tag,
    })
}

fn update_parent_connection(
    cresult: &mut ConnectionResult,
    parents: &mut HashMap<Uuid, Link>,
) -> Result<(), anyhow::Error> {
    let mut parent_uuids: HashMap<Uuid, ParentMatch> = HashMap::new();

    for (port_uuid, parent) in parents.iter() {
        let Some(parent_con) = cresult
            .connections
            .iter()
            .find(|c| c.interface == parent.master)
        else {
            log::warn!(
                "Missing parent connection with interface {} for port {port_uuid}",
                parent.clone().master.unwrap()
            );
            cresult.has_warnings = true;
            continue;
        };

        let Some(port) = &parent.port else {
            continue;
        };

        if let Some(parent_match) = match port.port_type {
            LinkPortType::OvsBridge => get_parentmatch_ovsbridge(parent_con, &cresult.connections),
            _ => Some(ParentMatch::from(parent_con.uuid)),
        } {
            parent_uuids.insert(*port_uuid, parent_match);
        }
    }

    for (port_uuid, parent_match) in parent_uuids {
        let Some(connection) = cresult.connections.iter_mut().find(|c| c.uuid == port_uuid) else {
            anyhow::bail!(
                "Unexpected failure - missing port connection {port_uuid} from parent {parent_match}"
            );
        };

        connection.controller = Some(parent_match.uuid);

        if let Some(vlan_tag) = parent_match.tag {
            if let ConnectionConfig::OvsPort(config) = &mut connection.config {
                config.tag = Some(vlan_tag);
            }
        }

        parents.remove(&port_uuid);
    }

    Ok(())
}

struct TeamPortOptions {
    name: String,
    prio: Option<u32>,
    sticky: bool,
}

fn apply_team_port_options_to_bond(
    connections: &mut [Connection],
    original_parents: &HashMap<Uuid, Link>,
) -> bool {
    let mut has_warnings = false;

    // Build a map of bond UUID -> list of team port options
    let mut bond_ports: HashMap<Uuid, Vec<TeamPortOptions>> = HashMap::new();

    for connection in connections.iter() {
        let Some(controller_uuid) = connection.controller else {
            continue;
        };
        // Check if this port has team port options
        let Some(link) = original_parents.get(&connection.uuid) else {
            continue;
        };
        let Some(port) = &link.port else {
            continue;
        };

        if port.port_type == LinkPortType::Team {
            let port_name = connection
                .interface
                .as_ref()
                .unwrap_or(&connection.id)
                .clone();
            bond_ports
                .entry(controller_uuid)
                .or_default()
                .push(TeamPortOptions {
                    name: port_name,
                    prio: port.prio,
                    sticky: port.sticky,
                });
        }
    }

    // Now update bond options based on collected port info
    for connection in connections.iter_mut() {
        let ConnectionConfig::Bond(bond_config) = &mut connection.config else {
            continue;
        };

        let Some(ports) = bond_ports.get(&connection.uuid) else {
            continue;
        };

        let ports_with_prio: Vec<&TeamPortOptions> =
            ports.iter().filter(|p| p.prio.is_some()).collect();
        let sticky_ports: Vec<&TeamPortOptions> = ports.iter().filter(|p| p.sticky).collect();

        // If no port has prio there is nothing to do but
        // warn about sticky ports
        if ports_with_prio.is_empty() {
            for sticky_port in sticky_ports {
                log::warn!(
                    "Team port '{}' is marked as sticky. Bond requires a primary port (with prio set) to use sticky behavior.",
                    sticky_port.name
                );
                has_warnings = true;
            }
            continue;
        }

        let max_prio = ports_with_prio
            .iter()
            .map(|p| p.prio.unwrap())
            .max()
            .unwrap();
        let ports_with_max_prio: Vec<&TeamPortOptions> = ports_with_prio
            .iter()
            .filter(|p| p.prio.unwrap() == max_prio)
            .copied()
            .collect();

        // Multiple ports with same highest priority - ambiguous
        if ports_with_max_prio.len() > 1 {
            let names: Vec<&str> = ports_with_max_prio
                .iter()
                .map(|p| p.name.as_str())
                .collect();
            log::warn!(
                "Team has multiple ports {:?} with the same highest prio={}. Bond requires a single primary port, not setting primary.",
                names,
                max_prio
            );
            has_warnings = true;
            continue;
        }

        let port = ports_with_max_prio[0];
        bond_config
            .options
            .0
            .insert(String::from("primary"), port.name.clone());

        if port.sticky {
            bond_config
                .options
                .0
                .insert(String::from("primary_reselect"), String::from("failure"));
        }

        // Check if mapping is perfect (only 2 different priority values)
        let unique_prios: std::collections::HashSet<u32> =
            ports_with_prio.iter().map(|p| p.prio.unwrap()).collect();

        if unique_prios.len() > 2 {
            log::warn!(
                "Team has {} different priority levels, but bond only supports primary vs backup (2 levels). Port '{}' with prio={} set as bond primary.",
                unique_prios.len(),
                port.name,
                port.prio.unwrap()
            );
            has_warnings = true;
        } else {
            log::info!(
                "Team port '{}' with highest prio={} mapped to bond primary",
                port.name,
                port.prio.unwrap()
            );
        }

        // Warn if other ports are sticky (bond doesn't support per-port sticky)
        for sticky_port in &sticky_ports {
            if sticky_port.name != port.name {
                log::warn!("Team port '{}' is marked as sticky. Bonding only allows the primary port to be sticky.", sticky_port.name);
                has_warnings = true;
            }
        }
    }

    has_warnings
}

fn create_lo_connection() -> Connection {
    Connection {
        id: "lo".to_string(),
        ip_config: IpConfig {
            method4: Some(Ipv4Method::Manual),
            method6: Some(Ipv6Method::Manual),
            addresses: vec![
                IpInet::from_str("127.0.0.1/8").unwrap(),
                IpInet::from_str("::1/128").unwrap(),
            ],
            ..Default::default()
        },
        interface: Some("lo".to_string()),
        match_config: MatchConfig::default(),
        config: model::ConnectionConfig::Loopback,
        ..Default::default()
    }
}

#[derive(Default)]
pub struct NetworkStateResult {
    pub network_state: NetworkState,
    pub has_warnings: bool,
}

pub fn to_networkstate(
    interface_result: &InterfacesResult,
) -> Result<NetworkStateResult, anyhow::Error> {
    let settings = MIGRATION_SETTINGS.get().unwrap();
    let mut parents: HashMap<Uuid, Link> = HashMap::new();
    let mut connection_result: ConnectionResult = ConnectionResult {
        has_warnings: interface_result.has_warnings,
        ..Default::default()
    };

    for interface in &interface_result.interfaces {
        let ifc_connection_result = interface.to_connection(&interface_result.netconfig_dhcp)?;
        connection_result.has_warnings |= ifc_connection_result.has_warnings;

        for connection in ifc_connection_result.connections {
            if connection.controller.is_none() {
                if interface.link.master.is_some() {
                    parents.insert(connection.uuid, interface.link.clone());
                } else if let Some(ovs_bridge) = &interface.ovs_bridge {
                    //  This "if let" handles the special port handling of ovs-bridge
                    //  which is NOT defined via the `<link>` field but inside the
                    //  `<ovs-bridge>` tag like (aka "fake bridge", see man 5 ifcfg-ovs-bridge):
                    //
                    //   <ovs-bridge>
                    //    <vlan>
                    //      <parent>ovsbrA</parent>
                    //      <tag>10</tag>
                    //    </vlan>
                    //   </ovs-bridge>
                    //
                    //  The `vlan tag` is set in the corresponding ovs-port and needs to
                    //  be inherited to the ports of this "fake bridge" (see:
                    //  update_parent_connection() )
                    //
                    if let Some(vlan) = &ovs_bridge.vlan {
                        let link = Link {
                            master: Some(vlan.parent.clone()),
                            port: Some(LinkPort {
                                port_type: LinkPortType::OvsBridge,
                                priority: None,
                                path_cost: None,
                                queue_id: None,
                                prio: None,
                                sticky: false,
                                lacp_key: None,
                                lacp_prio: None,
                            }),
                            ..Default::default()
                        };
                        parents.insert(connection.uuid, link);
                    }
                }
            }
            connection_result.connections.push(connection);
        }
    }

    // Store original parents before they get consumed by update_parent_connection loop
    let original_parents = parents.clone();

    loop {
        // This loop is needed, as we need to map the "ovs-port" of a "fake bridge"
        // to the "ovs-bridge" first. And then link all "ovs-ports" from the fakebridge
        // to the same "ovs-bridge".
        //
        let len = parents.len();
        update_parent_connection(&mut connection_result, &mut parents)?;
        if parents.is_empty() {
            break;
        }

        if len == parents.len() {
            let connections = connection_result
                .connections
                .iter()
                .filter(|c| parents.contains_key(&c.uuid))
                .map(|c| c.id.as_str())
                .collect::<Vec<&str>>()
                .join("\n");
            anyhow::bail!("Unexpected error, port connection is missing controller: {connections}");
        }
    }

    // Apply team port options (prio, sticky) to bond configuration
    connection_result.has_warnings |=
        apply_team_port_options_to_bond(&mut connection_result.connections, &original_parents);

    if settings.activate_connections {
        let system_interfaces = list_system_interfaces()?;
        for con in &mut connection_result.connections {
            let interface_name = match &con.config {
                ConnectionConfig::Dummy => continue,
                ConnectionConfig::Bond(_) => continue,
                ConnectionConfig::Loopback => continue,
                ConnectionConfig::Vlan(_) => continue,
                ConnectionConfig::Bridge(_) => continue,
                ConnectionConfig::Tun(_) => continue,
                ConnectionConfig::OvsBridge(_) => continue,
                ConnectionConfig::OvsPort(_) => continue,
                ConnectionConfig::OvsInterface(_) => continue,
                ConnectionConfig::Ethernet => con.interface.as_ref().unwrap_or(&con.id),
                ConnectionConfig::Wireless(_) => con.interface.as_ref().unwrap_or(&con.id),
                ConnectionConfig::Infiniband(config) => {
                    if let Some(parent) = &config.parent {
                        parent
                    } else {
                        continue;
                    }
                }
            };

            if con.autoconnect && !system_interfaces.contains(interface_name) {
                con.status = agama_network::types::Status::Down;
            }
        }
    }

    let mut state_result = NetworkStateResult {
        has_warnings: connection_result.has_warnings,
        ..Default::default()
    };

    for connection in &connection_result.connections {
        state_result
            .network_state
            .add_connection(connection.clone())?;
    }

    Ok(state_result)
}

pub async fn apply_networkstate(
    state: &mut NetworkState,
    netconfig: Option<Netconfig>,
) -> Result<(), anyhow::Error> {
    let nm = NetworkManagerAdapter::from_system().await?;

    if let Some(netconfig) = netconfig {
        let current_state = nm.read(StateConfig::default()).await?;
        let mut loopback = match current_state.get_connection("lo") {
            Some(lo) => lo.clone(),
            None => create_lo_connection(),
        };
        loopback.ip_config.nameservers = netconfig.static_dns_servers.clone();

        if let Some(static_dns_searchlist) = &netconfig.static_dns_searchlist {
            loopback.ip_config.dns_searchlist = static_dns_searchlist.clone();
        }

        state.add_connection(loopback)?;

        apply_dns_policy(&netconfig, state)?;

        // When a connection didn't get a dns priority it means it wasn't matched by the netconfig policy,
        // so ignore-auto-dns should be set to true.
        for con in state.connections.iter_mut() {
            if con.id != "lo"
                && con.ip_config.dns_priority4.is_none()
                && con.ip_config.dns_priority6.is_none()
            {
                con.ip_config.ignore_auto_dns = true;
            }
        }
    }

    nm.write(state).await?;
    Ok(())
}

fn list_system_interfaces() -> Result<HashSet<String>, anyhow::Error> {
    let mut interface_names = HashSet::new();

    for ifaddr in getifaddrs()? {
        interface_names.insert(ifaddr.interface_name);
    }

    Ok(interface_names)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bond::{Bond as WickedBond, WickedBondMode};
    use crate::interface::{Interface, Link, LinkPort, LinkPortType};
    use crate::ovs::OvsBridge;
    use crate::reader::InterfacesResult;
    use crate::team::{Runner, RunnerName, Team as WickedTeam};
    use log::Level;

    #[test]
    fn test_apply_team_port_options_prio_to_primary() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with lower priority
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(10),
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with higher priority
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Higher priority
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            assert_eq!(bond.options.0.get("primary").unwrap(), "eth1");
        } else {
            panic!("Expected bond config");
        }
    }

    #[test]
    fn test_apply_team_port_options_sticky_to_primary_reselect() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with highest priority and sticky
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Highest prio and sticky
                        sticky: true,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with lower priority
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50), // Lower prio, not sticky
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(!result.has_warnings); // No warnings - perfect mapping

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            assert_eq!(bond.options.0.get("primary").unwrap(), "eth0");
            assert_eq!(bond.options.0.get("primary_reselect").unwrap(), "failure");
        } else {
            panic!("Expected bond config");
        }
    }

    #[test]
    fn test_apply_team_port_options_multiple_prio_warns() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with low priority
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(10),
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with medium priority
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50),
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 3 (eth2) with highest priority
            Interface {
                name: "eth2".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100),
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(result.has_warnings);

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            assert_eq!(bond.options.0.get("primary").unwrap(), "eth2");
        } else {
            panic!("Expected bond config");
        }

        testing_logger::validate(|captured_logs| {
            let warnings: Vec<_> = captured_logs
                .iter()
                .filter(|l| l.level == Level::Warn)
                .collect();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].body.contains("3 different priority levels"));
            assert!(warnings[0].body.contains("primary vs backup"));
        });
    }

    #[test]
    fn test_apply_team_port_options_non_primary_sticky_warns() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with highest priority, not sticky
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Highest prio, not sticky
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with lower priority, but sticky
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50), // Lower prio, but sticky - should warn
                        sticky: true,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(result.has_warnings);

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            assert_eq!(bond.options.0.get("primary").unwrap(), "eth0");
            // primary_reselect should NOT be set because highest prio is not sticky
            assert!(!bond.options.0.contains_key("primary_reselect"));
        } else {
            panic!("Expected bond config");
        }

        testing_logger::validate(|captured_logs| {
            let warnings: Vec<_> = captured_logs
                .iter()
                .filter(|l| l.level == Level::Warn)
                .collect();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].body.contains("eth1"));
        });
    }

    #[test]
    fn test_apply_team_port_options_sticky_without_prio_warns() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with sticky but no prio
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None,   // No prio set
                        sticky: true, // But sticky is set
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(result.has_warnings);

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            // Should NOT set primary_reselect because no prio is set
            assert!(!bond.options.0.contains_key("primary_reselect"));
        } else {
            panic!("Expected bond config");
        }

        testing_logger::validate(|captured_logs| {
            let warnings: Vec<_> = captured_logs
                .iter()
                .filter(|l| l.level == Level::Warn)
                .collect();
            assert_eq!(warnings.len(), 1);
            assert!(warnings[0].body.contains("eth0"));
        });
    }

    #[test]
    fn test_apply_team_port_options_duplicate_prio_warns() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with prio 100
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Same as eth1
                        sticky: true, // Sticky, but shouldn't set primary_reselect due to ambiguous priority
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with same prio 100
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Same as eth0
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(result.has_warnings);

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            // Should NOT set primary because of ambiguous priority
            assert!(!bond.options.0.contains_key("primary"));
            assert!(!bond.options.0.contains_key("primary_reselect"));
        } else {
            panic!("Expected bond config");
        }

        testing_logger::validate(|captured_logs| {
            let warnings: Vec<_> = captured_logs
                .iter()
                .filter(|l| l.level == Level::Warn)
                .collect();
            assert_eq!(warnings.len(), 1);
            // Should mention both ports and the duplicate priority
            assert!(warnings[0].body.contains("eth0"));
            assert!(warnings[0].body.contains("eth1"));
            assert!(warnings[0].body.contains("100"));
            assert!(warnings[0].body.contains("same highest prio"));
        });
    }

    #[test]
    fn test_apply_team_port_options_clear_primary_with_duplicate_backups() {
        testing_logger::setup();
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        let interfaces = vec![
            // Team port 1 (eth0) with highest priority
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Highest - should be primary
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) with lower priority
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50), // Lower - backup
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 3 (eth2) with same priority as eth1
            Interface {
                name: "eth2".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50), // Same as eth1 - also backup
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();
        assert!(!result.has_warnings); // Should map cleanly - only 2 priority levels (100 and 50)

        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond) = &team0.config {
            // eth0 should be primary (highest prio)
            assert_eq!(bond.options.0.get("primary").unwrap(), "eth0");
            // No primary_reselect since not sticky
            assert!(!bond.options.0.contains_key("primary_reselect"));
        } else {
            panic!("Expected bond config");
        }

        testing_logger::validate(|captured_logs| {
            let warnings: Vec<_> = captured_logs
                .iter()
                .filter(|l| l.level == Level::Warn)
                .collect();
            // No warnings - only 2 priority levels
            assert_eq!(warnings.len(), 0);
        });
    }

    #[test]
    fn test_team_single_prio_multiple_interfaces() {
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        // Create a team with 3 ports where only one has a prio
        let interfaces = vec![
            // Team port 1 (eth0) - has prio
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Only this port has a prio
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 2 (eth1) - no prio
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None, // No prio
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team port 3 (eth2) - no prio
            Interface {
                name: "eth2".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None, // No prio
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Team controller
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();

        // Should not have warnings - single prio is unambiguous
        assert!(!result.has_warnings);

        // Verify team was converted to bond
        let team0 = result
            .network_state
            .get_connection("team0")
            .expect("team0 should exist");

        if let ConnectionConfig::Bond(bond_config) = &team0.config {
            assert_eq!(
                bond_config.options.0.get("primary"),
                Some(&"eth0".to_string()),
                "eth0 should be the primary port"
            );
            // No primary_reselect since not sticky
            assert!(
                !bond_config.options.0.contains_key("primary_reselect"),
                "primary_reselect should not be set"
            );
        } else {
            panic!("team0 should have been converted to bond");
        }
    }

    #[test]
    fn test_to_networkstate_complex_topology() {
        let _ = MIGRATION_SETTINGS.set(crate::MigrationSettings::default());

        // Create a complex topology with:
        // 1. A regular bond (bond0) with 2 ports (eth0, eth1)
        // 2. A team (team0) that will be converted to bond with 2 ports (eth2 with prio+sticky, eth3)
        // 3. An OVS bridge (ovsbr0) with 2 ports (eth4 untagged, eth5 as vlan port)
        //
        // NOTE: Interfaces are intentionally shuffled (ports before controllers)
        // to ensure the migration code doesn't rely on ordering

        let interfaces = vec![
            // Team port 1 (eth2)
            Interface {
                name: "eth2".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(100), // Highest priority
                        sticky: true,    // Sticky port
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // OVS port 1 (eth4)
            Interface {
                name: "eth4".to_string(),
                link: Link {
                    master: Some("ovsbr0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::OvsBridge,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None,
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Bond controller
            Interface {
                name: "bond0".to_string(),
                bond: Some(WickedBond {
                    mode: WickedBondMode::ActiveBackup,
                    miimon: None,
                    arpmon: None,
                    xmit_hash_policy: None,
                    packets_per_slave: None,
                    tlb_dynamic_lb: None,
                    lacp_rate: None,
                    ad_select: None,
                    ad_user_port_key: None,
                    ad_actor_sys_prio: None,
                    ad_actor_system: None,
                    min_links: None,
                    primary_reselect: None,
                    primary: None,
                    num_grat_arp: None,
                    num_unsol_na: None,
                    fail_over_mac: None,
                    all_slaves_active: None,
                    resend_igmp: None,
                    lp_interval: None,
                    address: None,
                }),
                ..Default::default()
            },
            // Team controller (will be converted to bond)
            Interface {
                name: "team0".to_string(),
                team: Some(WickedTeam {
                    runner: Some(Runner {
                        name: RunnerName::ActiveBackup,
                        ..Default::default()
                    }),
                    ..Default::default()
                }),
                ..Default::default()
            },
            // Bond port 1 (eth0)
            Interface {
                name: "eth0".to_string(),
                link: Link {
                    master: Some("bond0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Bond,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None,
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // OVS bridge
            Interface {
                name: "ovsbr0".to_string(),
                ovs_bridge: Some(OvsBridge { vlan: None }),
                ..Default::default()
            },
            // Team port 2 (eth3)
            Interface {
                name: "eth3".to_string(),
                link: Link {
                    master: Some("team0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Team,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: Some(50), // Lower priority
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
            // Bond port 2 (eth1)
            Interface {
                name: "eth1".to_string(),
                link: Link {
                    master: Some("bond0".to_string()),
                    mtu: None,
                    port: Some(LinkPort {
                        port_type: LinkPortType::Bond,
                        priority: None,
                        path_cost: None,
                        queue_id: None,
                        prio: None,
                        sticky: false,
                        lacp_key: None,
                        lacp_prio: None,
                    }),
                },
                ..Default::default()
            },
        ];

        let interfaces_result = InterfacesResult {
            interfaces,
            netconfig: None,
            netconfig_dhcp: None,
            has_warnings: false,
        };

        let result = to_networkstate(&interfaces_result).unwrap();

        let find_connection_by_interface =
            |name: &str| result.network_state.get_connection(name).cloned();

        // Verify regular bond (bond0) exists and is still a bond
        let bond0 = find_connection_by_interface("bond0").expect("bond0 should exist");
        assert!(
            matches!(bond0.config, ConnectionConfig::Bond(_)),
            "bond0 should be a bond"
        );

        // Verify team0 was converted to bond
        let team0 = find_connection_by_interface("team0").expect("team0 should exist");
        if let ConnectionConfig::Bond(bond_config) = &team0.config {
            assert_eq!(
                bond_config.options.0.get("primary"),
                Some(&"eth2".to_string()),
                "eth2 should be the primary port"
            );
            assert_eq!(
                bond_config.options.0.get("primary_reselect"),
                Some(&"failure".to_string()),
                "primary_reselect should be set to failure due to sticky"
            );
        } else {
            panic!("team0 should have been converted to bond");
        }

        // Verify OVS bridge exists (created with "-bridge" suffix)
        let ovsbr0_bridge = result
            .network_state
            .get_connection("ovsbr0-bridge")
            .expect("OVS bridge should exist");
        assert!(
            matches!(ovsbr0_bridge.config, ConnectionConfig::OvsBridge(_)),
            "ovsbr0-bridge should be an OVS bridge"
        );

        // Verify OVS port for eth4 exists
        let eth4_port = find_connection_by_interface("eth4").expect("eth4 should exist");
        assert!(
            eth4_port.controller.is_some(),
            "eth4 should have a controller"
        );

        // Verify all expected connections exist
        assert!(
            find_connection_by_interface("bond0").is_some(),
            "bond0 should exist"
        );
        assert!(
            find_connection_by_interface("eth0").is_some(),
            "eth0 should exist"
        );
        assert!(
            find_connection_by_interface("eth1").is_some(),
            "eth1 should exist"
        );
        assert!(
            find_connection_by_interface("team0").is_some(),
            "team0 should exist"
        );
        assert!(
            find_connection_by_interface("eth2").is_some(),
            "eth2 should exist"
        );
        assert!(
            find_connection_by_interface("eth3").is_some(),
            "eth3 should exist"
        );
        assert!(
            find_connection_by_interface("eth4").is_some(),
            "eth4 should exist"
        );
    }
}
