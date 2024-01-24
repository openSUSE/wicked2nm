use crate::MIGRATION_SETTINGS;
use agama_dbus_server::network::model::{self, WEPAuthAlg, WEPKeyType, WEPSecurity};
use agama_lib::network::types::SSID;
use anyhow::anyhow;
use macaddr::MacAddr6;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::formats::CommaSeparator;
use serde_with::StringWithSeparator;
use serde_with::{serde_as, skip_serializing_none, DeserializeFromStr, SerializeDisplay};
use std::str::FromStr;
use strum_macros::{Display, EnumString};

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Wireless {
    #[serde(rename = "ap-scan")]
    pub ap_scan: u32,
    #[serde(default)]
    #[serde(deserialize_with = "unwrap_wireless_networks")]
    pub networks: Option<Vec<Network>>,
}

#[serde_as]
#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Network {
    pub essid: String,
    // #[serde(rename = "scan-ssid")]
    // pub scan_ssid: bool,
    pub mode: WickedWirelessMode,
    #[serde(rename = "wpa-psk")]
    pub wpa_psk: Option<WpaPsk>,
    #[serde(default)]
    #[serde(rename = "key-management")]
    #[serde_as(as = "StringWithSeparator::<CommaSeparator, String>")]
    pub key_management: Vec<String>,
    pub channel: Option<u32>,
    #[serde(rename = "access-point")]
    pub access_point: Option<String>,
    pub wep: Option<Wep>,
}

#[derive(Default, Debug, PartialEq, SerializeDisplay, DeserializeFromStr, EnumString, Display)]
#[strum(serialize_all = "kebab-case")]
pub enum WickedWirelessMode {
    AdHoc = 0,
    #[default]
    Infrastructure = 1,
    AP = 2,
}

impl From<&WickedWirelessMode> for model::WirelessMode {
    fn from(value: &WickedWirelessMode) -> Self {
        match value {
            WickedWirelessMode::AdHoc => model::WirelessMode::AdHoc,
            WickedWirelessMode::Infrastructure => model::WirelessMode::Infra,
            WickedWirelessMode::AP => model::WirelessMode::AP,
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct WpaPsk {
    pub passphrase: String,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Wep {
    #[serde(rename = "auth-algo")]
    pub auth_algo: String,
    #[serde(rename = "default-key")]
    pub default_key: u32,
    pub key: Vec<String>,
}

fn unwrap_wireless_networks<'de, D>(deserializer: D) -> Result<Option<Vec<Network>>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
    struct Networks {
        network: Vec<Network>,
    }
    Ok(Some(Networks::deserialize(deserializer)?.network))
}

fn wireless_security_protocol(
    wicked_value: &[String],
) -> Result<model::SecurityProtocol, anyhow::Error> {
    if wicked_value.contains(&"wpa-psk".to_string())
        || wicked_value.contains(&"wpa-psk-sha256".to_string())
    {
        Ok(model::SecurityProtocol::WPA2)
    } else if wicked_value.contains(&"sae".to_string()) {
        Ok(model::SecurityProtocol::WPA3Personal)
    } else if wicked_value.contains(&"wpa-eap".to_string())
        || wicked_value.contains(&"wpa-eap-sha256".to_string())
    {
        Ok(model::SecurityProtocol::WPA2Enterprise)
    } else if wicked_value.contains(&"owe".to_string()) {
        Ok(model::SecurityProtocol::OWE)
    } else if wicked_value.contains(&"wpa-eap-suite-b-192".to_string()) {
        Ok(model::SecurityProtocol::WPA3Only)
    } else if wicked_value.contains(&"none".to_string()) {
        Ok(model::SecurityProtocol::WEP)
    } else {
        Err(anyhow!("Unrecognized key-management protocol"))
    }
}

impl TryFrom<&Network> for model::ConnectionConfig {
    type Error = anyhow::Error;
    fn try_from(network: &Network) -> Result<Self, Self::Error> {
        let settings = MIGRATION_SETTINGS.get().unwrap();
        let mut config = model::WirelessConfig {
            ssid: SSID(network.essid.as_bytes().to_vec()),
            ..Default::default()
        };

        if network.key_management.len() > 1 && settings.continue_migration {
            log::warn!("Migration of multiple key-management algorithms isn't supported")
        } else if network.key_management.len() > 1 {
            return Err(anyhow!(
                "Migration of multiple key-management algorithms isn't supported"
            ));
        }
        config.security = wireless_security_protocol(&network.key_management)?;

        if let Some(wpa_psk) = &network.wpa_psk {
            config.password = Some(wpa_psk.passphrase.clone())
        }
        if let Some(channel) = network.channel {
            config.channel = Some(channel);
            if channel <= 14 {
                config.band = Some("bg".try_into().unwrap());
            } else {
                config.band = Some("a".try_into().unwrap());
            }
            log::warn!(
                "NetworkManager requires setting a band for wireless when a channel is set. The band has been set to \"{}\". This may in certain regions be incorrect.",
                config.band.unwrap()
            );
        }
        if let Some(access_point) = &network.access_point {
            config.bssid = Some(MacAddr6::from_str(access_point)?);
        }

        if let Some(wep) = &network.wep {
            // filter out `s:`, `h:`, `:`, and `-` of wep keys
            let keys: Vec<String> = wep
                .key
                .clone()
                .into_iter()
                .map(|mut x| {
                    x = x.replace("s:", "");
                    x = x.replace("h:", "");
                    x = x.replace(':', "");
                    x.replace('-', "")
                })
                .collect();
            let wep_security = WEPSecurity {
                auth_alg: WEPAuthAlg::try_from(wep.auth_algo.as_str())?,
                wep_key_type: WEPKeyType::Key,
                keys,
                wep_key_index: wep.default_key,
            };
            config.wep_security = Some(wep_security);
        }

        config.mode = (&network.mode).into();
        Ok(model::ConnectionConfig::Wireless(config))
    }
}
