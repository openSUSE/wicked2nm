use crate::interface::{Interface, ParentKind};
use quick_xml::de::from_str;
use regex::Regex;
use std::collections::HashMap;
use std::fs::{self, read_dir};
use std::path::{Path, PathBuf};

pub fn read_xml(str: &str) -> Result<Vec<Interface>, quick_xml::DeError> {
    from_str(replace_colons(str).as_str())
}

pub fn read_xml_file(path: PathBuf) -> Result<Vec<Interface>, anyhow::Error> {
    let contents = match fs::read_to_string(path.clone()) {
        Ok(contents) => contents,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Couldn't read {}: {}",
                path.as_path().display(),
                e
            ))
        }
    };
    Ok(read_xml(contents.as_str())?)
}

fn replace_colons(colon_string: &str) -> String {
    let re = Regex::new(r"<([\/]?)(\w+):(\w+)\b").unwrap();
    let replaced = re.replace_all(colon_string, "<$1$2-$3").to_string();
    replaced
}

pub fn post_process_interface(interfaces: &mut [Interface]) {
    let mut helper = HashMap::new();
    for (idx, i) in interfaces.iter().enumerate() {
        if let Some(parent) = &i.link.parent {
            for j in interfaces.iter() {
                if j.name == *parent && j.bond.is_some() {
                    helper.insert(idx, Some(ParentKind::Bond));
                }
            }
        }
    }
    for (_, (k, v)) in helper.iter().enumerate() {
        if let Some(ifc) = interfaces.get_mut(*k) {
            ifc.link.kind = v.clone();
        }
    }
}

// https://stackoverflow.com/a/76820878
fn recurse_files(path: impl AsRef<Path>) -> std::io::Result<Vec<PathBuf>> {
    let mut buf = vec![];
    let entries = read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;

        if meta.is_dir() {
            let mut subdir = recurse_files(entry.path())?;
            buf.append(&mut subdir);
        }

        if meta.is_file() {
            buf.push(entry.path());
        }
    }

    Ok(buf)
}

pub fn read(paths: Vec<String>) -> Result<Vec<Interface>, anyhow::Error> {
    let mut interfaces: Vec<Interface> = vec![];
    for path in paths {
        let path: PathBuf = path.into();
        if path.is_dir() {
            let files = recurse_files(path)?;
            for file in files {
                interfaces.append(&mut read_xml_file(file)?);
            }
        } else {
            interfaces.append(&mut read_xml_file(path)?);
        }
    }
    post_process_interface(&mut interfaces);
    Ok(interfaces)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interface::*;

    #[test]
    fn test_bond_options_from_xml() {
        let xml = r##"
            <interface>
                <name>bond0</name>
                <bond>
                    <mode>active-backup</mode>
                    <xmit-hash-policy>layer34</xmit-hash-policy>
                    <fail-over-mac>none</fail-over-mac>
                    <packets-per-slave>1</packets-per-slave>
                    <tlb-dynamic-lb>true</tlb-dynamic-lb>
                    <lacp-rate>slow</lacp-rate>
                    <ad-select>bandwidth</ad-select>
                    <ad-user-port-key>5</ad-user-port-key>
                    <ad-actor-sys-prio>7</ad-actor-sys-prio>
                    <ad-actor-system>00:de:ad:be:ef:00</ad-actor-system>
                    <min-links>11</min-links>
                    <primary-reselect>better</primary-reselect>
                    <num-grat-arp>13</num-grat-arp>
                    <num-unsol-na>17</num-unsol-na>
                    <lp-interval>19</lp-interval>
                    <resend-igmp>23</resend-igmp>
                    <all-slaves-active>true</all-slaves-active>
                    <slaves>
                        <slave><device>en0</device></slave>
                    </slaves>
                    <miimon>
                        <frequency>23</frequency>
                        <updelay>27</updelay>
                        <downdelay>31</downdelay>
                        <carrier-detect>ioctl</carrier-detect>
                    </miimon>
                    <arpmon>
                        <interval>23</interval>
                        <validate>filter_backup</validate>
                        <validate-targets>any</validate-targets>
                        <targets>
                            <ipv4-address>1.2.3.4</ipv4-address>
                            <ipv4-address>4.3.2.1</ipv4-address>
                        </targets>
                    </arpmon>
                    <address>02:11:22:33:44:55</address>
                </bond>
            </interface>
            "##;
        let ifc = read_xml(xml).unwrap().pop().unwrap();
        assert!(ifc.bond.is_some());
        let bond = ifc.bond.unwrap();

        assert_eq!(
            bond,
            Bond {
                mode: BondMode::ActiveBackup,
                xmit_hash_policy: Some(XmitHashPolicy::Layer34),
                fail_over_mac: Some(FailOverMac::None),
                packets_per_slave: Some(1),
                tlb_dynamic_lb: Some(true),
                lacp_rate: Some(LacpRate::Slow),
                ad_select: Some(AdSelect::Bandwidth),
                ad_user_port_key: Some(5),
                ad_actor_sys_prio: Some(7),
                ad_actor_system: Some(String::from("00:de:ad:be:ef:00")),
                min_links: Some(11),
                primary_reselect: Some(PrimaryReselect::Better),
                num_grat_arp: Some(13),
                num_unsol_na: Some(17),
                lp_interval: Some(19),
                resend_igmp: Some(23),
                all_slaves_active: Some(true),
                slaves: vec![Slave {
                    device: String::from("en0"),
                    primary: None
                }],
                miimon: Some(Miimon {
                    frequency: 23,
                    carrier_detect: CarrierDetect::Ioctl,
                    downdelay: Some(31),
                    updelay: Some(27),
                }),
                arpmon: Some(ArpMon {
                    interval: 23,
                    validate: ArpValidate::FilterBackup,
                    validate_targets: Some(ArpValidateTargets::Any),
                    targets: vec![String::from("1.2.3.4"), String::from("4.3.2.1")]
                }),
                address: Some(String::from("02:11:22:33:44:55")),
            }
        );
    }

    #[test]
    fn test_broken_xml() {
        let xml = r##"
            <interface>
                <name>eth1</name>
                <ipv4:static>
                  <address>127.0.0.1</>
                </ipv4:static>
            </interface>
            "##;
        let err = read_xml(xml);
        assert!(err.is_err());
    }
}
