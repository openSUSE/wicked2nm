use crate::interface::{Interface, Link, LinkPort, LinkPortType};
use crate::netconfig::{apply_dns_policy, Netconfig};
use crate::netconfig_dhcp::NetconfigDhcp;
use crate::MIGRATION_SETTINGS;
use agama_network::model::{
    Connection, ConnectionConfig, GeneralState, IpConfig, MatchConfig, StateConfig,
};
use agama_network::{model, Adapter, NetworkManagerAdapter, NetworkState};
use cidr::IpInet;
use std::fmt;
use std::str::FromStr;
use std::{collections::HashMap, error::Error};
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
    connections: &mut [Connection],
    parents: &mut HashMap<Uuid, Link>,
) -> Result<(), anyhow::Error> {
    let settings = MIGRATION_SETTINGS.get().unwrap();
    let mut parent_uuids: HashMap<Uuid, ParentMatch> = HashMap::new();

    for (port_uuid, parent) in parents.iter() {
        let Some(parent_con) = connections.iter().find(|c| c.interface == parent.master) else {
            log::warn!(
                "Missing parent connection with interface {} for port {port_uuid}",
                parent.clone().master.unwrap()
            );
            if !settings.continue_migration {
                anyhow::bail!("Migration of {port_uuid} failed because of warnings, use the `--continue-migration` flag to ignore");
            }
            continue;
        };

        let Some(port) = &parent.port else {
            continue;
        };

        if let Some(parent_match) = match port.port_type {
            LinkPortType::OvsBridge => get_parentmatch_ovsbridge(parent_con, connections),
            _ => Some(ParentMatch::from(parent_con.uuid)),
        } {
            parent_uuids.insert(*port_uuid, parent_match);
        }
    }

    for (port_uuid, parent_match) in parent_uuids {
        let Some(connection) = connections.iter_mut().find(|c| c.uuid == port_uuid) else {
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

fn create_lo_connection() -> Connection {
    Connection {
        id: "lo".to_string(),
        ip_config: IpConfig {
            method4: model::Ipv4Method::Manual,
            method6: model::Ipv6Method::Manual,
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

pub async fn migrate(
    interfaces: Vec<Interface>,
    netconfig: Option<Netconfig>,
    netconfig_dhcp: Option<NetconfigDhcp>,
) -> Result<(), Box<dyn Error>> {
    let settings = MIGRATION_SETTINGS.get().unwrap();
    let mut parents: HashMap<Uuid, Link> = HashMap::new();
    let mut connections: Vec<Connection> = vec![];

    for interface in interfaces {
        let connection_result = interface.to_connection(&netconfig_dhcp)?;
        if !connection_result.warnings.is_empty() {
            for connection_error in &connection_result.warnings {
                log::warn!("{connection_error}");
            }
            if !settings.continue_migration {
                return Err(anyhow::anyhow!(
                    "Migration of {} failed because of warnings, use the `--continue-migration` flag to ignore",
                    connection_result.connections[0].id
                )
                .into());
            }
        }

        for connection in connection_result.connections {
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
                            }),
                            ..Default::default()
                        };
                        parents.insert(connection.uuid, link);
                    }
                }
            }
            connections.push(connection);
        }
    }

    loop {
        // This loop is needed, as we need to map the "ovs-port" of a "fake bridge"
        // to the "ovs-bridge" first. And then link all "ovs-ports" from the fakebridge
        // to the same "ovs-bridge".
        //
        let len = parents.len();
        update_parent_connection(&mut connections, &mut parents)?;
        if parents.is_empty() {
            break;
        }

        if len == parents.len() {
            let connections = connections
                .iter()
                .filter(|c| parents.contains_key(&c.uuid))
                .map(|c| c.id.as_str())
                .collect::<Vec<&str>>()
                .join("\n");
            return Err(anyhow::anyhow!(
                "Unexpected error, port connection is missing controller: {connections}"
            )
            .into());
        }
    }

    let mut state = NetworkState::new(GeneralState::default(), vec![], vec![], vec![]);
    for connection in &connections {
        state.add_connection(connection.clone())?;
    }

    if settings.dry_run {
        for connection in state.connections {
            log::debug!("{connection:#?}");
        }
        return Ok(());
    }
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

        apply_dns_policy(&netconfig, &mut state)?;

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

    nm.write(&state).await?;
    Ok(())
}
