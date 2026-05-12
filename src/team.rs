use agama_network::model::{self};
use agama_network::types::BondMode as AgamaBondMode;
use serde::{Deserialize, Serialize};
use serde_with::{skip_serializing_none, DeserializeFromStr, SerializeDisplay};
use std::collections::HashMap;
use strum_macros::{Display, EnumString};

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Team {
    // ignored
    pub debug_level: Option<u32>,
    pub notify_peers: Option<NotifyPeers>,
    pub mcast_rejoin: Option<McastRejoin>,
    pub runner: Option<Runner>,
    pub link_watch_policy: Option<String>,
    pub link_watch: Option<LinkWatch>,
    pub address: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct NotifyPeers {
    pub count: Option<u32>,
    pub interval: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct McastRejoin {
    pub count: Option<u32>,
    pub interval: Option<u32>,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display, Default)]
#[strum(serialize_all = "lowercase")]
pub enum RunnerName {
    Lacp,
    ActiveBackup,
    #[default]
    RoundRobin,
    Broadcast,
    LoadBalance,
    Random,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display, Default)]
#[strum(serialize_all = "snake_case")]
pub enum SelectPolicy {
    #[default]
    LacpPrio,
    LacpPrioStable,
    Bandwidth,
    Count,
    PortOptions,
}

fn default_true() -> bool {
    true
}

fn default_sys_prio() -> u16 {
    255
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display, Default)]
#[strum(serialize_all = "snake_case")]
pub enum HwAddrPolicy {
    #[default]
    SameAll,
    ByActive,
    OnlyActive,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Runner {
    #[serde(rename = "@name", default)]
    pub name: RunnerName,
    // LACP runner fields
    #[serde(default = "default_true")]
    pub active: bool,
    #[serde(default)]
    pub fast_rate: bool,
    #[serde(default = "default_sys_prio")]
    pub sys_prio: u16,
    #[serde(default)]
    pub min_ports: u16,
    #[serde(default)]
    pub select_policy: SelectPolicy,
    // LoadBalance and LACP runner fields
    pub tx_hash: Option<String>,
    pub tx_balancer: Option<TxBalancer>,
    // ActiveBackup runner fields
    pub hwaddr_policy: Option<HwAddrPolicy>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct TxBalancer {
    pub name: Option<String>,
    pub balancing_interval: Option<u32>,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct LinkWatch {
    #[serde(rename = "watch", default)]
    pub watches: Vec<Watch>,
}

#[derive(Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display, Default)]
#[strum(serialize_all = "snake_case")]
pub enum WatchName {
    #[default]
    Ethtool,
    ArpPing,
    NsnaPing,
    Tipc,
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Default)]
pub struct Watch {
    #[serde(rename = "@name", default)]
    pub name: WatchName,
    // Ethtool watch fields
    #[serde(default)]
    pub delay_up: u32,
    #[serde(default)]
    pub delay_down: u32,
    // ARP ping and NSNA ping watch fields
    #[serde(default)]
    pub interval: u32,
    #[serde(default)]
    pub init_wait: u32,
    pub target_host: Option<String>,
    // ARP ping specific fields
    pub source_host: Option<String>,
    pub validate_active: Option<bool>,
    pub validate_inactive: Option<bool>,
    pub send_always: Option<bool>,
    #[serde(default)]
    pub missed_max: u32,
    pub vlanid: Option<u16>,
    // TIPC watch fields
    pub bearer: Option<String>,
}

/// Calculate greatest common divisor using Euclidean algorithm
fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let temp = b;
        b = a % b;
        a = temp;
    }
    a
}

