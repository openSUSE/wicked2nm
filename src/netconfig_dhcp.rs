use serde::Serialize;
use std::path::Path;

#[derive(Debug, Default, PartialEq, Serialize)]
pub struct NetconfigDhcp {
    pub dhclient_hostname_option: HostnameOption,
    pub dhclient6_hostname_option: HostnameOption,
}

#[derive(Default, Debug, PartialEq, Serialize)]
pub enum HostnameOption {
    #[default]
    Empty,
    Auto,
    Value(String),
}

impl From<String> for HostnameOption {
    fn from(s: String) -> Self {
        match s.as_str() {
            "" => Self::Empty,
            "AUTO" => Self::Auto,
            _ => Self::Value(s),
        }
    }
}

pub fn read_netconfig_dhcp(path: impl AsRef<Path>) -> Result<NetconfigDhcp, anyhow::Error> {
    if let Err(e) = dotenv::from_filename(path) {
        return Err(e.into());
    };
    Ok(handle_netconfig_dhcp_values())
}

fn handle_netconfig_dhcp_values() -> NetconfigDhcp {
    let mut netconfig_dhcp = NetconfigDhcp::default();
    if let Ok(dhclient_hostname_option) = dotenv::var("DHCLIENT_HOSTNAME_OPTION") {
        netconfig_dhcp.dhclient_hostname_option = dhclient_hostname_option.into();
    }
    if let Ok(dhclient6_hostname_option) = dotenv::var("DHCLIENT6_HOSTNAME_OPTION") {
        netconfig_dhcp.dhclient6_hostname_option = dhclient6_hostname_option.into();
    }
    netconfig_dhcp
}
