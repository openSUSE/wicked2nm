use agama_dbus_server::network::model;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Bridge {
    #[serde(default)]
    pub stp: bool,
    pub priority: Option<u16>,
    pub forward_delay: Option<f32>,
    pub hello_time: Option<f32>,
    pub max_age: Option<f32>,
    pub aging_time: Option<f32>, // wicked uses US english, but kernel and nm UK (ageing)
    #[serde(deserialize_with = "unwrap_ports")]
    pub ports: Vec<BridgePort>,
    pub address: Option<String>,
}

fn unwrap_ports<'de, D>(deserializer: D) -> Result<Vec<BridgePort>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Debug, PartialEq, Default, Serialize, Deserialize)]
    struct BridgePorts {
        port: Vec<BridgePort>,
    }
    Ok(BridgePorts::deserialize(deserializer)?.port)
}

#[skip_serializing_none]
#[derive(Debug, PartialEq, Serialize, Deserialize, Clone)]
#[serde(rename_all = "kebab-case")]
pub struct BridgePort {
    pub device: String,
    pub priority: Option<u32>,
    pub path_cost: Option<u32>,
}

impl From<&Bridge> for model::ConnectionConfig {
    fn from(bridge: &Bridge) -> model::ConnectionConfig {
        model::ConnectionConfig::Bridge(model::BridgeConfig {
            stp: bridge.stp,
            priority: bridge.priority.map(|v| v as u32),
            forward_delay: bridge.forward_delay.map(|v| v.round() as u32),
            hello_time: bridge.hello_time.map(|v| v.round() as u32),
            max_age: bridge.max_age.map(|v| v.round() as u32),
            ageing_time: bridge.aging_time.map(|v| v.round() as u32),
        })
    }
}

impl From<&BridgePort> for model::PortConfig {
    fn from(port: &BridgePort) -> model::PortConfig {
        model::PortConfig::Bridge(model::BridgePortConfig {
            priority: port.priority,
            path_cost: port.path_cost,
        })
    }
}