impl Team {
    /// Convert Team configuration to bond ConnectionConfig, returning (config, has_warnings)
    pub fn to_connection_config(&self) -> (model::ConnectionConfig, bool) {
        let mut bond_options: HashMap<String, String> = HashMap::new();
        let mut mode = AgamaBondMode::RoundRobin; // Default fallback
        let mut has_warnings = false;

        // Handle top-level team options
        if let Some(notify_peers) = &self.notify_peers {
            if let Some(count) = notify_peers.count {
                bond_options.insert(String::from("num_grat_arp"), count.to_string());
                bond_options.insert(String::from("num_unsol_na"), count.to_string());
            }
            if let Some(interval) = notify_peers.interval {
                bond_options.insert(String::from("peer_notif_delay"), interval.to_string());
            }
        }

        if let Some(mcast_rejoin) = &self.mcast_rejoin {
            if let Some(count) = mcast_rejoin.count {
                bond_options.insert(String::from("resend_igmp"), count.to_string());
            }
            if mcast_rejoin.interval.is_some() {
                log::warn!(
                    "Team 'mcast_rejoin.interval' is not supported in bond configuration - bond uses hardcoded 200ms interval"
                );
                has_warnings = true;
            }
        }

        if self.link_watch_policy.is_some() {
            log::warn!("Team 'link_watch_policy' is not supported in bond configuration");
            has_warnings = true;
        }

        if let Some(runner) = &self.runner {
            match runner.name {
                RunnerName::Lacp => {
                    mode = AgamaBondMode::LACP;
                    bond_options.insert(
                        String::from("lacp_active"),
                        if runner.active { "1" } else { "0" }.to_string(),
                    );
                    bond_options.insert(
                        String::from("lacp_rate"),
                        if runner.fast_rate { "fast" } else { "slow" }.to_string(),
                    );

                    // Bond's ad_actor_sys_prio valid range is 1-65535, team allows 0-65535
                    let sys_prio = if runner.sys_prio == 0 {
                        log::info!("Team sys_prio '0' converted to '1' - bond range starts at 1");
                        1
                    } else {
                        runner.sys_prio
                    };
                    bond_options.insert(String::from("ad_actor_sys_prio"), sys_prio.to_string());

                    bond_options.insert(String::from("min_links"), runner.min_ports.to_string());

                    let val = match runner.select_policy {
                        SelectPolicy::LacpPrio => {
                            log::info!("Team select_policy 'lacp_prio' approximated to bond ad_select 'stable'.");
                            "stable"
                        }
                        SelectPolicy::LacpPrioStable => {
                            log::info!("Team select_policy 'lacp_prio_stable' approximated to bond ad_select 'stable'.");
                            "stable"
                        }
                        SelectPolicy::Bandwidth => "bandwidth",
                        SelectPolicy::Count => "count",
                        SelectPolicy::PortOptions => {
                            log::warn!(
                                "Team select_policy 'port_options' not supported in kernel 6.12 - requires kernel 6.18+ (actor_port_prio). Using 'stable' instead."
                            );
                            has_warnings = true;
                            "stable"
                        }
                    };
                    bond_options.insert(String::from("ad_select"), val.to_string());

                    if runner.tx_balancer.is_some() {
                        log::info!(
                            "Team LACP with tx_balancer converted to bond 802.3ad - dynamic flow rebalancing not available in bond"
                        );
                    }
                }
                RunnerName::ActiveBackup => {
                    mode = AgamaBondMode::ActiveBackup;
                    if let Some(hwaddr_policy) = &runner.hwaddr_policy {
                        let fail_over_mac = match hwaddr_policy {
                            HwAddrPolicy::SameAll => "none",
                            HwAddrPolicy::ByActive => "active",
                            HwAddrPolicy::OnlyActive => "follow",
                        };
                        bond_options
                            .insert(String::from("fail_over_mac"), fail_over_mac.to_string());
                    }
                }
                RunnerName::RoundRobin => {
                    mode = AgamaBondMode::RoundRobin;
                }
                RunnerName::Broadcast => {
                    mode = AgamaBondMode::Broadcast;
                }
                RunnerName::LoadBalance => {
                    mode = AgamaBondMode::BalanceTLB;
                    // Not possible in wicked currently, but maybe things change in the future
                    if runner.tx_balancer.is_none() {
                        bond_options.insert(String::from("tlb_dynamic_lb"), String::from("0"));
                    }
                }
                RunnerName::Random => {
                    log::info!("Team runner 'random' approximated by bond mode 'balance-rr' with packets_per_slave=0");
                    mode = AgamaBondMode::RoundRobin;
                    bond_options.insert(String::from("packets_per_slave"), String::from("0"));
                }
            }

            if let Some(tx_hash) = &runner.tx_hash {
                let team: Vec<&str> = tx_hash.split(',').map(|s| s.trim()).collect();

                // Expand team aliases to actual elements
                let mut team_expanded = Vec::new();
                for &elem in &team {
                    match elem {
                        "ip" | "l3" => {
                            team_expanded.push("ipv4");
                            team_expanded.push("ipv6");
                        }
                        "l4" => {
                            team_expanded.push("tcp");
                            team_expanded.push("udp");
                            team_expanded.push("sctp");
                        }
                        other => team_expanded.push(other),
                    }
                }

                // Define bond policies and what they hash on
                let policies = [
                    ("layer2", vec!["eth"]),
                    ("layer2+3", vec!["eth", "ipv4", "ipv6"]),
                    ("layer3+4", vec!["ipv4", "ipv6", "tcp", "udp", "sctp"]),
                    ("vlan+srcmac", vec!["vlan"]),
                ];

                // Find the bond policy with most matches
                let mut best_policy = "layer2";
                let mut best_elements = &policies[0].1;
                let mut best_count = 0;

                for (policy, elements) in &policies {
                    let count = team_expanded
                        .iter()
                        .filter(|&elem| elements.contains(elem))
                        .count();
                    if count > best_count {
                        best_policy = policy;
                        best_elements = elements;
                        best_count = count;
                    }
                }

                bond_options.insert(String::from("xmit_hash_policy"), best_policy.to_string());

                // Check if there are team elements not in the selected bond policy
                let team_set: std::collections::HashSet<_> = team_expanded.iter().collect();
                let bond_set: std::collections::HashSet<_> = best_elements.iter().collect();
                let missing: Vec<_> = team_set.difference(&bond_set).map(|&&s| s).collect();

                if !missing.is_empty() {
                    log::info!(
                        "Team tx_hash '{}' mapped to bond xmit_hash_policy '{}' - elements not covered in this conversion: {}",
                        tx_hash,
                        best_policy,
                        missing.join(", ")
                    );
                }
            // If balancer is set it should fall back to the team default: ["eth", "ipv4", "ipv6"] → layer2+3
            } else if runner.tx_balancer.is_some() {
                // When tx_hash is not explicitly set, use team's default
                bond_options.insert(String::from("xmit_hash_policy"), String::from("layer2+3"));
            }
        }

        if let Some(lw_container) = &self.link_watch {
            // Bond only supports one monitoring method at a time (miimon OR arp_interval, not both)
            // Prefer ethtool (miimon) over arp_ping/nsna_ping, warn if multiple are configured
            let has_ethtool = lw_container
                .watches
                .iter()
                .any(|w| w.name == WatchName::Ethtool);
            let has_arp_ping = lw_container
                .watches
                .iter()
                .any(|w| w.name == WatchName::ArpPing);
            let has_nsna_ping = lw_container
                .watches
                .iter()
                .any(|w| w.name == WatchName::NsnaPing);

            // Warn if we have ethtool AND (arp_ping OR nsna_ping)
            if has_ethtool && (has_arp_ping || has_nsna_ping) {
                log::warn!("Team has both ethtool and arp_ping/nsna_ping watches - bond supports only one monitoring method. Using miimon (ethtool), ignoring arp_ping/nsna_ping.");
                has_warnings = true;
            }

            // Track if we've already processed ethtool or warned about unsupported fields
            let mut ethtool_done = false;
            let mut arp_warnings_done = false;
            let mut nsna_warnings_done = false;

            // Process watches - prefer ethtool, fallback to arp_ping/nsna_ping
            for watch in &lw_container.watches {
                match watch.name {
                    WatchName::Ethtool => {
                        if ethtool_done {
                            log::info!("Team has multiple ethtool watches - bond only supports one. Using first watch's settings. Behavior may differ from team's link_watch_policy.");
                            continue;
                        }

                        // Calculate miimon as GCD of delays to ensure updelay/downdelay are exact multiples
                        // Bond requires updelay and downdelay to be multiples of miimon
                        let miimon = if watch.delay_up > 0 && watch.delay_down > 0 {
                            gcd(watch.delay_up, watch.delay_down)
                        } else if watch.delay_up > 0 {
                            watch.delay_up
                        } else if watch.delay_down > 0 {
                            watch.delay_down
                        } else {
                            100 // Default when no delays configured
                        };

                        bond_options.insert(String::from("miimon"), miimon.to_string());

                        if watch.delay_up > 0 {
                            bond_options
                                .insert(String::from("updelay"), watch.delay_up.to_string());
                        }
                        if watch.delay_down > 0 {
                            bond_options
                                .insert(String::from("downdelay"), watch.delay_down.to_string());
                        }
                        ethtool_done = true;
                    }
                    WatchName::ArpPing if !has_ethtool => {
                        // Handle interval - insert if not present, warn if different
                        if let Some(existing_interval) = bond_options.get("arp_interval") {
                            if existing_interval != &watch.interval.to_string() {
                                log::warn!("Team has multiple arp_ping/nsna_ping watches with different intervals - bond only supports one value (shared for both IPv4 and IPv6). Using first watch's interval.");
                                has_warnings = true;
                            }
                        } else {
                            bond_options
                                .insert(String::from("arp_interval"), watch.interval.to_string());
                        }

                        // Handle arp_validate - insert if not present, warn if different
                        if watch.validate_active.is_some() || watch.validate_inactive.is_some() {
                            let validate_active = watch.validate_active.unwrap_or(false);
                            let validate_inactive = watch.validate_inactive.unwrap_or(false);

                            let arp_validate = match (validate_active, validate_inactive) {
                                (true, true) => "all",
                                (true, false) => "active",
                                (false, true) => "backup",
                                (false, false) => "none",
                            };

                            if let Some(existing_validate) = bond_options.get("arp_validate") {
                                if existing_validate != arp_validate {
                                    log::warn!("Team has multiple arp_ping watches with different validation settings - bond only supports one value. Using first watch's validation.");
                                    has_warnings = true;
                                }
                            } else {
                                bond_options
                                    .insert(String::from("arp_validate"), arp_validate.to_string());
                            }
                        }

                        // Add target to comma-separated list
                        if let Some(target) = &watch.target_host {
                            if let Some(existing_targets) = bond_options.get("arp_ip_target") {
                                bond_options.insert(
                                    String::from("arp_ip_target"),
                                    format!("{},{}", existing_targets, target),
                                );
                            } else {
                                bond_options.insert(String::from("arp_ip_target"), target.clone());
                            }
                        }

                        // Handle missed_max - insert if not present, warn if different
                        if watch.missed_max > 0 {
                            if let Some(existing_missed_max) = bond_options.get("arp_missed_max") {
                                if existing_missed_max != &watch.missed_max.to_string() {
                                    log::warn!("Team has multiple arp_ping/nsna_ping watches with different missed_max values - bond only supports one value (shared for both IPv4 and IPv6). Using first watch's missed_max.");
                                    has_warnings = true;
                                }
                            } else {
                                bond_options.insert(
                                    String::from("arp_missed_max"),
                                    watch.missed_max.to_string(),
                                );
                            }
                        }

                        // Warn about unsupported ARP ping fields (only once)
                        if !arp_warnings_done {
                            if watch.source_host.is_some() {
                                log::warn!("Team ARP ping 'source_host' is not supported in bond configuration");
                                has_warnings = true;
                            }
                            if watch.init_wait > 0 {
                                log::warn!("Team ARP ping 'init_wait' is not supported in bond configuration");
                                has_warnings = true;
                            }
                            if watch.send_always.is_some() {
                                log::warn!("Team ARP ping 'send_always' is not supported in bond configuration");
                                has_warnings = true;
                            }
                            if watch.vlanid.is_some() {
                                log::warn!(
                                    "Team ARP ping 'vlanid' is not supported in bond configuration"
                                );
                                has_warnings = true;
                            }
                            arp_warnings_done = true;
                        }
                    }
                    WatchName::NsnaPing if !has_ethtool => {
                        // Handle interval - insert if not present, warn if different (shared with arp_ping)
                        if let Some(existing_interval) = bond_options.get("arp_interval") {
                            if existing_interval != &watch.interval.to_string() {
                                log::warn!("Team has multiple arp_ping/nsna_ping watches with different intervals - bond only supports one value (shared for both IPv4 and IPv6). Using first watch's interval.");
                                has_warnings = true;
                            }
                        } else {
                            bond_options
                                .insert(String::from("arp_interval"), watch.interval.to_string());
                        }

                        // Append target_host to ns_ip6_target (comma-separated)
                        if let Some(target) = &watch.target_host {
                            if let Some(existing_targets) = bond_options.get("ns_ip6_target") {
                                bond_options.insert(
                                    String::from("ns_ip6_target"),
                                    format!("{},{}", existing_targets, target),
                                );
                            } else {
                                bond_options.insert(String::from("ns_ip6_target"), target.clone());
                            }
                        }

                        // Handle missed_max - shared with arp_ping
                        if watch.missed_max > 0 {
                            if let Some(existing_missed_max) = bond_options.get("arp_missed_max") {
                                if existing_missed_max != &watch.missed_max.to_string() {
                                    log::warn!("Team has multiple arp_ping/nsna_ping watches with different missed_max values - bond only supports one value (shared for both IPv4 and IPv6). Using first watch's missed_max.");
                                    has_warnings = true;
                                }
                            } else {
                                bond_options.insert(
                                    String::from("arp_missed_max"),
                                    watch.missed_max.to_string(),
                                );
                            }
                        }

                        // Warn about unsupported NS/NA ping fields (only once)
                        if !nsna_warnings_done {
                            if watch.init_wait > 0 {
                                log::warn!("Team NS/NA ping 'init_wait' is not supported in bond configuration");
                                has_warnings = true;
                            }
                            nsna_warnings_done = true;
                        }
                    }
                    WatchName::Tipc => {
                        log::warn!("Team link watch 'tipc' is not supported in bond configuration");
                        has_warnings = true;
                    }
                    _ => {} // Skip non-preferred watches (e.g., arp_ping when ethtool exists)
                }
            }
        }

        let config = model::ConnectionConfig::Bond(model::BondConfig {
            options: model::BondOptions(bond_options),
            mode,
        });
        (config, has_warnings)
    }
}

impl From<&Team> for model::ConnectionConfig {
    fn from(team: &Team) -> model::ConnectionConfig {
        team.to_connection_config().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agama_network::model::ConnectionConfig;
    use agama_network::types::BondMode as AgamaBondMode;

    #[test]
    fn test_team_to_connection_config_lacp() {
        let team = Team {
            runner: Some(Runner {
                name: RunnerName::Lacp,
                active: true,
                fast_rate: true,
                sys_prio: 100,
                min_ports: 2,
                select_policy: SelectPolicy::Bandwidth,
                tx_hash: Some("ipv4,ipv6,l4".to_string()),
                tx_balancer: None,
                hwaddr_policy: None,
            }),
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 10,
                        delay_down: 20,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 100,
                        target_host: Some("1.2.3.4".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.mode, AgamaBondMode::LACP);
            let options = bond.options.0;
            assert_eq!(options.get("lacp_active").unwrap(), "1");
            assert_eq!(options.get("lacp_rate").unwrap(), "fast");
            assert_eq!(options.get("ad_actor_sys_prio").unwrap(), "100");
            assert_eq!(options.get("min_links").unwrap(), "2");
            assert_eq!(options.get("ad_select").unwrap(), "bandwidth");
            assert_eq!(options.get("xmit_hash_policy").unwrap(), "layer3+4");
            assert_eq!(options.get("miimon").unwrap(), "10"); // GCD(10, 20) = 10
            assert_eq!(options.get("updelay").unwrap(), "10");
            assert_eq!(options.get("downdelay").unwrap(), "20");
            // arp_interval should NOT be set (bond prefers ethtool/miimon when both present)
            assert!(options.get("arp_interval").is_none());
            assert!(options.get("arp_ip_target").is_none());
        } else {
            panic!("Expected Bond config");
        }
    }

    #[test]
    fn test_team_to_connection_config_tx_hash() {
        let cases = vec![
            // Basic elements
            (Some("ipv4,ipv6,l4".to_string()), "layer3+4"),
            (Some("tcp,udp".to_string()), "layer3+4"),
            (Some("ipv4,ipv6".to_string()), "layer2+3"),
            (Some("ipv4".to_string()), "layer2+3"),
            (Some("eth".to_string()), "layer2"),
            (Some("vlan,eth".to_string()), "layer2"),
            (Some("vlan".to_string()), "vlan+srcmac"),
            // Aliases
            (Some("ip".to_string()), "layer2+3"),
            (Some("l3".to_string()), "layer2+3"),
            (Some("l4".to_string()), "layer3+4"),
            // Most matches logic
            (Some("eth,ipv4".to_string()), "layer2+3"), // layer2+3 has 2 matches vs layer2 has 1
            (Some("eth,tcp".to_string()), "layer2"), // layer2 has 1 match (eth), layer3+4 has 1 (tcp) - tie goes to layer2
            (Some("ipv4,tcp".to_string()), "layer3+4"), // layer3+4 has 2 matches vs layer2+3 has 1
            (Some("ipv4,ipv6,tcp".to_string()), "layer3+4"), // layer3+4 has 3 matches vs layer2+3 has 2
            (None, ""),
        ];

        for (tx_hash, expected) in cases {
            let team = Team {
                runner: Some(Runner {
                    name: RunnerName::RoundRobin,
                    tx_hash: tx_hash.clone(),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let config: ConnectionConfig = (&team).into();
            if let ConnectionConfig::Bond(bond) = config {
                if tx_hash.is_some() {
                    assert_eq!(bond.options.0.get("xmit_hash_policy").unwrap(), expected);
                } else {
                    assert!(!bond.options.0.contains_key("xmit_hash_policy"));
                }
            }
        }
    }

    #[test]
    fn test_team_tx_hash_unsupported_elements_logging() {
        testing_logger::setup();

        let team = Team {
            runner: Some(Runner {
                name: RunnerName::RoundRobin,
                tx_hash: Some("eth,ipv4,tcp,vlan".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("xmit_hash_policy").unwrap(), "layer2+3");
        }

        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);

            // Log contains a - to differentiate
            // [0]: What tx_hash is and what is was converted to
            // [1]: What was missed in this conversion
            let captured_logs_split: Vec<_> =
                captured_logs[0].body.split('-').map(|s| s.trim()).collect();
            assert!(captured_logs_split[0].contains("'eth,ipv4,tcp,vlan'"));
            assert!(captured_logs_split[0].contains("'layer2+3'"));
            assert!(captured_logs_split[1].contains("tcp"));
            assert!(captured_logs_split[1].contains("vlan"));
        });
    }

    #[test]
    fn test_team_tx_hash_no_logging_when_all_supported() {
        testing_logger::setup();

        let team = Team {
            runner: Some(Runner {
                name: RunnerName::RoundRobin,
                tx_hash: Some("eth,ipv4".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("xmit_hash_policy").unwrap(), "layer2+3");
        }

        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 0);
        });
    }

    #[test]
    fn test_team_loadbalance_with_tx_balancer() {
        let team = Team {
            runner: Some(Runner {
                name: RunnerName::LoadBalance,
                tx_balancer: Some(TxBalancer {
                    name: Some("basic".to_string()),
                    balancing_interval: Some(100),
                }),
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.mode, AgamaBondMode::BalanceTLB);
            assert_eq!(bond.options.0.get("xmit_hash_policy").unwrap(), "layer2+3");
            // With tx_balancer, tlb_dynamic_lb should NOT be set (uses default=1)
            assert!(bond.options.0.get("tlb_dynamic_lb").is_none());
        }
    }

    // This scenario can't actually happen in wicked currently
    #[test]
    fn test_team_loadbalance_without_tx_balancer() {
        let team = Team {
            runner: Some(Runner {
                name: RunnerName::LoadBalance,
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.mode, AgamaBondMode::BalanceTLB);
            // Without tx_balancer, tlb_dynamic_lb should be set to 0 (passive mode)
            assert_eq!(bond.options.0.get("tlb_dynamic_lb").unwrap(), "0");
        }
    }

    #[test]
    fn test_runner_name_mapping() {
        let cases = vec![
            (RunnerName::Lacp, AgamaBondMode::LACP),
            (RunnerName::ActiveBackup, AgamaBondMode::ActiveBackup),
            (RunnerName::RoundRobin, AgamaBondMode::RoundRobin),
            (RunnerName::Broadcast, AgamaBondMode::Broadcast),
            (RunnerName::LoadBalance, AgamaBondMode::BalanceTLB),
            (RunnerName::Random, AgamaBondMode::RoundRobin),
        ];

        for (name, expected_mode) in cases {
            let team = Team {
                runner: Some(Runner {
                    name,
                    ..Default::default()
                }),
                ..Default::default()
            };

            let config: ConnectionConfig = (&team).into();
            if let ConnectionConfig::Bond(bond) = config {
                assert_eq!(bond.mode, expected_mode);
            }
        }
    }

    #[test]
    fn test_random_runner_packets_per_slave() {
        let team = Team {
            runner: Some(Runner {
                name: RunnerName::Random,
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.mode, AgamaBondMode::RoundRobin);
            assert_eq!(
                bond.options.0.get("packets_per_slave").unwrap(),
                "0",
                "Random runner should set packets_per_slave=0 for random slave selection"
            );
        }
    }

    #[test]
    fn test_select_policy_mapping() {
        let policies = vec![
            (SelectPolicy::LacpPrio, "stable"),
            (SelectPolicy::LacpPrioStable, "stable"),
            (SelectPolicy::Bandwidth, "bandwidth"),
            (SelectPolicy::Count, "count"),
            (SelectPolicy::PortOptions, "stable"),
        ];

        for (policy, expected) in policies {
            let team = Team {
                runner: Some(Runner {
                    name: RunnerName::Lacp,
                    select_policy: policy,
                    ..Default::default()
                }),
                ..Default::default()
            };

            let config: ConnectionConfig = (&team).into();
            if let ConnectionConfig::Bond(bond) = config {
                assert_eq!(bond.options.0.get("ad_select").unwrap(), expected);
            }
        }
    }

    #[test]
    fn test_lacp_sys_prio_zero_conversion() {
        testing_logger::setup();

        let team = Team {
            runner: Some(Runner {
                name: RunnerName::Lacp,
                sys_prio: 0,
                select_policy: SelectPolicy::Bandwidth,
                ..Default::default()
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("ad_actor_sys_prio").unwrap(), "1");
        }

        testing_logger::validate(|captured_logs| {
            assert_eq!(captured_logs.len(), 1);
            assert!(captured_logs[0].body.contains("sys_prio '0'"));
            assert!(captured_logs[0].body.contains("converted to '1'"));
        });
    }

    #[test]
    fn test_nsna_ping_supported() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![Watch {
                    name: WatchName::NsnaPing,
                    interval: 100,
                    target_host: Some("fe80::1".to_string()),
                    missed_max: 3,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(!has_warnings, "Single nsna_ping should not warn");

        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "100");
            assert_eq!(bond.options.0.get("ns_ip6_target").unwrap(), "fe80::1");
            assert_eq!(bond.options.0.get("arp_missed_max").unwrap(), "3");
            // No arp_validate - nsna_ping doesn't support validation options in team
        }
    }

    #[test]
    fn test_arp_ping_and_nsna_ping_same_interval() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 1000,
                        target_host: Some("fe80::1".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(!has_warnings, "Same interval should not warn");

        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(bond.options.0.get("arp_ip_target").unwrap(), "192.168.1.1");
            assert_eq!(bond.options.0.get("ns_ip6_target").unwrap(), "fe80::1");
        }
    }

    #[test]
    fn test_arp_ping_and_nsna_ping_different_intervals() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 2000,
                        target_host: Some("fe80::1".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "Different intervals should warn");

        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(bond.options.0.get("arp_ip_target").unwrap(), "192.168.1.1");
            assert_eq!(bond.options.0.get("ns_ip6_target").unwrap(), "fe80::1");
        }
    }

    #[test]
    fn test_multiple_nsna_ping_targets() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 1000,
                        target_host: Some("fe80::1".to_string()),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 1000,
                        target_host: Some("fe80::2".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(
            !has_warnings,
            "Multiple nsna_ping with same interval should not warn"
        );

        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(
                bond.options.0.get("ns_ip6_target").unwrap(),
                "fe80::1,fe80::2"
            );
        }
    }

    #[test]
    fn test_notify_peers_and_mcast_rejoin() {
        let team = Team {
            notify_peers: Some(NotifyPeers {
                count: Some(5),
                interval: Some(200),
            }),
            mcast_rejoin: Some(McastRejoin {
                count: Some(3),
                interval: Some(100),
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("num_grat_arp").unwrap(), "5");
            assert_eq!(bond.options.0.get("num_unsol_na").unwrap(), "5");
            assert_eq!(bond.options.0.get("peer_notif_delay").unwrap(), "200");
            assert_eq!(bond.options.0.get("resend_igmp").unwrap(), "3");
        }
    }

    #[test]
    fn test_mcast_rejoin_interval_warning() {
        let team = Team {
            mcast_rejoin: Some(McastRejoin {
                count: Some(5),
                interval: Some(150),
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "Should warn about mcast_rejoin.interval");
        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("resend_igmp").unwrap(), "5");
            // interval is ignored
        }
    }

    #[test]
    fn test_hwaddr_policy_mapping() {
        let test_cases = vec![
            (HwAddrPolicy::SameAll, "none"),
            (HwAddrPolicy::ByActive, "active"),
            (HwAddrPolicy::OnlyActive, "follow"),
        ];

        for (hwaddr_policy, expected_fail_over_mac) in test_cases {
            let team = Team {
                runner: Some(Runner {
                    name: RunnerName::ActiveBackup,
                    hwaddr_policy: Some(hwaddr_policy),
                    ..Default::default()
                }),
                ..Default::default()
            };

            let config: ConnectionConfig = (&team).into();
            if let ConnectionConfig::Bond(bond) = config {
                assert_eq!(bond.mode, AgamaBondMode::ActiveBackup);
                assert_eq!(
                    bond.options.0.get("fail_over_mac").unwrap(),
                    expected_fail_over_mac
                );
            }
        }
    }

    #[test]
    fn test_arp_validate_mapping() {
        let test_cases = vec![
            (Some(true), Some(true), "all"),
            (Some(true), Some(false), "active"),
            (Some(false), Some(true), "backup"),
            (Some(false), Some(false), "none"),
            (Some(true), None, "active"),
            (None, Some(true), "backup"),
        ];

        for (validate_active, validate_inactive, expected) in test_cases {
            let team = Team {
                link_watch: Some(LinkWatch {
                    watches: vec![Watch {
                        name: WatchName::ArpPing,
                        interval: 100,
                        target_host: Some("192.168.1.1".to_string()),
                        validate_active,
                        validate_inactive,
                        ..Default::default()
                    }],
                }),
                ..Default::default()
            };

            let config: ConnectionConfig = (&team).into();
            if let ConnectionConfig::Bond(bond) = config {
                assert_eq!(bond.options.0.get("arp_validate").unwrap(), expected);
            }
        }
    }

    #[test]
    fn test_tipc_watch_ignored() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![Watch {
                    name: WatchName::Tipc,
                    bearer: Some("eth:eth0".to_string()),
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            assert!(bond.options.0.is_empty());
        }
    }

    #[test]
    fn test_team_full_deserialization() {
        let xml = r#"
            <team>
                <debug_level>5</debug_level>
                <notify_peers>
                    <count>3</count>
                    <interval>100</interval>
                </notify_peers>
                <mcast_rejoin>
                    <count>2</count>
                    <interval>50</interval>
                </mcast_rejoin>
                <link_watch_policy>any</link_watch_policy>
                <address>02:00:00:00:00:01</address>
                <runner name="activebackup">
                    <hwaddr_policy>by_active</hwaddr_policy>
                </runner>
                <link_watch>
                    <watch name="arp_ping">
                        <interval>100</interval>
                        <init_wait>10</init_wait>
                        <target_host>192.168.1.1</target_host>
                        <source_host>192.168.1.100</source_host>
                        <validate_active>true</validate_active>
                        <validate_inactive>false</validate_inactive>
                        <send_always>false</send_always>
                        <missed_max>5</missed_max>
                        <vlanid>10</vlanid>
                    </watch>
                </link_watch>
            </team>
        "#;
        let mut deserializer = quick_xml::de::Deserializer::from_str(xml);
        let team: Team = Team::deserialize(&mut deserializer).unwrap();

        assert_eq!(team.debug_level, Some(5));
        assert_eq!(team.notify_peers.as_ref().unwrap().count, Some(3));
        assert_eq!(team.notify_peers.as_ref().unwrap().interval, Some(100));
        assert_eq!(team.mcast_rejoin.as_ref().unwrap().count, Some(2));
        assert_eq!(team.mcast_rejoin.as_ref().unwrap().interval, Some(50));
        assert_eq!(team.link_watch_policy.as_ref().unwrap(), "any");
        assert_eq!(team.address.as_ref().unwrap(), "02:00:00:00:00:01");

        let runner = team.runner.as_ref().unwrap();
        assert_eq!(runner.name, RunnerName::ActiveBackup);
        assert_eq!(
            runner.hwaddr_policy.as_ref().unwrap(),
            &HwAddrPolicy::ByActive
        );

        let link_watch = team.link_watch.as_ref().unwrap();
        assert_eq!(link_watch.watches.len(), 1);
        let watch = &link_watch.watches[0];
        assert_eq!(watch.name, WatchName::ArpPing);
        assert_eq!(watch.interval, 100);
        assert_eq!(watch.init_wait, 10);
        assert_eq!(watch.target_host.as_ref().unwrap(), "192.168.1.1");
        assert_eq!(watch.source_host.as_ref().unwrap(), "192.168.1.100");
        assert_eq!(watch.validate_active, Some(true));
        assert_eq!(watch.validate_inactive, Some(false));
        assert_eq!(watch.send_always, Some(false));
        assert_eq!(watch.missed_max, 5);
        assert_eq!(watch.vlanid, Some(10));
    }

    #[test]
    fn test_multiple_link_watches() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 100,
                        delay_down: 200,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("10.0.0.1".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let config: ConnectionConfig = (&team).into();
        if let ConnectionConfig::Bond(bond) = config {
            // Bond only supports one monitoring method - ethtool (miimon) is preferred
            assert_eq!(bond.options.0.get("miimon").unwrap(), "100");
            assert_eq!(bond.options.0.get("updelay").unwrap(), "100");
            assert_eq!(bond.options.0.get("downdelay").unwrap(), "200");
            // arp_interval should NOT be set (bond can't use both)
            assert!(bond.options.0.get("arp_interval").is_none());
            assert!(bond.options.0.get("arp_ip_target").is_none());
        }
    }

    #[test]
    fn test_warnings_tracking() {
        // Test that unsupported features generate warnings
        let team_with_warnings = Team {
            link_watch_policy: Some("any".to_string()),
            mcast_rejoin: Some(McastRejoin {
                count: Some(3),
                interval: Some(100),
            }),
            runner: Some(Runner {
                name: RunnerName::Random,
                ..Default::default()
            }),
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 100,
                        target_host: Some("192.168.1.1".to_string()),
                        source_host: Some("192.168.1.100".to_string()),
                        init_wait: 50,
                        send_always: Some(true),
                        missed_max: 3,
                        vlanid: Some(100),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 100,
                        target_host: Some("fe80::1".to_string()),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::Tipc,
                        bearer: Some("eth:eth0".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (_, has_warnings) = team_with_warnings.to_connection_config();
        assert!(has_warnings, "Expected warnings for unsupported features");

        // Test that supported features don't generate warnings
        let team_without_warnings = Team {
            runner: Some(Runner {
                name: RunnerName::Lacp,
                select_policy: SelectPolicy::Bandwidth,
                ..Default::default()
            }),
            link_watch: Some(LinkWatch {
                watches: vec![Watch {
                    name: WatchName::Ethtool,
                    delay_up: 10,
                    delay_down: 20,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let (_, has_warnings) = team_without_warnings.to_connection_config();
        assert!(!has_warnings, "Expected no warnings for supported features");
    }

    #[test]
    fn test_multiple_arp_ping_targets() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        validate_active: Some(true),
                        validate_inactive: Some(false),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("8.8.8.8".to_string()),
                        validate_active: Some(true),
                        validate_inactive: Some(false),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(
            !has_warnings,
            "Same interval and validation should not warn"
        );

        if let ConnectionConfig::Bond(bond) = config {
            // Both targets should be combined into comma-separated list
            assert_eq!(
                bond.options.0.get("arp_ip_target").unwrap(),
                "192.168.1.1,8.8.8.8"
            );
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(bond.options.0.get("arp_validate").unwrap(), "active");
        }
    }

    #[test]
    fn test_multiple_arp_ping_different_intervals() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        missed_max: 3,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 2000,
                        target_host: Some("8.8.8.8".to_string()),
                        missed_max: 5,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(
            has_warnings,
            "Different intervals and missed_max should warn"
        );

        if let ConnectionConfig::Bond(bond) = config {
            // Should use first interval and first missed_max
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(bond.options.0.get("arp_missed_max").unwrap(), "3");
            assert_eq!(
                bond.options.0.get("arp_ip_target").unwrap(),
                "192.168.1.1,8.8.8.8"
            );
        }
    }

    #[test]
    fn test_multiple_arp_ping_different_validation() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        validate_active: Some(true),
                        validate_inactive: Some(false),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("8.8.8.8".to_string()),
                        validate_active: Some(true),
                        validate_inactive: Some(true),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "Different validation settings should warn");

        if let ConnectionConfig::Bond(bond) = config {
            // Should use first validation setting
            assert_eq!(bond.options.0.get("arp_validate").unwrap(), "active");
            assert_eq!(
                bond.options.0.get("arp_ip_target").unwrap(),
                "192.168.1.1,8.8.8.8"
            );
        }
    }

    #[test]
    fn test_single_ethtool_no_warnings() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![Watch {
                    name: WatchName::Ethtool,
                    delay_up: 10,
                    delay_down: 20,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let (_, has_warnings) = team.to_connection_config();
        assert!(!has_warnings, "Single ethtool should not warn");
    }

    #[test]
    fn test_single_arp_ping_no_warnings() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![Watch {
                    name: WatchName::ArpPing,
                    interval: 1000,
                    target_host: Some("192.168.1.1".to_string()),
                    missed_max: 5,
                    ..Default::default()
                }],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(!has_warnings, "Single arp_ping should not warn");

        if let ConnectionConfig::Bond(bond) = config {
            assert_eq!(bond.options.0.get("arp_interval").unwrap(), "1000");
            assert_eq!(bond.options.0.get("arp_ip_target").unwrap(), "192.168.1.1");
            assert_eq!(bond.options.0.get("arp_missed_max").unwrap(), "5");
        }
    }

    #[test]
    fn test_ethtool_and_arp_ping_warns() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 10,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "Both ethtool and arp_ping should warn");

        if let ConnectionConfig::Bond(bond) = config {
            // Should use ethtool (miimon), not arp_ping
            assert!(bond.options.0.get("miimon").is_some());
            assert!(bond.options.0.get("arp_interval").is_none());
        }
    }

    #[test]
    fn test_ethtool_and_nsna_warns() {
        testing_logger::setup();

        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 10,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::NsnaPing,
                        interval: 1000,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (_, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "nsna_ping should warn");

        testing_logger::validate(|captured_logs| {
            assert!(captured_logs
                .iter()
                .any(|log| log.body.contains("nsna_ping")));
        });
    }

    #[test]
    fn test_arp_ping_and_tipc_warns() {
        testing_logger::setup();

        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::ArpPing,
                        interval: 1000,
                        target_host: Some("192.168.1.1".to_string()),
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::Tipc,
                        bearer: Some("eth:eth0".to_string()),
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (_, has_warnings) = team.to_connection_config();
        assert!(has_warnings, "tipc should warn");

        testing_logger::validate(|captured_logs| {
            assert!(captured_logs.iter().any(|log| log.body.contains("tipc")));
        });
    }

    #[test]
    fn test_multiple_ethtool_uses_first() {
        let team = Team {
            link_watch: Some(LinkWatch {
                watches: vec![
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 10,
                        delay_down: 20,
                        ..Default::default()
                    },
                    Watch {
                        name: WatchName::Ethtool,
                        delay_up: 30,
                        delay_down: 40,
                        ..Default::default()
                    },
                ],
            }),
            ..Default::default()
        };

        let (config, has_warnings) = team.to_connection_config();
        assert!(!has_warnings, "Multiple ethtool watches should not warn");

        if let ConnectionConfig::Bond(bond) = config {
            // Should use first ethtool watch
            assert_eq!(bond.options.0.get("miimon").unwrap(), "10"); // GCD(10, 20) = 10
            assert_eq!(bond.options.0.get("updelay").unwrap(), "10");
            assert_eq!(bond.options.0.get("downdelay").unwrap(), "20");
        }
    }

    #[test]
    fn test_miimon_gcd_calculation() {
        // Test GCD calculation for miimon
        let test_cases = vec![
            (15, 25, "5"),     // GCD(15, 25) = 5
            (10, 20, "10"),    // GCD(10, 20) = 10
            (100, 200, "100"), // GCD(100, 200) = 100
            (7, 13, "1"),      // GCD(7, 13) = 1 (coprime)
            (0, 50, "50"),     // Only delay_down
            (50, 0, "50"),     // Only delay_up
            (0, 0, "100"),     // No delays, use default
        ];

        for (delay_up, delay_down, expected_miimon) in test_cases {
            let team = Team {
                link_watch: Some(LinkWatch {
                    watches: vec![Watch {
                        name: WatchName::Ethtool,
                        delay_up,
                        delay_down,
                        ..Default::default()
                    }],
                }),
                ..Default::default()
            };

            let (config, _) = team.to_connection_config();
            if let ConnectionConfig::Bond(bond) = config {
                assert_eq!(
                    bond.options.0.get("miimon").unwrap(),
                    expected_miimon,
                    "Failed for delay_up={}, delay_down={}",
                    delay_up,
                    delay_down
                );
            }
        }
    }
}
